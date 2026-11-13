//! File safety mechanisms for mutation testing.
//!
//! Provides RAII-based guards to ensure source files are always restored
//! to their original state, even if a panic occurs during mutation testing.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;

use crate::core::{Error, Result};

/// Global counter for generating unique temp file names across threads.
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
type ActiveMutations = Arc<parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>>;

static ACTIVE_MUTATIONS: LazyLock<ActiveMutations> =
    LazyLock::new(|| Arc::new(parking_lot::Mutex::new(HashMap::new())));
static SIGNAL_HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn mark_interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn register_interrupt_handler() -> std::result::Result<(), String> {
    let previous = unsafe {
        libc::signal(
            libc::SIGINT,
            mark_interrupted as *const () as libc::sighandler_t,
        )
    };
    if previous == libc::SIG_ERR {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
extern "system" fn mark_interrupted_windows(signal: u32) -> i32 {
    const CTRL_C_EVENT: u32 = 0;
    if signal == CTRL_C_EVENT {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

#[cfg(windows)]
fn register_interrupt_handler() -> std::result::Result<(), String> {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: extern "system" fn(u32) -> i32, add: i32) -> i32;
    }

    if unsafe { SetConsoleCtrlHandler(mark_interrupted_windows, 1) } == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

/// RAII guard that ensures a file is restored to its original content.
///
/// When the guard is dropped (whether normally or due to panic), it restores
/// the original file content.
pub struct MutationGuard {
    /// Path to the file being mutated.
    path: PathBuf,
    /// Original file content (backup).
    original: Vec<u8>,
    /// Whether the file has been modified.
    modified: bool,
    active_mutations: ActiveMutations,
    interrupted: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl MutationGuard {
    /// Create a new mutation guard for the given file.
    ///
    /// Reads and stores the original file content for later restoration.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_state(path, Arc::clone(&ACTIVE_MUTATIONS), None)
    }

    fn new_with_state(
        path: impl AsRef<Path>,
        active_mutations: ActiveMutations,
        interrupted: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let original = fs::read(&path).map_err(Error::Io)?;

        Ok(Self {
            path,
            original,
            modified: false,
            active_mutations,
            interrupted,
        })
    }

    fn is_interrupted(&self) -> bool {
        self.interrupted
            .as_deref()
            .unwrap_or(&INTERRUPTED)
            .load(Ordering::SeqCst)
    }

    /// Apply a mutation to the file.
    ///
    /// This writes the mutated content to the file atomically.
    pub fn apply(&mut self, content: &[u8]) -> Result<()> {
        let mut active = self.active_mutations.lock();
        if self.is_interrupted() {
            return Ok(());
        }
        active.insert(self.path.clone(), self.original.clone());
        if let Err(error) = atomic_write(&self.path, content) {
            active.remove(&self.path);
            return Err(error);
        }
        drop(active);
        self.modified = true;
        Ok(())
    }

    /// Get the original file content.
    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// Get the file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if the file has been modified.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Restore the original content without dropping the guard.
    ///
    /// This is useful for running multiple mutations in sequence.
    pub fn restore(&mut self) -> Result<()> {
        if self.modified {
            atomic_write(&self.path, &self.original)?;
            self.modified = false;
            self.active_mutations.lock().remove(&self.path);
        }
        Ok(())
    }
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        if self.modified {
            // Best-effort atomic restoration: write to temp, then rename.
            // Falls back to direct write if rename fails (e.g. cross-device).
            let parent = self.path.parent().unwrap_or(Path::new("."));
            let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let temp_path = parent.join(format!(
                ".omen-restore-{}-{}.tmp",
                std::process::id(),
                counter
            ));
            if File::create(&temp_path)
                .and_then(|mut f| {
                    f.write_all(&self.original)?;
                    f.sync_all()
                })
                .is_ok()
            {
                if fs::rename(&temp_path, &self.path).is_err() {
                    let _ = fs::remove_file(&temp_path);
                    let _ = fs::write(&self.path, &self.original);
                }
            } else {
                let _ = fs::remove_file(&temp_path);
                let _ = fs::write(&self.path, &self.original);
            }
        }
        self.active_mutations.lock().remove(&self.path);
    }
}

fn restore_registered_mutations(
    active_mutations: &parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>,
    interrupted: &std::sync::atomic::AtomicBool,
) {
    interrupted.store(true, Ordering::SeqCst);
    let active = active_mutations.lock();
    for (path, original) in active.iter() {
        let _ = atomic_write(path, original);
    }
}

#[cfg(unix)]
type ProcessGroups = Arc<parking_lot::Mutex<std::collections::HashSet<u32>>>;

/// The process groups of the test commands that run now. The interrupt
/// watcher stops them before it restores the files.
#[cfg(unix)]
static ACTIVE_PROCESS_GROUPS: LazyLock<ProcessGroups> =
    LazyLock::new(|| Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new())));

