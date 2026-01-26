//! Worker pool for parallel mutation testing.
//!
//! Provides a configurable worker pool with work-stealing for efficient
//! parallel execution of mutation tests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;

use super::mutant::MutantStatus;
use super::Mutant;

/// Progress update for mutation testing.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Total number of mutants to process.
    pub total: usize,
    /// Number of mutants completed.
    pub completed: usize,
    /// Number of mutants killed.
    pub killed: usize,
    /// Number of mutants survived.
    pub survived: usize,
    /// Number of mutants that timed out.
    pub timeout: usize,
    /// Number of mutants with errors.
    pub error: usize,
    /// Current mutation score.
    pub score: f64,
}

impl ProgressUpdate {
    /// Create a new progress update.
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            killed: 0,
            survived: 0,
            timeout: 0,
            error: 0,
            score: 0.0,
        }
    }

    /// Update progress with a result.
    pub fn update(&mut self, status: MutantStatus) {
        self.completed += 1;
        match status {
            MutantStatus::Killed => self.killed += 1,
            MutantStatus::Survived => self.survived += 1,
            MutantStatus::Timeout => self.timeout += 1,
            MutantStatus::BuildError | MutantStatus::Equivalent => self.error += 1,
            MutantStatus::Pending | MutantStatus::Skipped => {}
        }
        let scored = self.killed + self.survived;
        if scored > 0 {
            self.score = self.killed as f64 / scored as f64;
        }
    }
}

/// Work item for the worker pool.
#[derive(Debug, Clone)]
pub struct WorkItem {
    /// The mutant to test.
    pub mutant: Mutant,
    /// Original source content.
    pub source: Arc<Vec<u8>>,
}

impl WorkItem {
    /// Create a new work item.
    pub fn new(mutant: Mutant, source: Arc<Vec<u8>>) -> Self {
        Self { mutant, source }
    }
}

/// Configuration for the worker pool.
#[derive(Debug, Clone)]
pub struct WorkerPoolConfig {
    /// Number of workers (0 = num_cpus).
    pub workers: usize,
    /// Whether work-stealing is enabled.
    pub work_stealing: bool,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            workers: 0,
            work_stealing: true,
        }
    }
}

impl WorkerPoolConfig {
    /// Create a new config with the given number of workers.
    pub fn with_workers(workers: usize) -> Self {
        Self {
            workers,
            ..Default::default()
        }
    }

    /// Get the effective number of workers.
    pub fn effective_workers(&self) -> usize {
        if self.workers == 0 {
            num_cpus()
        } else {
            self.workers
        }
    }

    /// Enable or disable work-stealing.
    pub fn work_stealing(mut self, enabled: bool) -> Self {
        self.work_stealing = enabled;
        self
    }
}

/// Get the number of CPUs available.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

/// File lock manager for ensuring only one mutant per file at a time.
#[derive(Debug, Default)]
pub struct FileLockManager {
    locks: RwLock<HashMap<PathBuf, Arc<Semaphore>>>,
}

impl FileLockManager {
    /// Create a new file lock manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a lock for the given file.
    pub fn get_lock(&self, path: &PathBuf) -> Arc<Semaphore> {
        // Fast path: read lock
        {
            let locks = self.locks.read();
            if let Some(lock) = locks.get(path) {
                return Arc::clone(lock);
            }
        }

        // Slow path: write lock to insert
        let mut locks = self.locks.write();
        locks
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .clone()
    }
}

/// Work-stealing queue for load balancing.
pub struct WorkQueue {
    /// Work items waiting to be processed.
    items: Mutex<Vec<WorkItem>>,
    /// Number of items remaining.
    remaining: AtomicUsize,
    /// Whether the queue is closed.
    closed: AtomicBool,
}

impl WorkQueue {
    /// Create a new work queue with the given items.
    pub fn new(items: Vec<WorkItem>) -> Self {
        let remaining = items.len();
        Self {
            items: Mutex::new(items),
            remaining: AtomicUsize::new(remaining),
            closed: AtomicBool::new(false),
        }
    }

