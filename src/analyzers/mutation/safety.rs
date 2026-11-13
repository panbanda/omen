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

/// The parts of the process that the interrupt gate controls. The tests
/// replace them, thus they can make each sequence of events occur.
trait GateEnvironment {
    /// The flag that the interrupt handler sets.
    fn interrupted(&self) -> &std::sync::atomic::AtomicBool;
    /// Give the interrupt to the handler of omen.
    fn arm(&self) -> std::result::Result<(), String>;
    /// Give the interrupt its default effect again.
    fn disarm(&self);
    /// Stop each test command, and then restore each mutated file.
    fn stop_and_restore(&self);
    /// Stop the process. In the process, this function does not return.
    fn exit(&self, code: i32);
}

/// The single place that makes a decision about an interrupt.
///
/// The interrupt watcher and each run loop make their decision below one
/// lock, and the thread that sees the interrupt stops the process while it
/// holds the lock. Thus only one thread makes the decision, and the result
/// does not depend on which thread is faster.
///
/// The lock holds the number of mutation runs that are active in the process.
/// One process can have more than one run at the same time. The handler of
/// omen stays registered while the number is above zero.
struct InterruptGate {
    active_runs: parking_lot::Mutex<usize>,
}

impl InterruptGate {
    const fn new() -> Self {
        Self {
            active_runs: parking_lot::Mutex::new(0),
        }
    }

    /// Start a run. The first run registers the handler.
    ///
    /// An interrupt that came while no run was active is not for a run. The
    /// first run thus clears the flag, or it stops immediately.
    fn begin(&self, environment: &impl GateEnvironment) -> std::result::Result<(), String> {
        let mut active_runs = self.active_runs.lock();
        if *active_runs == 0 {
            environment.interrupted().store(false, Ordering::SeqCst);
            environment.arm()?;
        }
        *active_runs += 1;
        Ok(())
    }

    /// The decision of a run loop when it stops: after its last mutant, or
    /// when it returns an error.
    ///
    /// If an interrupt came at a time before this call, the function stops
    /// the test commands, restores each file, and stops the process. This
    /// applies to an interrupt before the first test command, during a test
    /// command, and between two mutants. The results are then not complete,
    /// because a run starts no test command after an interrupt.
    ///
    /// If no interrupt came, the run ends. The last run that ends gives the
    /// interrupt its default effect again. An interrupt after that moment
    /// does not change the exit status of the report.
    fn conclude(&self, environment: &impl GateEnvironment) {
        let mut active_runs = self.active_runs.lock();
        if environment.interrupted().load(Ordering::SeqCst) {
            environment.stop_and_restore();
            // The lock stays held: no other thread makes a decision while
            // the process stops.
            environment.exit(INTERRUPT_EXIT_CODE);
            return;
        }
        *active_runs = active_runs.saturating_sub(1);
        if *active_runs == 0 {
            environment.disarm();
        }
    }

    /// The decision of the interrupt watcher. An interrupt has an effect only
    /// while a run is active.
    fn watcher_step(&self, environment: &impl GateEnvironment) {
        let active_runs = self.active_runs.lock();
        if *active_runs > 0 && environment.interrupted().load(Ordering::SeqCst) {
            environment.stop_and_restore();
            // The lock stays held: no run loop continues while the process
            // stops.
            environment.exit(INTERRUPT_EXIT_CODE);
        }
    }

    #[cfg(test)]
    fn active_runs(&self) -> usize {
        *self.active_runs.lock()
    }
}

static INTERRUPT_GATE: InterruptGate = InterruptGate::new();

/// The gate environment of the process.
struct ProcessEnvironment;

impl GateEnvironment for ProcessEnvironment {
    fn interrupted(&self) -> &std::sync::atomic::AtomicBool {
        &INTERRUPTED
    }

    fn arm(&self) -> std::result::Result<(), String> {
        register_interrupt_handler()
    }

    fn disarm(&self) {
        remove_interrupt_handler();
    }

    fn stop_and_restore(&self) {
        // The test commands do not get the interrupt, because each has its
        // own process group. Stop them first: a command that runs can write
        // a file after the restore.
        stop_active_test_commands();
        restore_active_mutations();
    }

    fn exit(&self, code: i32) {
        std::process::exit(code);
    }
}

fn stop_active_test_commands() {
    #[cfg(unix)]
    terminate_active_process_groups();
    #[cfg(windows)]
    terminate_active_process_trees();
}

/// One active mutation run.
///
/// The drop ends the run, thus each return from the run loop makes the
/// decision about an interrupt: the usual end, an error, and a panic.
#[must_use = "the run ends when this value drops"]
pub struct ActiveRun {
    concluded: bool,
}