/// The longest time to wait for a killed group. SIGKILL cannot be caught, so
/// the usual time is a few milliseconds.
#[cfg(unix)]
pub const PROCESS_GROUP_EXIT_LIMIT: Duration = Duration::from_secs(5);

/// The process group of one test command.
///
/// A descendant of the test command can write a source file after the command
/// stops. Thus the group stays in the registry until it has no process: the
/// interrupt watcher must be able to kill it at each moment before that. Only
/// `drop` removes the registration, and `drop` first kills the group and waits
/// for it. This order also holds if the owner panics.
#[cfg(unix)]
pub struct ProcessGroup {
    group: u32,
    registry: ProcessGroups,
    exited: bool,
}

#[cfg(unix)]
impl ProcessGroup {
    /// Start the leader of a new process group and register the group.
    ///
    /// `spawn` must start a process that `process_group(0)` made a group
    /// leader. `id` gives the process id, which is also the group id.
    ///
    /// The start and the registration occur below one lock. The interrupt
    /// watcher takes the same lock, so it cannot run between the two steps and
    /// miss the group. After an interrupt, this function starts no process.
    pub fn spawn<C>(
        spawn: impl FnOnce() -> std::io::Result<C>,
        id: impl FnOnce(&C) -> Option<u32>,
    ) -> std::io::Result<(C, Option<Self>)> {
        Self::spawn_with(&ACTIVE_PROCESS_GROUPS, &INTERRUPTED, spawn, id)
    }

    fn spawn_with<C>(
        registry: &ProcessGroups,
        interrupted: &std::sync::atomic::AtomicBool,
        spawn: impl FnOnce() -> std::io::Result<C>,
        id: impl FnOnce(&C) -> Option<u32>,
    ) -> std::io::Result<(C, Option<Self>)> {
        let mut groups = registry.lock();
        if interrupted.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "the mutation run got an interrupt",
            ));
        }
        let child = spawn()?;
        let group = id(&child).map(|group| {
            groups.insert(group);
            Self {
                group,
                registry: Arc::clone(registry),
                exited: false,
            }
        });
        Ok((child, group))
    }

    /// Send SIGKILL to each process of the group. The group stays registered.
    pub fn kill(&self) {
        kill_group(self.group);
    }

    /// Tell if the group has a process.
    pub fn exists(&self) -> bool {
        group_exists(self.group)
    }

    /// Record that the group has no process. If it has one, do nothing.
    pub fn confirm_exit(&mut self) {
        self.exited = !self.exists();
    }

    /// Wait until the group has no process, for a maximum of
    /// `PROCESS_GROUP_EXIT_LIMIT`.
    pub fn wait_for_exit(&mut self) {
        wait_for_group_exit(self.group);
        self.confirm_exit();
    }
}

#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if !self.exited {
            self.kill();
            wait_for_group_exit(self.group);
        }
        self.registry.lock().remove(&self.group);
    }
}