    /// Steal a work item from the queue.
    pub fn steal(&self) -> Option<WorkItem> {
        if self.closed.load(Ordering::Acquire) {
            return None;
        }
        let mut items = self.items.lock();
        items.pop()
    }

    /// Get the number of remaining items.
    pub fn remaining(&self) -> usize {
        self.remaining.load(Ordering::Relaxed)
    }

    /// Mark an item as completed.
    pub fn complete(&self) {
        self.remaining.fetch_sub(1, Ordering::Release);
    }

    /// Close the queue (no more stealing).
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    /// Check if the queue is closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Check if all items have been completed.
    pub fn is_complete(&self) -> bool {
        self.remaining() == 0
    }
}

/// Handle for controlling the worker pool.
pub struct WorkerPoolHandle {
    /// Shutdown signal sender.
    shutdown_tx: mpsc::Sender<()>,
    /// Worker task handles.
    handles: Vec<JoinHandle<()>>,
}

impl WorkerPoolHandle {
    /// Create a new handle.
    pub fn new(shutdown_tx: mpsc::Sender<()>, handles: Vec<JoinHandle<()>>) -> Self {
        Self {
            shutdown_tx,
            handles,
        }
    }

    /// Signal graceful shutdown.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
        for handle in self.handles {
            let _ = handle.await;
        }
    }

    /// Check if all workers have finished.
    pub fn is_finished(&self) -> bool {
        self.handles.iter().all(|h| h.is_finished())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- ProgressUpdate tests ---

    #[test]
    fn test_progress_update_new() {
        let progress = ProgressUpdate::new(10);
        assert_eq!(progress.total, 10);
        assert_eq!(progress.completed, 0);
        assert_eq!(progress.killed, 0);
        assert_eq!(progress.survived, 0);
        assert_eq!(progress.score, 0.0);
    }

    #[test]
    fn test_progress_update_killed() {
        let mut progress = ProgressUpdate::new(10);
        progress.update(MutantStatus::Killed);

        assert_eq!(progress.completed, 1);
        assert_eq!(progress.killed, 1);
        assert_eq!(progress.score, 1.0);
    }

    #[test]
    fn test_progress_update_survived() {
        let mut progress = ProgressUpdate::new(10);
        progress.update(MutantStatus::Survived);

        assert_eq!(progress.completed, 1);
        assert_eq!(progress.survived, 1);
        assert_eq!(progress.score, 0.0);
    }

    #[test]
    fn test_progress_update_mixed() {
        let mut progress = ProgressUpdate::new(10);
        progress.update(MutantStatus::Killed);
        progress.update(MutantStatus::Killed);
        progress.update(MutantStatus::Survived);

        assert_eq!(progress.completed, 3);
        assert_eq!(progress.killed, 2);
        assert_eq!(progress.survived, 1);
        assert!((progress.score - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_progress_update_timeout() {
        let mut progress = ProgressUpdate::new(10);
        progress.update(MutantStatus::Timeout);

        assert_eq!(progress.completed, 1);
        assert_eq!(progress.timeout, 1);
        assert_eq!(progress.score, 0.0); // Timeout doesn't count for score
    }

    #[test]
    fn test_progress_update_error() {
        let mut progress = ProgressUpdate::new(10);
        progress.update(MutantStatus::BuildError);
        progress.update(MutantStatus::Equivalent);

        assert_eq!(progress.completed, 2);
        assert_eq!(progress.error, 2);
    }

    // --- WorkItem tests ---

    #[test]
    fn test_work_item_new() {
        let mutant = create_test_mutant("test.rs");
        let source = Arc::new(b"test content".to_vec());
        let item = WorkItem::new(mutant.clone(), Arc::clone(&source));

        assert_eq!(item.mutant.id, mutant.id);
        assert_eq!(item.source.as_slice(), b"test content");
    }

    // --- WorkerPoolConfig tests ---

    #[test]
    fn test_worker_pool_config_default() {
        let config = WorkerPoolConfig::default();
        assert_eq!(config.workers, 0);
        assert!(config.work_stealing);
    }

    #[test]
    fn test_worker_pool_config_with_workers() {
        let config = WorkerPoolConfig::with_workers(4);
        assert_eq!(config.workers, 4);
        assert!(config.work_stealing);
    }

    #[test]
    fn test_worker_pool_config_effective_workers() {
        let config = WorkerPoolConfig::with_workers(4);
        assert_eq!(config.effective_workers(), 4);

        let config = WorkerPoolConfig::default();
        assert!(config.effective_workers() >= 1);
    }

    #[test]
    fn test_worker_pool_config_work_stealing_disabled() {
        let config = WorkerPoolConfig::with_workers(2).work_stealing(false);
        assert!(!config.work_stealing);
    }

    // --- FileLockManager tests ---

    #[test]
    fn test_file_lock_manager_new() {
        let manager = FileLockManager::new();
        let path = PathBuf::from("test.rs");
        let lock = manager.get_lock(&path);
        assert!(lock.available_permits() > 0);
    }

    #[test]
    fn test_file_lock_manager_same_file_same_lock() {
        let manager = FileLockManager::new();
        let path = PathBuf::from("test.rs");

        let lock1 = manager.get_lock(&path);
        let lock2 = manager.get_lock(&path);

        // Both should reference the same semaphore
        assert!(Arc::ptr_eq(&lock1, &lock2));
    }

    #[test]
    fn test_file_lock_manager_different_files() {
        let manager = FileLockManager::new();
        let path1 = PathBuf::from("test1.rs");
        let path2 = PathBuf::from("test2.rs");

        let lock1 = manager.get_lock(&path1);
        let lock2 = manager.get_lock(&path2);

        // Different files should have different semaphores
        assert!(!Arc::ptr_eq(&lock1, &lock2));
    }

    // --- WorkQueue tests ---

    #[test]
    fn test_work_queue_new() {
        let items = vec![
            WorkItem::new(create_test_mutant("a.rs"), Arc::new(vec![])),
            WorkItem::new(create_test_mutant("b.rs"), Arc::new(vec![])),
        ];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.remaining(), 2);
        assert!(!queue.is_closed());
        assert!(!queue.is_complete());
    }

    #[test]
    fn test_work_queue_steal() {
        let items = vec![WorkItem::new(create_test_mutant("a.rs"), Arc::new(vec![]))];
        let queue = WorkQueue::new(items);

        let item = queue.steal();
        assert!(item.is_some());

        let item = queue.steal();
        assert!(item.is_none());
    }

    #[test]
    fn test_work_queue_complete() {
        let items = vec![WorkItem::new(create_test_mutant("a.rs"), Arc::new(vec![]))];
        let queue = WorkQueue::new(items);

        queue.steal();
        queue.complete();

        assert_eq!(queue.remaining(), 0);
        assert!(queue.is_complete());
    }

    #[test]
    fn test_work_queue_close() {
        let items = vec![WorkItem::new(create_test_mutant("a.rs"), Arc::new(vec![]))];
        let queue = WorkQueue::new(items);

        queue.close();
        assert!(queue.is_closed());

        // Stealing from closed queue returns None
        let item = queue.steal();
        assert!(item.is_none());
    }

    #[test]
    fn test_work_queue_empty() {
        let queue = WorkQueue::new(vec![]);
        assert_eq!(queue.remaining(), 0);
        assert!(queue.is_complete());
    }

    // Helper function to create test mutants
    fn create_test_mutant(file: &str) -> Mutant {
        Mutant::new(
            format!("mut-{}", file),
            file,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (0, 2),
        )
    }
}