impl ActiveRun {
    /// End the run after its last mutant.
    ///
    /// If an interrupt came during the run, this function restores each file
    /// and stops the process with the status 130. It does not return, thus
    /// the caller cannot make a report from results that are not complete.
    pub fn conclude(mut self) {
        self.concluded = true;
        INTERRUPT_GATE.conclude(&ProcessEnvironment);
    }
}

impl Drop for ActiveRun {
    fn drop(&mut self) {
        if !self.concluded {
            INTERRUPT_GATE.conclude(&ProcessEnvironment);
        }
    }
}

/// Start a mutation run. The interrupt watcher protects the files of the run
/// until the returned value drops.
pub fn begin_run() -> Result<ActiveRun> {
    INTERRUPT_GATE
        .begin(&ProcessEnvironment)
        .map_err(Error::analysis)?;
    Ok(ActiveRun { concluded: false })
}

/// Start the thread that reads the interrupt flag. `begin_run` registers the
/// handler that sets the flag.
pub fn install_signal_handler() -> Result<()> {
    SIGNAL_HANDLER
        .get_or_init(|| {
            std::thread::Builder::new()
                .name("omen-mutation-signal".to_string())
                .spawn(|| loop {
                    INTERRUPT_GATE.watcher_step(&ProcessEnvironment);
                    std::thread::park_timeout(Duration::from_millis(20));
                })
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .clone()
        .map_err(Error::analysis)
}

#[cfg(unix)]
fn remove_interrupt_handler() {
    // SAFETY: `signal` with `SIG_DFL` only changes the signal disposition.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

#[cfg(windows)]
fn remove_interrupt_handler() {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: extern "system" fn(u32) -> i32, add: i32) -> i32;
    }

    // SAFETY: the call removes the handler that `register_interrupt_handler`
    // added. An error means that the handler was not present.
    unsafe {
        SetConsoleCtrlHandler(mark_interrupted_windows, 0);
    }
}

/// The process identifiers of the test commands that run now, on Windows.
#[cfg(windows)]
static ACTIVE_PROCESS_TREES: LazyLock<parking_lot::Mutex<std::collections::HashSet<u32>>> =
    LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashSet::new()));

/// One test command and its descendants, on Windows.
///
/// The interrupt watcher must stop the test commands before it restores the
/// files, as on Unix. The drop stops the processes that the command left.
///
/// No build of this project compiles or tests this code: the CI and the
/// releases have no Windows target.
#[cfg(windows)]
pub struct ProcessTree {
    id: u32,
}

#[cfg(windows)]
impl ProcessTree {
    /// Start a test command and register it. The start and the registration
    /// occur below one lock, which the interrupt watcher also takes. After an
    /// interrupt, this function starts no process.
    pub fn spawn<C>(
        spawn: impl FnOnce() -> std::io::Result<C>,
        id: impl FnOnce(&C) -> Option<u32>,
    ) -> std::io::Result<(C, Option<Self>)> {
        let mut trees = ACTIVE_PROCESS_TREES.lock();
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "the mutation run got an interrupt",
            ));
        }
        let child = spawn()?;
        let tree = id(&child).map(|id| {
            trees.insert(id);
            Self { id }
        });
        Ok((child, tree))
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        kill_process_tree(self.id);
        ACTIVE_PROCESS_TREES.lock().remove(&self.id);
    }
}

/// Stop a process and each of its descendants. `taskkill` returns after it
/// sent the stop request to each process.
#[cfg(windows)]
fn kill_process_tree(id: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &id.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
fn terminate_active_process_trees() {
    let trees: Vec<u32> = ACTIVE_PROCESS_TREES.lock().iter().copied().collect();
    for id in trees {
        kill_process_tree(id);
    }
}

/// The tests that use the gate and the signal disposition of the process.
/// They must not run at the same time.
#[cfg(all(test, unix))]
pub(crate) static PROCESS_GATE_TESTS: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// The handler of SIGINT that the operating system has now.
#[cfg(all(test, unix))]
pub(crate) fn sigint_disposition() -> libc::sighandler_t {
    // SAFETY: a null new action makes `sigaction` only read the action.
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current);
        current.sa_sigaction
    }
}