#[cfg(unix)]
fn kill_group(group: u32) {
    // SAFETY: `killpg` has no memory effect. A group that has no process gives
    // an error, which is not important here.
    unsafe {
        libc::killpg(group as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn group_exists(group: u32) -> bool {
    // SAFETY: signal 0 does only the existence check.
    unsafe { libc::killpg(group as libc::pid_t, 0) == 0 }
}

#[cfg(unix)]
fn wait_for_group_exit(group: u32) {
    let start = std::time::Instant::now();
    while group_exists(group) && start.elapsed() < PROCESS_GROUP_EXIT_LIMIT {
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Kill each registered group and wait for it. The owner of each group
/// removes the registration, so a group stays visible here until it exits.
#[cfg(unix)]
fn terminate_registered_process_groups(registry: &ProcessGroups) {
    let groups: Vec<u32> = registry.lock().iter().copied().collect();
    for group in &groups {
        kill_group(*group);
    }
    for group in groups {
        wait_for_group_exit(group);
    }
}

#[cfg(unix)]
fn terminate_active_process_groups() {
    terminate_registered_process_groups(&ACTIVE_PROCESS_GROUPS);
}

pub fn restore_active_mutations() {
    restore_registered_mutations(&ACTIVE_MUTATIONS, &INTERRUPTED);
}

/// The exit status of a mutation run that got an interrupt.
pub const INTERRUPT_EXIT_CODE: i32 = 130;

/// True while a mutation run can have a mutated file or a test command.
///
/// The interrupt watcher and the run loop make the decision about an
/// interrupt below this lock, and the thread that sees the interrupt stops
/// the process while it holds the lock. Thus only one of the two threads
/// makes the decision, and the result does not depend on which thread is
/// faster.
static RUN_IS_ACTIVE: parking_lot::Mutex<bool> = parking_lot::Mutex::new(false);

/// Stops the test commands and restores each mutated file.
fn stop_and_restore(
    active_mutations: &parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>,
    interrupted: &std::sync::atomic::AtomicBool,
    stop_test_commands: impl FnOnce(),
) {
    // The test commands do not get the interrupt, because each has its own
    // process group. Stop them first: a command that runs can write a file
    // after the restore.
    stop_test_commands();
    restore_registered_mutations(active_mutations, interrupted);
}

/// The decision of the interrupt watcher. It gives the exit status when an
/// interrupt came during a run. After the run loop concluded the run, an
/// interrupt has no effect here.
fn watcher_decision(
    run_is_active: &bool,
    active_mutations: &parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>,
    interrupted: &std::sync::atomic::AtomicBool,
    stop_test_commands: impl FnOnce(),
) -> Option<i32> {
    if !*run_is_active || !interrupted.load(Ordering::SeqCst) {
        return None;
    }
    stop_and_restore(active_mutations, interrupted, stop_test_commands);
    Some(INTERRUPT_EXIT_CODE)
}

/// The decision of the run loop when it has no more mutants. It gives the
/// exit status when an interrupt came at a time before this call: before the
/// first test command, during a test command, or between two mutants. The
/// results are then not complete, because the loop starts no test command
/// after an interrupt.
///
/// If no interrupt came, the run is complete and this function ends it. An
/// interrupt after that moment does not change the exit status: the report
/// is complete and no file is mutated.
fn run_loop_decision(
    run_is_active: &mut bool,
    active_mutations: &parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>,
    interrupted: &std::sync::atomic::AtomicBool,
    stop_test_commands: impl FnOnce(),
) -> Option<i32> {
    if interrupted.load(Ordering::SeqCst) {
        stop_and_restore(active_mutations, interrupted, stop_test_commands);
        return Some(INTERRUPT_EXIT_CODE);
    }
    *run_is_active = false;
    None
}

fn stop_active_test_commands() {
    #[cfg(unix)]
    terminate_active_process_groups();
}

/// Tell the interrupt watcher that a mutation run starts.
///
/// `conclude_run` gives the interrupt its default effect again. Thus each run
/// registers the handler again, or an interrupt during a second run in one
/// process stops the process with a mutated file.
pub fn begin_run() -> Result<()> {
    #[cfg(unix)]
    register_interrupt_handler().map_err(Error::analysis)?;
    *RUN_IS_ACTIVE.lock() = true;
    Ok(())
}

/// End a mutation run after its last mutant.
///
/// If an interrupt came during the run, this function restores each file and
/// stops the process with the status 130. It does not return, thus the
/// caller cannot make a report from results that are not complete. If no
/// interrupt came, the run is complete, and a later interrupt has the
/// default effect of the operating system.
pub fn conclude_run() {
    let mut run_is_active = RUN_IS_ACTIVE.lock();
    if let Some(code) = run_loop_decision(
        &mut run_is_active,
        &ACTIVE_MUTATIONS,
        &INTERRUPTED,
        stop_active_test_commands,
    ) {
        std::process::exit(code);
    }
    // No file is mutated now. Give the interrupt its default effect again,
    // or the user cannot stop the process while it writes the report.
    #[cfg(unix)]
    // SAFETY: `signal` with `SIG_DFL` only changes the signal disposition.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

pub fn install_signal_handler() -> Result<()> {
    SIGNAL_HANDLER
        .get_or_init(|| {
            register_interrupt_handler()?;
            std::thread::Builder::new()
                .name("omen-mutation-signal".to_string())
                .spawn(|| loop {
                    {
                        let run_is_active = RUN_IS_ACTIVE.lock();
                        if let Some(code) = watcher_decision(
                            &run_is_active,
                            &ACTIVE_MUTATIONS,
                            &INTERRUPTED,
                            stop_active_test_commands,
                        ) {
                            // The lock stays held: the run loop must not
                            // continue while the process stops.
                            std::process::exit(code);
                        }
                    }
                    std::thread::park_timeout(Duration::from_millis(20));
                })
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .clone()
        .map_err(Error::analysis)
}

/// Write content to a file atomically.
///
/// This writes to a temporary file first, then renames it to the target path.
/// This ensures that the file is never in a partially-written state.
pub fn atomic_write(path: impl AsRef<Path>, content: &[u8]) -> Result<()> {
    let path = path.as_ref();

    // Get the parent directory for the temp file
    let parent = path.parent().unwrap_or(Path::new("."));

    // Create a temporary file in the same directory (unique per thread via atomic counter)
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = parent.join(format!(
        ".omen-mutation-{}-{}.tmp",
        std::process::id(),
        counter
    ));

    // Write to temp file
    let mut file = File::create(&temp_path).map_err(Error::Io)?;
    file.write_all(content).map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    drop(file);

    // Rename temp file to target (atomic on POSIX, near-atomic on Windows)
    fs::rename(&temp_path, path).map_err(Error::Io)?;

    Ok(())
}

/// Check if there are uncommitted changes in the given file.
///
/// This is a safety check to warn users before mutating files with uncommitted changes.
pub fn has_uncommitted_changes(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();

    // Try to find git root
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.join(".git").exists() {
            // Found git root, check if file has uncommitted changes
            let output = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .arg("--")
                .arg(path)
                .current_dir(d)
                .output();

            if let Ok(output) = output {
                return !output.stdout.is_empty();
            }
            return false;
        }
        dir = d.parent();
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn local_mutations() -> parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>> {
        parking_lot::Mutex::new(HashMap::new())
    }

    fn flag(value: bool) -> std::sync::atomic::AtomicBool {
        std::sync::atomic::AtomicBool::new(value)
    }

    /// Writes a mutated file and registers its original content.
    fn mutated_file(
        directory: &TempDir,
        mutations: &parking_lot::Mutex<HashMap<PathBuf, Vec<u8>>>,
    ) -> PathBuf {
        let path = directory.path().join("lib.rs");
        fs::write(&path, "mutated").unwrap();
        mutations.lock().insert(path.clone(), b"original".to_vec());
        path
    }

    #[test]
    fn test_run_loop_without_an_interrupt_concludes_the_run() {
        let mut run_is_active = true;
        let stopped = std::cell::Cell::new(false);

        let code = run_loop_decision(&mut run_is_active, &local_mutations(), &flag(false), || {
            stopped.set(true)
        });

        assert_eq!(code, None);
        assert!(!run_is_active);
        assert!(!stopped.get());
    }

    #[test]
    fn test_run_loop_with_an_interrupt_before_the_first_test_command_exits_130() {
        // No file is mutated and no test command runs.
        let mut run_is_active = true;

        let code = run_loop_decision(&mut run_is_active, &local_mutations(), &flag(true), || {});

        assert_eq!(code, Some(INTERRUPT_EXIT_CODE));
        assert!(run_is_active);
    }

    #[test]
    fn test_run_loop_with_an_interrupt_during_a_test_command_restores_the_file() {
        let directory = TempDir::new().unwrap();
        let mutations = local_mutations();
        let path = mutated_file(&directory, &mutations);
        let stopped = std::cell::Cell::new(false);
        let mut run_is_active = true;

        let code = run_loop_decision(&mut run_is_active, &mutations, &flag(true), || {
            stopped.set(true)
        });

        assert_eq!(code, Some(INTERRUPT_EXIT_CODE));
        assert!(stopped.get());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn test_run_loop_with_an_interrupt_between_two_mutants_exits_130() {
        // The guard of the last mutant restored its file, and the loop starts
        // no test command after an interrupt. Thus the loop gets to its end.
        let interrupted = flag(true);
        let mut run_is_active = true;

        let code = run_loop_decision(&mut run_is_active, &local_mutations(), &interrupted, || {});

        assert_eq!(code, Some(INTERRUPT_EXIT_CODE));
    }

    #[test]
    fn test_watcher_with_an_interrupt_during_a_run_restores_the_file() {
        let directory = TempDir::new().unwrap();
        let mutations = local_mutations();
        let path = mutated_file(&directory, &mutations);

        let code = watcher_decision(&true, &mutations, &flag(true), || {});

        assert_eq!(code, Some(INTERRUPT_EXIT_CODE));
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn test_watcher_without_an_interrupt_does_nothing() {
        assert_eq!(
            watcher_decision(&true, &local_mutations(), &flag(false), || {}),
            None
        );
    }

    #[test]
    fn test_an_interrupt_after_the_run_concluded_does_not_change_the_exit_status() {
        // The run loop saw no interrupt, thus the results are complete. The
        // report follows, and the exit status is the status of the report.
        let interrupted = flag(false);
        let mut run_is_active = true;
        run_loop_decision(&mut run_is_active, &local_mutations(), &interrupted, || {});

        interrupted.store(true, Ordering::SeqCst);

        assert_eq!(
            watcher_decision(&run_is_active, &local_mutations(), &interrupted, || {}),
            None
        );
    }

    #[cfg(unix)]
    fn local_registry() -> ProcessGroups {
        Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new()))
    }

    /// Starts a group leader that stays alive, and registers it in `registry`.
    #[cfg(unix)]
    fn spawn_sleeper(registry: &ProcessGroups) -> (std::process::Child, ProcessGroup) {
        let not_interrupted = std::sync::atomic::AtomicBool::new(false);
        let (child, group) = ProcessGroup::spawn_with(
            registry,
            &not_interrupted,
            || {
                let mut command = std::process::Command::new("sh");
                command.args(["-c", "sleep 30 & wait"]);
                std::os::unix::process::CommandExt::process_group(&mut command, 0);
                command.spawn()
            },
            |child| Some(child.id()),
        )
        .unwrap();
        (child, group.unwrap())
    }

    #[cfg(unix)]
    #[test]
    fn test_process_group_stays_registered_until_it_exits() {
        let registry = local_registry();
        let (mut child, mut group) = spawn_sleeper(&registry);
        let id = child.id();
        assert!(registry.lock().contains(&id));

        // The leader is not reaped, so the group has a process during the
        // wait. This is the interval in which an interrupt must see the group.
        group.kill();
        assert!(group.exists());
        assert!(registry.lock().contains(&id), "the kill removed the group");

        child.wait().unwrap();
        group.wait_for_exit();
        assert!(!group.exists());
        assert!(registry.lock().contains(&id), "the wait removed the group");

        drop(group);
        assert!(!registry.lock().contains(&id));
    }

    #[cfg(unix)]
    #[test]
    fn test_interrupt_path_kills_and_waits_for_a_registered_group() {
        let registry = local_registry();
        let (mut child, group) = spawn_sleeper(&registry);
        let id = child.id();
        let reaper = std::thread::spawn(move || child.wait().unwrap());

        terminate_registered_process_groups(&registry);

        // The function returned, so each process of the group stopped before
        // the file restore that follows it.
        assert!(!group.exists());
        assert!(registry.lock().contains(&id));
        assert!(!reaper.join().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn test_process_group_drop_kills_waits_and_removes_after_a_panic() {
        let registry = local_registry();
        let (mut child, group) = spawn_sleeper(&registry);
        let id = child.id();
        let reaper = std::thread::spawn(move || child.wait().unwrap());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _group = group;
            panic!("the owner of the group panics");
        }));

        assert!(result.is_err());
        assert!(!group_exists(id));
        assert!(!registry.lock().contains(&id));
        assert!(!reaper.join().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn test_no_test_command_starts_after_an_interrupt() {
        let registry = local_registry();
        let interrupted = std::sync::atomic::AtomicBool::new(true);
        let mut started = false;

        let result = ProcessGroup::spawn_with(
            &registry,
            &interrupted,
            || {
                started = true;
                std::process::Command::new("true").spawn()
            },
            |child| Some(child.id()),
        );

        assert!(result.is_err());
        assert!(!started);
        assert!(registry.lock().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_interrupt_path_waits_for_a_start_that_is_in_progress() {
        // The start and the registration occur below the registry lock. The
        // interrupt path needs the same lock, so it sees the new group.
        let registry = local_registry();
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawner = {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || {
                let not_interrupted = std::sync::atomic::AtomicBool::new(false);
                ProcessGroup::spawn_with(
                    &registry,
                    &not_interrupted,
                    || {
                        sender.send(()).unwrap();
                        // Give the interrupt path time to try the lock.
                        std::thread::sleep(Duration::from_millis(100));
                        let mut command = std::process::Command::new("sh");
                        command.args(["-c", "sleep 30 & wait"]);
                        std::os::unix::process::CommandExt::process_group(&mut command, 0);
                        command.spawn()
                    },
                    |child| Some(child.id()),
                )
                .unwrap()
            })
        };

        receiver.recv().unwrap();
        terminate_registered_process_groups(&registry);

        let (mut child, group) = spawner.join().unwrap();
        let group = group.unwrap();
        assert!(!child.wait().unwrap().success());
        assert!(!group.exists());
    }

    #[test]
    fn test_mutation_guard_new() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let guard = MutationGuard::new(&file_path).unwrap();

        assert_eq!(guard.original(), b"original content");
        assert_eq!(guard.path(), file_path);
        assert!(!guard.is_modified());
    }

    #[test]
    fn test_mutation_guard_new_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.rs");

        let result = MutationGuard::new(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_mutation_guard_apply() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated content").unwrap();

        assert!(guard.is_modified());

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"mutated content");
    }

    #[test]
    fn test_mutation_guard_restore() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated content").unwrap();
        guard.restore().unwrap();

        assert!(!guard.is_modified());

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_drop_restores() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        {
            let mut guard = MutationGuard::new(&file_path).unwrap();
            guard.apply(b"mutated content").unwrap();
            // Guard dropped here
        }

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_drop_does_nothing_if_not_modified() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        {
            let _guard = MutationGuard::new(&file_path).unwrap();
            // Guard dropped without modification
        }

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_multiple_mutations() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();

        // First mutation
        guard.apply(b"mutation 1").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"mutation 1");

        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");

        // Second mutation
        guard.apply(b"mutation 2").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"mutation 2");

        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");
    }

    #[test]
    fn test_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        atomic_write(&file_path, b"test content").unwrap();

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"test content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"old content").unwrap();

        atomic_write(&file_path, b"new content").unwrap();

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"new content");
    }

    #[test]
    fn test_atomic_write_no_temp_file_left() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        atomic_write(&file_path, b"content").unwrap();

        // Check no temp files left behind
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "test.rs");
    }

    #[test]
    fn test_has_uncommitted_changes_no_git() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"content").unwrap();

        // Should return false when not in a git repo
        assert!(!has_uncommitted_changes(&file_path));
    }

    #[test]
    fn test_has_uncommitted_changes_handles_option_like_filename() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("--output=stolen");
        std::process::Command::new("git")
            .arg("init")
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        fs::write(&file_path, b"content").unwrap();

        assert!(has_uncommitted_changes(&file_path));
    }

    #[test]
    fn test_restore_active_mutations_restores_modified_files() {
        let active_mutations = Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let interrupted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let mut guard = MutationGuard::new_with_state(
            &file_path,
            Arc::clone(&active_mutations),
            Some(Arc::clone(&interrupted)),
        )
        .unwrap();
        guard.apply(b"mutated").unwrap();

        restore_registered_mutations(&active_mutations, &interrupted);

        assert_eq!(fs::read(&file_path).unwrap(), b"original");
        guard.apply(b"mutated again").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[test]
    fn test_mutation_guard_apply_is_noop_after_interrupt() {
        let active_mutations = Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let interrupted = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let mut guard =
            MutationGuard::new_with_state(&file_path, active_mutations, Some(interrupted)).unwrap();

        guard.apply(b"mutated").unwrap();

        assert!(!guard.is_modified());
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[test]
    fn test_temp_file_names_unique_across_threads() {
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};

        let temp_dir = TempDir::new().unwrap();
        let collected = Arc::new(Mutex::new(HashSet::new()));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let dir = temp_dir.path().to_path_buf();
                let collected = Arc::clone(&collected);
                std::thread::spawn(move || {
                    let file = dir.join("src.rs");
                    fs::write(&file, b"original").unwrap();
                    for _ in 0..50 {
                        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
                        let name = format!(".omen-mutation-{}-{}.tmp", std::process::id(), counter);
                        collected.lock().unwrap().insert(name);
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        // 8 threads x 50 iterations = 400 unique names expected
        assert_eq!(collected.lock().unwrap().len(), 400);
    }

    #[test]
    fn test_drop_restores_atomically() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        {
            let mut guard = MutationGuard::new(&file_path).unwrap();
            guard.apply(b"mutated content").unwrap();
            // Guard dropped here -- should restore atomically
        }

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");

        // Verify no temp files left behind
        let temp_files: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".omen-restore-")
            })
            .collect();
        assert!(temp_files.is_empty());
    }
}
