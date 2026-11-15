//! File safety mechanisms for mutation testing.
//!
//! Provides RAII-based guards to ensure source files are always restored
//! to their original state, even if a panic occurs during mutation testing.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;

use crate::core::{Error, Result};

type ActiveMutations = Arc<parking_lot::Mutex<HashMap<PathBuf, RestoreRecord>>>;

static ACTIVE_MUTATIONS: LazyLock<ActiveMutations> =
    LazyLock::new(|| Arc::new(parking_lot::Mutex::new(HashMap::new())));
static SIGNAL_HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The number of the last signal that the handler got. It is zero if there is none.
static RECEIVED_SIGNAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// True after a restore that failed. The process then exits with
/// `RESTORE_FAILURE_EXIT_CODE`.
static RESTORE_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The exit status of a run that could not restore a source file.
pub const RESTORE_FAILURE_EXIT_CODE: i32 = 74;

#[cfg(unix)]
const HANDLED_SIGNALS: [libc::c_int; 4] =
    [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

#[cfg(unix)]
extern "C" fn mark_interrupted(signal: libc::c_int) {
    RECEIVED_SIGNAL.store(signal, Ordering::SeqCst);
    INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn register_interrupt_handler() -> std::result::Result<(), String> {
    for signal in HANDLED_SIGNALS {
        let previous =
            unsafe { libc::signal(signal, mark_interrupted as *const () as libc::sighandler_t) };
        if previous == libc::SIG_ERR {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
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

/// All that a restore of one source file needs. Each restore path uses it: the
/// normal restore, the drop of the guard, and the signal watcher.
#[derive(Clone)]
struct RestoreRecord {
    path: PathBuf,
    original: Vec<u8>,
    /// The permissions of the file before the first write.
    permissions: fs::Permissions,
    /// The target of the symbolic link at `path` before the first write, if it was a link.
    link: Option<PathBuf>,
    /// The file that a link at `path` points to.
    real_path: PathBuf,
    backup: Option<PathBuf>,
}

/// Writes the original bytes into the staged file of a restore. A test replaces
/// it to inject a fault or a pause.
type StageWriter = fn(&mut fs::File, &[u8]) -> std::io::Result<()>;

fn write_all_bytes(file: &mut fs::File, content: &[u8]) -> std::io::Result<()> {
    file.write_all(content)
}

impl RestoreRecord {
    fn restore(&self) -> std::io::Result<()> {
        self.restore_with(write_all_bytes)
    }

    /// Replace the file with a new file that has the original bytes and the
    /// recorded permissions, then read it again to check bytes and permissions.
    ///
    /// The new file is complete before the rename, and the rename replaces the
    /// path itself and never follows a link. Thus a failed restore never leaves
    /// an empty or partial source, and a link that a test command put at the
    /// path does not redirect the write to another file.
    fn restore_with(&self, stage: StageWriter) -> std::io::Result<()> {
        let target = match &self.link {
            Some(link) => {
                ensure_link(&self.path, link)?;
                self.real_path.clone()
            }
            None => self.path.clone(),
        };
        let directory = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        stage(staged.as_file_mut(), &self.original)?;
        staged.as_file().set_permissions(self.permissions.clone())?;
        staged.as_file().sync_all()?;
        staged.persist(&target).map_err(|error| error.error)?;
        #[cfg(unix)]
        fs::File::open(directory)?.sync_all()?;

        if !fs::symlink_metadata(&target)?.is_file() {
            return Err(std::io::Error::other(
                "the restored path is not a regular file",
            ));
        }
        if fs::read(&target)? != self.original {
            return Err(std::io::Error::other(
                "the restored file differs from the original",
            ));
        }
        if fs::metadata(&target)?.permissions() != self.permissions {
            return Err(std::io::Error::other(
                "the restored permissions differ from the original",
            ));
        }
        Ok(())
    }
}

/// Make `path` a symbolic link to `link` again if a test command changed it.
#[cfg(unix)]
fn ensure_link(path: &Path, link: &Path) -> std::io::Result<()> {
    if fs::read_link(path).is_ok_and(|current| current == link) {
        return Ok(());
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for attempt in 0..100u32 {
        let candidate = directory.join(format!(".omen-link-{}-{attempt}", std::process::id()));
        match std::os::unix::fs::symlink(link, &candidate) {
            Ok(()) => return fs::rename(&candidate, path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other("no free name for a temporary link"))
}

#[cfg(not(unix))]
fn ensure_link(_: &Path, _: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The path of a backup file that exists and has the original bytes. If the
/// backup of the record is gone, this function writes a new private file from
/// the bytes in memory.
fn ensure_backup(record: &RestoreRecord) -> Option<PathBuf> {
    if let Some(backup) = &record.backup {
        if fs::read(backup).is_ok_and(|content| content == record.original) {
            return Some(backup.clone());
        }
    }
    let mut file = tempfile::Builder::new()
        .prefix("omen-mutation-backup-")
        .tempfile_in(std::env::temp_dir())
        .ok()?;
    file.write_all(&record.original).ok()?;
    file.as_file().sync_all().ok()?;
    file.keep().ok().map(|(_, path)| path)
}

fn shell_word(path: &Path) -> String {
    let text = path.to_string_lossy();
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./-+@:=".contains(c));
    if plain && !text.is_empty() {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn recovery_text(path: &Path, backup: &Path, in_git: bool) -> String {
    let mut text = format!(
        "omen could not restore {}.\nThe original content is in {}.\nTo restore the file, run: cp {} {}\n",
        path.display(),
        backup.display(),
        shell_word(backup),
        shell_word(path)
    );
    if in_git {
        text.push_str(&format!(
            "If the file had no change before the run, you can also run: git checkout -- {}\n",
            shell_word(path)
        ));
    }
    text
}

fn is_in_git_repository(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .any(|dir| dir.join(".git").exists())
}

/// Report a restore that failed: keep a backup that exists, print the path of
/// the backup and the commands to recover, and mark the process as failed.
fn report_failed_restore(record: &RestoreRecord, error: &std::io::Error) {
    RESTORE_FAILED.store(true, Ordering::SeqCst);
    match ensure_backup(record) {
        Some(backup) => eprintln!(
            "omen: restore of {} failed: {error}\n{}",
            record.path.display(),
            recovery_text(&record.path, &backup, is_in_git_repository(&record.path))
        ),
        None => eprintln!(
            "omen: restore of {} failed: {error}\nomen could not write a backup file, so the original content is lost. If the file had no change before the run, run: git checkout -- {}",
            record.path.display(),
            shell_word(&record.path)
        ),
    }
}

#[cfg(not(test))]
fn exit_after_failed_restore() {
    std::process::exit(RESTORE_FAILURE_EXIT_CODE);
}

#[cfg(test)]
static EXIT_REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn exit_after_failed_restore() {
    EXIT_REQUESTS.fetch_add(1, Ordering::SeqCst);
}

/// Exit with `RESTORE_FAILURE_EXIT_CODE` if a restore failed. A run calls this
/// after it restored each file, never inside a guard.
fn exit_if_restore_failed() {
    if RESTORE_FAILED.load(Ordering::SeqCst) {
        exit_after_failed_restore();
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
    /// The permissions of the file before the first write.
    permissions: fs::Permissions,
    /// Whether the file has been modified.
    modified: bool,
    /// A copy of the original content in the temporary directory.
    backup: Option<tempfile::NamedTempFile>,
    active_mutations: ActiveMutations,
    interrupted: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Writes the mutated content. Tests replace it to inject a write fault.
    writer: fn(&Path, &[u8]) -> Result<()>,
    /// Writes the original content in a restore of the guard. Tests replace it.
    restore_writer: StageWriter,
    link: Option<PathBuf>,
    real_path: PathBuf,
}

impl MutationGuard {
    /// Create a new mutation guard for the given file.
    ///
    /// Reads and stores the original file content and permissions for later restoration.
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
        let permissions = fs::metadata(&path).map_err(Error::Io)?.permissions();
        let link = fs::symlink_metadata(&path)
            .map_err(Error::Io)?
            .is_symlink()
            .then(|| fs::read_link(&path))
            .transpose()
            .map_err(Error::Io)?;
        let real_path = fs::canonicalize(&path).map_err(Error::Io)?;

        Ok(Self {
            link,
            real_path,
            path,
            original,
            permissions,
            modified: false,
            backup: None,
            active_mutations,
            interrupted,
            writer: |path, content| write_in_place(path, content),
            restore_writer: write_all_bytes,
        })
    }

    fn is_interrupted(&self) -> bool {
        self.interrupted
            .as_deref()
            .unwrap_or(&INTERRUPTED)
            .load(Ordering::SeqCst)
    }

    fn record(&self) -> RestoreRecord {
        RestoreRecord {
            path: self.path.clone(),
            original: self.original.clone(),
            permissions: self.permissions.clone(),
            link: self.link.clone(),
            real_path: self.real_path.clone(),
            backup: self
                .backup
                .as_ref()
                .map(|backup| backup.path().to_path_buf()),
        }
    }

    /// Apply a mutation to the file.
    ///
    /// This writes the mutated content into the existing file, so the inode, the
    /// owner, the permissions, and a symlink stay as they are. Before the first
    /// write, it saves the original content in a backup file.
    ///
    /// The mutant write is in place for that reason (#549): a replacement of the file
    /// would change the inode and the owner of a source file of the user. A restore does
    /// not need this, and it replaces the file, so a failed restore never leaves a
    /// partial source.
    pub fn apply(&mut self, content: &[u8]) -> Result<()> {
        let active_mutations = Arc::clone(&self.active_mutations);
        let mut active = active_mutations.lock();
        if self.is_interrupted() {
            return Ok(());
        }
        if self.backup.is_none() {
            self.backup = Some(save_backup(&self.original)?);
        }
        let record = self.record();
        active.insert(self.path.clone(), record.clone());
        // A write that fails can truncate the file first, so the file counts as
        // modified from the first write.
        self.modified = true;
        if let Err(error) = (self.writer)(&self.path, content) {
            match record.restore() {
                Ok(()) => {
                    self.modified = false;
                    active.remove(&self.path);
                }
                Err(restore_error) => self.give_up_restore(&record, &restore_error),
            }
            return Err(error);
        }
        Ok(())
    }

    /// Report a restore that failed, and keep the backup file on the disk.
    fn give_up_restore(&mut self, record: &RestoreRecord, error: &std::io::Error) {
        report_failed_restore(record, error);
        if let Some(backup) = self.backup.take() {
            let _ = backup.keep();
        }
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
        if !self.modified {
            return Ok(());
        }
        // The lock of the map covers each write to a source file, so the
        // signal restore never runs in the middle of this write. The signal
        // restore removes the entry after a verified restore. The guard must
        // then not write again: the process exits soon, and a second truncate
        // can leave an empty file.
        let active_mutations = Arc::clone(&self.active_mutations);
        let mut active = active_mutations.lock();
        if active.contains_key(&self.path) {
            self.record()
                .restore_with(self.restore_writer)
                .map_err(Error::Io)?;
        }
        self.modified = false;
        active.remove(&self.path);
        Ok(())
    }
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        let active_mutations = Arc::clone(&self.active_mutations);
        let mut active = active_mutations.lock();
        if self.modified && active.contains_key(&self.path) {
            let record = self.record();
            if let Err(error) = record.restore_with(self.restore_writer) {
                self.give_up_restore(&record, &error);
                active.remove(&self.path);
                return;
            }
        }
        active.remove(&self.path);
    }
}

fn restore_registered_mutations(
    active_mutations: &parking_lot::Mutex<HashMap<PathBuf, RestoreRecord>>,
    interrupted: &std::sync::atomic::AtomicBool,
) {
    interrupted.store(true, Ordering::SeqCst);
    let mut active = active_mutations.lock();
    let paths: Vec<PathBuf> = active.keys().cloned().collect();
    for path in paths {
        let Some(record) = active.get(&path) else {
            continue;
        };
        match record.restore() {
            Ok(()) => {
                if let Some(backup) = &record.backup {
                    let _ = fs::remove_file(backup);
                }
                active.remove(&path);
            }
            Err(error) => report_failed_restore(record, &error),
        }
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

/// Tell if the group has a process.
///
/// A process that ended, and that its parent did not reap, is a process of
/// the group. Thus a group exists until its last process is reaped. The rule
/// is the same on Linux and on macOS, but the two systems report it in
/// different ways:
///
/// - Linux gives success for signal 0 when the group has only such
///   processes.
/// - macOS does not count such processes as targets of a signal. It gives
///   `EPERM` when the group has only such processes, and `ESRCH` when the
///   group does not exist.
///
/// `EPERM` also means that the group has a process to which the caller
/// cannot send a signal. Such a group exists. Only `ESRCH` means that the
/// group has no process.
#[cfg(unix)]
fn group_exists(group: u32) -> bool {
    // SAFETY: signal 0 does only the existence check.
    let result = unsafe { libc::killpg(group as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
        std::process::exit(exit_code_after_interrupt(code));
    }
}

/// The exit status after an interrupt: the failure status if a restore failed,
/// else 128 and the number of the signal, as a shell reports it.
fn exit_code_after_interrupt(default: i32) -> i32 {
    if RESTORE_FAILED.load(Ordering::SeqCst) {
        return RESTORE_FAILURE_EXIT_CODE;
    }
    match RECEIVED_SIGNAL.load(Ordering::SeqCst) {
        0 => default,
        signal => 128 + signal,
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
        exit_if_restore_failed();
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
        for signal in HANDLED_SIGNALS {
            libc::signal(signal, libc::SIG_DFL);
        }
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

/// The longest time to wait for a stopped test command, on Windows.
#[cfg(windows)]
const PROCESS_EXIT_LIMIT_MILLISECONDS: u32 = 5000;

/// Stop a process and each of its descendants.
///
/// `taskkill` returns after it sent the stop request to each process of the
/// tree. It does not wait for the processes to end. Thus this function then
/// waits for the test command itself, for a maximum of
/// `PROCESS_EXIT_LIMIT_MILLISECONDS`. It does not wait for the descendants:
/// a descendant can run for a short time after this function returns.
#[cfg(windows)]
fn kill_process_tree(id: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &id.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    wait_for_process_exit(id);
}

/// Wait until the process with this identifier ends, for a limited time.
///
/// The executor holds the child handle of each registered test command,
/// thus the operating system does not give the identifier to a different
/// process during the wait.
#[cfg(windows)]
fn wait_for_process_exit(id: u32) {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit_handle: i32, id: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    // The access right to wait for a process.
    const SYNCHRONIZE: u32 = 0x0010_0000;

    // SAFETY: the handle is used only for the wait and is closed here. A
    // null handle means that the process does not exist now.
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, id);
        if !handle.is_null() {
            WaitForSingleObject(handle, PROCESS_EXIT_LIMIT_MILLISECONDS);
            CloseHandle(handle);
        }
    }
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

#[cfg(all(test, unix))]
pub(crate) fn active_runs_of_the_process() -> usize {
    INTERRUPT_GATE.active_runs()
}

/// Write content into an existing file.
///
/// The write opens the path, follows a symlink, and truncates the file. It does
/// not replace the file, so the inode, the owner, the ACLs, the attributes, and
/// the symlink stay as they are.
pub fn write_in_place(path: impl AsRef<Path>, content: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path.as_ref())
        .map_err(Error::Io)?;
    file.write_all(content).map_err(Error::Io)?;
    file.flush().map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)
}

/// Save the original content in a new private file in the temporary directory.
///
/// The file is not in the repository, so a test command does not see it.
fn save_backup(original: &[u8]) -> Result<tempfile::NamedTempFile> {
    let mut backup = tempfile::Builder::new()
        .prefix("omen-mutation-backup-")
        .tempfile_in(std::env::temp_dir())
        .map_err(Error::Io)?;
    backup.write_all(original).map_err(Error::Io)?;
    backup.as_file().sync_all().map_err(Error::Io)?;
    Ok(backup)
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

    /// Starts a `sleep` that is the only process of a new group, and registers
    /// the group in `registry`.
    #[cfg(unix)]
    fn spawn_leader(registry: &ProcessGroups) -> (std::process::Child, ProcessGroup) {
        let not_interrupted = std::sync::atomic::AtomicBool::new(false);
        let (child, group) = ProcessGroup::spawn_with(
            registry,
            &not_interrupted,
            || {
                let mut command = std::process::Command::new("sleep");
                command.arg("30");
                std::os::unix::process::CommandExt::process_group(&mut command, 0);
                command.spawn()
            },
            |child| Some(child.id()),
        )
        .unwrap();
        (child, group.unwrap())
    }

    /// Starts a registered group with two processes: the leader and one more
    /// member.
    ///
    /// Both processes are children of the test, not a parent and its
    /// descendant. Thus the test reaps each process, and no orphan stays in
    /// the group. An orphan is reaped by the init process, and in a container
    /// that init process possibly does not reap.
    #[cfg(unix)]
    fn spawn_group_of_two(
        registry: &ProcessGroups,
    ) -> (std::process::Child, std::process::Child, ProcessGroup) {
        let (leader, group) = spawn_leader(registry);
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        std::os::unix::process::CommandExt::process_group(&mut command, leader.id() as i32);
        let member = command.spawn().unwrap();
        (leader, member, group)
    }

    /// Reaps each child in a different thread, and gives the exit statuses.
    #[cfg(unix)]
    fn reap_in_background(
        children: Vec<std::process::Child>,
    ) -> std::thread::JoinHandle<Vec<std::process::ExitStatus>> {
        std::thread::spawn(move || {
            children
                .into_iter()
                .map(|mut child| child.wait().unwrap())
                .collect()
        })
    }

    #[cfg(unix)]
    fn successes(statuses: &[std::process::ExitStatus]) -> usize {
        statuses.iter().filter(|status| status.success()).count()
    }

    /// Waits until the process ended, and does not reap it. The process is
    /// then a zombie until its parent calls `wait`.
    #[cfg(unix)]
    fn wait_for_end_without_reap(child: &std::process::Child) {
        // SAFETY: `waitid` writes only to `info`. `WNOWAIT` keeps the process
        // in the state in which a later `wait` can reap it.
        let result = unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
    }

    #[cfg(unix)]
    #[test]
    fn test_group_with_a_leader_that_ended_and_is_not_reaped_exists() {
        let registry = local_registry();
        let (mut leader, group) = spawn_leader(&registry);

        group.kill();
        wait_for_end_without_reap(&leader);
        let exists_before_the_reap = group.exists();
        leader.wait().unwrap();

        assert!(exists_before_the_reap);
        assert!(!group.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_process_group_stays_registered_until_it_exits() {
        let registry = local_registry();
        let (mut leader, mut member, mut group) = spawn_group_of_two(&registry);
        let id = leader.id();
        assert!(registry.lock().contains(&id));

        // The leader ended and is not reaped, so the group has a process. This
        // is the interval in which an interrupt must see the group.
        group.kill();
        wait_for_end_without_reap(&leader);
        assert!(group.exists());
        assert!(registry.lock().contains(&id), "the kill removed the group");

        assert!(!leader.wait().unwrap().success());
        assert!(!member.wait().unwrap().success());
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
        let (leader, member, group) = spawn_group_of_two(&registry);
        let id = leader.id();
        let reaper = reap_in_background(vec![leader, member]);

        terminate_registered_process_groups(&registry);

        // The function returned, so each process of the group stopped before
        // the file restore that follows it.
        assert!(!group.exists());
        assert!(registry.lock().contains(&id));
        assert_eq!(successes(&reaper.join().unwrap()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_process_group_drop_kills_waits_and_removes_after_a_panic() {
        let registry = local_registry();
        let (leader, member, group) = spawn_group_of_two(&registry);
        let id = leader.id();
        let reaper = reap_in_background(vec![leader, member]);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _group = group;
            panic!("the owner of the group panics");
        }));

        assert!(result.is_err());
        assert!(!group_exists(id));
        assert!(!registry.lock().contains(&id));
        assert_eq!(successes(&reaper.join().unwrap()), 0);
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
                let (mut child, group) = ProcessGroup::spawn_with(
                    &registry,
                    &not_interrupted,
                    || {
                        // This closure runs below the registry lock. Thus the
                        // interrupt path, which starts after this message,
                        // cannot read the registry before the registration.
                        sender.send(()).unwrap();
                        let mut command = std::process::Command::new("sleep");
                        command.arg("30");
                        std::os::unix::process::CommandExt::process_group(&mut command, 0);
                        command.spawn()
                    },
                    |child| Some(child.id()),
                )
                .unwrap();
                // The interrupt path waits until the group has no process, so
                // this thread must reap the leader during that wait.
                (child.wait().unwrap(), group.unwrap())
            })
        };

        receiver.recv().unwrap();
        terminate_registered_process_groups(&registry);
        let id = *registry.lock().iter().next().unwrap();
        let exists_after_the_termination = group_exists(id);

        let (status, group) = spawner.join().unwrap();
        assert!(!status.success());
        assert!(!exists_after_the_termination);
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
    fn test_write_in_place_does_not_create_a_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        assert!(write_in_place(&file_path, b"test content").is_err());
        assert!(!file_path.exists());
    }

    #[test]
    fn test_write_in_place_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"old content").unwrap();

        write_in_place(&file_path, b"new content").unwrap();

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"new content");
    }

    #[test]
    fn test_guard_leaves_no_backup_file_after_a_restore() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"content").unwrap();
        drop(guard);

        // Check no temp files left behind
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "test.rs");
    }

    fn write_then_fail(path: &Path, content: &[u8]) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(Error::Io)?;
        file.write_all(&content[..content.len() / 2])
            .map_err(Error::Io)?;
        Err(Error::Io(std::io::Error::other("disk full")))
    }

    #[test]
    fn test_failed_partial_write_restores_the_source_at_once() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.writer = write_then_fail;
        assert!(guard.apply(b"mutated content!").is_err());

        assert_eq!(fs::read(&file_path).unwrap(), b"original content");
        drop(guard);
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");
    }

    #[test]
    fn test_backup_is_not_in_the_source_directory_during_a_mutation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"content").unwrap();

        let names: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("test.rs")]);
    }

    #[cfg(unix)]
    #[test]
    fn test_backup_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let backup = save_backup(b"original").unwrap();
        let mode = backup.as_file().metadata().unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
        assert!(backup.path().starts_with(std::env::temp_dir()));
    }

    static PAUSE_TESTS: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    static PAUSE_ENTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static PAUSE_RELEASED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    /// A write that waits before it truncates the file. A signal restore that
    /// runs in the wait can finish before the truncate, and the process can
    /// then exit with an empty file.
    fn write_after_a_pause(file: &mut fs::File, content: &[u8]) -> std::io::Result<()> {
        PAUSE_ENTERED.store(true, Ordering::SeqCst);
        while !PAUSE_RELEASED.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        file.write_all(content)
    }

    #[test]
    fn test_the_signal_restore_waits_for_a_restore_of_the_guard() {
        let _serial = PAUSE_TESTS.lock();
        PAUSE_ENTERED.store(false, Ordering::SeqCst);
        PAUSE_RELEASED.store(false, Ordering::SeqCst);
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
        guard.restore_writer = write_after_a_pause;
        let dropper = std::thread::spawn(move || drop(guard));
        while !PAUSE_ENTERED.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }

        let watcher_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = {
            let watcher_done = Arc::clone(&watcher_done);
            std::thread::spawn(move || {
                restore_registered_mutations(&active_mutations, &interrupted);
                watcher_done.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        let finished_early = watcher_done.load(Ordering::SeqCst);
        PAUSE_RELEASED.store(true, Ordering::SeqCst);
        dropper.join().unwrap();
        watcher.join().unwrap();

        assert!(
            !finished_early,
            "the signal restore ran while the guard restored the file"
        );
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    fn truncate_only(_: &mut fs::File, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    fn stage_half_then_fail(file: &mut fs::File, content: &[u8]) -> std::io::Result<()> {
        file.write_all(&content[..content.len() / 2])?;
        Err(std::io::Error::other("disk full"))
    }

    #[test]
    fn test_a_failed_restore_does_not_stop_the_restore_of_other_guards() {
        let _serial = PAUSE_TESTS.lock();
        EXIT_REQUESTS.store(0, Ordering::SeqCst);
        let temp_dir = TempDir::new().unwrap();
        let first = temp_dir.path().join("first.rs");
        let second = temp_dir.path().join("second.rs");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let mut first_guard = MutationGuard::new(&first).unwrap();
        let mut second_guard = MutationGuard::new(&second).unwrap();
        first_guard.apply(b"mutated").unwrap();
        second_guard.apply(b"mutated").unwrap();
        fs::remove_file(&first).unwrap();
        fs::create_dir(&first).unwrap();

        drop(first_guard);

        assert_eq!(EXIT_REQUESTS.load(Ordering::SeqCst), 0);
        drop(second_guard);
        assert_eq!(fs::read(&second).unwrap(), b"second");
        RESTORE_FAILED.store(false, Ordering::SeqCst);
        for entry in fs::read_dir(std::env::temp_dir()).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("omen-mutation-backup-")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_restore_replaces_a_symlink_and_does_not_write_the_victim() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let victim = temp_dir.path().join("victim.txt");
        fs::write(&file_path, b"original").unwrap();
        fs::write(&victim, b"victim").unwrap();
        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        fs::remove_file(&file_path).unwrap();
        std::os::unix::fs::symlink(&victim, &file_path).unwrap();

        drop(guard);

        assert_eq!(fs::read(&victim).unwrap(), b"victim");
        assert!(fs::symlink_metadata(&file_path).unwrap().is_file());
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[test]
    fn test_a_failed_restore_write_never_leaves_a_partial_source() {
        let _serial = PAUSE_TESTS.lock();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();
        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        guard.restore_writer = stage_half_then_fail;

        assert!(guard.restore().is_err());

        let content = fs::read(&file_path).unwrap();
        assert!(
            content == b"mutated" || content == b"original content",
            "{content:?}"
        );
        guard.restore_writer = write_all_bytes;
        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");
    }

    #[test]
    fn test_the_run_exits_after_a_failed_restore_when_it_concludes() {
        let _serial = PAUSE_TESTS.lock();
        EXIT_REQUESTS.store(0, Ordering::SeqCst);
        exit_if_restore_failed();
        assert_eq!(EXIT_REQUESTS.load(Ordering::SeqCst), 0);
        RESTORE_FAILED.store(true, Ordering::SeqCst);
        exit_if_restore_failed();
        RESTORE_FAILED.store(false, Ordering::SeqCst);
        assert_eq!(EXIT_REQUESTS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_a_guard_does_not_write_again_after_the_signal_restore() {
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
        guard.restore_writer = truncate_only;

        drop(guard);

        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_restore_succeeds_when_the_test_command_made_the_source_read_only() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        set_mode(&file_path, 0o640);

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        set_mode(&file_path, 0o444);
        drop(guard);

        assert_eq!(fs::read(&file_path).unwrap(), b"original");
        assert_eq!(mode_of(&file_path), 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn test_restore_succeeds_when_the_backup_and_the_write_permission_are_gone() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        fs::remove_file(guard.backup.as_ref().unwrap().path()).unwrap();
        set_mode(&file_path, 0o444);
        drop(guard);

        assert_eq!(fs::read(&file_path).unwrap(), b"original");
        assert_eq!(mode_of(&file_path), 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn test_a_failed_restore_reports_a_backup_that_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        let record = guard.record();
        fs::remove_file(record.backup.as_ref().unwrap()).unwrap();

        let reported = ensure_backup(&record).unwrap();

        assert!(reported.exists());
        assert_eq!(fs::read(&reported).unwrap(), b"original");
        assert_eq!(mode_of(&reported) & 0o077, 0);
        fs::remove_file(reported).unwrap();
    }

    #[test]
    fn test_the_recovery_text_names_the_backup_and_the_commands() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let backup = temp_dir.path().join("backup");
        let text = recovery_text(&file_path, &backup, true);
        assert!(text.contains(&format!("cp {} {}", backup.display(), file_path.display())));
        assert!(text.contains(&format!("git checkout -- {}", file_path.display())));
        assert!(!recovery_text(&file_path, &backup, false).contains("git checkout"));
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

    /// Put a link at each temporary name that the next writes can use. A write
    /// that follows a link changes the victim file.
    #[cfg(unix)]
    fn plant_links(directory: &Path, victim: &Path) {
        for counter in 0..500 {
            for prefix in [".omen-mutation-", ".omen-restore-"] {
                let name = format!("{prefix}{}-{counter}.tmp", std::process::id());
                std::os::unix::fs::symlink(victim, directory.join(name)).unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_write_in_place_does_not_follow_a_link_at_a_guessed_name() {
        let temp_dir = TempDir::new().unwrap();
        let victim = temp_dir.path().join("victim.txt");
        fs::write(&victim, b"victim").unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        plant_links(temp_dir.path(), &victim);

        write_in_place(&file_path, b"mutated").unwrap();

        assert_eq!(fs::read(&victim).unwrap(), b"victim");
        assert_eq!(fs::read(&file_path).unwrap(), b"mutated");
    }

    #[cfg(unix)]
    #[test]
    fn test_drop_restore_does_not_follow_a_link_at_a_guessed_name() {
        let temp_dir = TempDir::new().unwrap();
        let victim = temp_dir.path().join("victim.txt");
        fs::write(&victim, b"victim").unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        plant_links(temp_dir.path(), &victim);

        drop(guard);

        assert_eq!(fs::read(&victim).unwrap(), b"victim");
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn test_a_symlinked_source_stays_a_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let real = temp_dir.path().join("real.rs");
        let link = temp_dir.path().join("link.rs");
        fs::write(&real, b"original").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let mut guard = MutationGuard::new(&link).unwrap();
        guard.apply(b"mutated").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"mutated");
        drop(guard);

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn test_mutation_keeps_the_inode_of_the_file() {
        use std::os::unix::fs::MetadataExt;
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        let inode = fs::metadata(&file_path).unwrap().ino();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        assert_eq!(fs::metadata(&file_path).unwrap().ino(), inode);
        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn test_mutation_and_restore_keep_the_mode_of_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original").unwrap();
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o750)).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated").unwrap();
        assert_eq!(mode(&file_path), 0o750);
        drop(guard);

        assert_eq!(fs::read(&file_path).unwrap(), b"original");
        assert_eq!(mode(&file_path), 0o750);
    }
}