#[cfg(test)]
pub(crate) fn active_runs_of_the_process() -> usize {
    INTERRUPT_GATE.active_runs()
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

    /// A gate environment that records each call. `exit` can wait, as a
    /// process that stops does not continue.
    struct FakeEnvironment {
        interrupted: std::sync::atomic::AtomicBool,
        arm_calls: std::sync::atomic::AtomicUsize,
        arm_fails: bool,
        disarm_calls: std::sync::atomic::AtomicUsize,
        restore_calls: std::sync::atomic::AtomicUsize,
        exits: std::sync::mpsc::Sender<i32>,
        exit_codes: parking_lot::Mutex<std::sync::mpsc::Receiver<i32>>,
        /// `exit` waits for a message on this channel when it is present.
        stay_stopped: Option<parking_lot::Mutex<std::sync::mpsc::Receiver<()>>>,
    }

    impl FakeEnvironment {
        fn new(interrupted: bool) -> Self {
            let (exits, exit_codes) = std::sync::mpsc::channel();
            Self {
                interrupted: std::sync::atomic::AtomicBool::new(interrupted),
                arm_calls: std::sync::atomic::AtomicUsize::new(0),
                arm_fails: false,
                disarm_calls: std::sync::atomic::AtomicUsize::new(0),
                restore_calls: std::sync::atomic::AtomicUsize::new(0),
                exits,
                exit_codes: parking_lot::Mutex::new(exit_codes),
                stay_stopped: None,
            }
        }

        /// An environment whose `exit` waits until the returned sender drops
        /// or sends a message.
        fn that_stays_stopped(interrupted: bool) -> (Self, std::sync::mpsc::Sender<()>) {
            let (release, stay_stopped) = std::sync::mpsc::channel();
            let mut environment = Self::new(interrupted);
            environment.stay_stopped = Some(parking_lot::Mutex::new(stay_stopped));
            (environment, release)
        }

        fn calls(&self, counter: &std::sync::atomic::AtomicUsize) -> usize {
            counter.load(Ordering::SeqCst)
        }

        /// The exit codes that `exit` got until now.
        fn exit_codes(&self) -> Vec<i32> {
            self.exit_codes.lock().try_iter().collect()
        }

        /// Waits for the next call of `exit`. A decision that does not come
        /// makes the test fail, not hang.
        fn wait_for_exit(&self) -> Option<i32> {
            self.exit_codes
                .lock()
                .recv_timeout(Duration::from_secs(10))
                .ok()
        }
    }

    impl GateEnvironment for FakeEnvironment {
        fn interrupted(&self) -> &std::sync::atomic::AtomicBool {
            &self.interrupted
        }

        fn arm(&self) -> std::result::Result<(), String> {
            self.arm_calls.fetch_add(1, Ordering::SeqCst);
            if self.arm_fails {
                Err("no handler".to_string())
            } else {
                Ok(())
            }
        }

        fn disarm(&self) {
            self.disarm_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn stop_and_restore(&self) {
            self.restore_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn exit(&self, code: i32) {
            let _ = self.exits.send(code);
            if let Some(stay_stopped) = &self.stay_stopped {
                let _ = stay_stopped.lock().recv();
            }
        }
    }

    #[test]
    fn test_first_run_registers_the_handler_and_the_last_run_removes_it() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);

        gate.begin(&environment).unwrap();
        gate.begin(&environment).unwrap();
        assert_eq!(gate.active_runs(), 2);
        assert_eq!(environment.calls(&environment.arm_calls), 1);

        gate.conclude(&environment);
        assert_eq!(gate.active_runs(), 1);
        assert_eq!(environment.calls(&environment.disarm_calls), 0);

        gate.conclude(&environment);
        assert_eq!(gate.active_runs(), 0);
        assert_eq!(environment.calls(&environment.disarm_calls), 1);
        assert_eq!(environment.exit_codes(), Vec::<i32>::new());
    }

    #[test]
    fn test_an_interrupt_after_one_of_two_runs_concluded_protects_the_other_run() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();
        gate.begin(&environment).unwrap();
        gate.conclude(&environment);

        environment.interrupted.store(true, Ordering::SeqCst);
        gate.watcher_step(&environment);

        assert_eq!(environment.calls(&environment.restore_calls), 1);
        assert_eq!(environment.exit_codes(), [INTERRUPT_EXIT_CODE]);
    }

    #[test]
    fn test_first_run_clears_an_interrupt_that_came_while_no_run_was_active() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(true);

        gate.begin(&environment).unwrap();
        gate.watcher_step(&environment);

        assert!(!environment.interrupted.load(Ordering::SeqCst));
        assert_eq!(environment.exit_codes(), Vec::<i32>::new());
    }

    #[test]
    fn test_second_run_keeps_an_interrupt_that_came_during_the_first_run() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();
        environment.interrupted.store(true, Ordering::SeqCst);

        gate.begin(&environment).unwrap();

        assert!(environment.interrupted.load(Ordering::SeqCst));
    }

    #[test]
    fn test_run_that_cannot_register_the_handler_does_not_start() {
        let gate = InterruptGate::new();
        let mut environment = FakeEnvironment::new(false);
        environment.arm_fails = true;

        assert!(gate.begin(&environment).is_err());
        assert_eq!(gate.active_runs(), 0);
    }

    #[test]
    fn test_run_loop_without_an_interrupt_concludes_the_run() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();

        gate.conclude(&environment);

        assert_eq!(gate.active_runs(), 0);
        assert_eq!(environment.calls(&environment.restore_calls), 0);
        assert_eq!(environment.exit_codes(), Vec::<i32>::new());
    }

    /// The same decision applies to an interrupt before the first test
    /// command, during a test command, and between two mutants: the run loop
    /// starts no test command after an interrupt, thus it comes to this call.
    #[test]
    fn test_run_loop_with_an_interrupt_restores_and_exits_130() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();
        environment.interrupted.store(true, Ordering::SeqCst);

        gate.conclude(&environment);

        assert_eq!(environment.calls(&environment.restore_calls), 1);
        assert_eq!(environment.exit_codes(), [INTERRUPT_EXIT_CODE]);
        assert_eq!(gate.active_runs(), 1);
    }

    #[test]
    fn test_watcher_without_an_interrupt_does_nothing() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();

        gate.watcher_step(&environment);

        assert_eq!(environment.calls(&environment.restore_calls), 0);
        assert_eq!(environment.exit_codes(), Vec::<i32>::new());
    }

    /// The run loop saw no interrupt, thus the results are complete. The
    /// report follows, and the exit status is the status of the report.
    #[test]
    fn test_an_interrupt_after_the_last_run_concluded_has_no_effect() {
        let gate = InterruptGate::new();
        let environment = FakeEnvironment::new(false);
        gate.begin(&environment).unwrap();
        gate.conclude(&environment);

        environment.interrupted.store(true, Ordering::SeqCst);
        gate.watcher_step(&environment);

        assert_eq!(environment.calls(&environment.restore_calls), 0);
        assert_eq!(environment.exit_codes(), Vec::<i32>::new());
    }

    /// The sequence that gave the exit status 0: the run loop comes to its
    /// end before the watcher reads the flag. The run loop must make the
    /// decision itself, and the watcher must not make a second decision.
    #[test]
    fn test_run_loop_that_is_first_makes_the_decision_and_holds_the_watcher() {
        let gate = InterruptGate::new();
        let (environment, release) = FakeEnvironment::that_stays_stopped(false);
        gate.begin(&environment).unwrap();
        environment.interrupted.store(true, Ordering::SeqCst);

        std::thread::scope(|scope| {
            scope.spawn(|| gate.conclude(&environment));
            let decision = environment.wait_for_exit();
            // The run loop holds the gate while the process stops.
            let watcher_can_decide = gate.active_runs.try_lock().is_some();
            drop(release);

            assert_eq!(decision, Some(INTERRUPT_EXIT_CODE));
            assert!(!watcher_can_decide);
        });
        assert_eq!(environment.calls(&environment.restore_calls), 1);
    }

    /// The mirror sequence: the watcher reads the flag first. The run loop
    /// must not continue to its report while the process stops.
    #[test]
    fn test_watcher_that_is_first_makes_the_decision_and_holds_the_run_loop() {
        let gate = InterruptGate::new();
        let (environment, release) = FakeEnvironment::that_stays_stopped(false);
        gate.begin(&environment).unwrap();
        environment.interrupted.store(true, Ordering::SeqCst);

        std::thread::scope(|scope| {
            scope.spawn(|| gate.watcher_step(&environment));
            let decision = environment.wait_for_exit();
            // The watcher holds the gate while the process stops.
            let run_loop_can_continue = gate.active_runs.try_lock().is_some();
            drop(release);

            assert_eq!(decision, Some(INTERRUPT_EXIT_CODE));
            assert!(!run_loop_can_continue);
        });
        assert_eq!(environment.calls(&environment.restore_calls), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_registers_the_handler_and_its_end_gives_the_default_action() {
        let _serial = PROCESS_GATE_TESTS.lock();

        let run = begin_run().unwrap();
        let during_the_run = sigint_disposition();
        let second = begin_run().unwrap();
        run.conclude();
        let after_one_of_two_runs = sigint_disposition();
        drop(second);
        let after_the_last_run = sigint_disposition();

        let handler = mark_interrupted as *const () as libc::sighandler_t;
        assert_eq!(during_the_run, handler);
        assert_eq!(after_one_of_two_runs, handler);
        assert_eq!(after_the_last_run, libc::SIG_DFL);
        assert_eq!(active_runs_of_the_process(), 0);
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
