//! Test executor for mutation testing.
//!
//! Runs test commands against mutated code and determines if mutants
//! are killed or survived. Supports both synchronous and async execution
//! with parallel worker pools.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::Command as AsyncCommand;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::core::Result;

use super::mutant::{MutantStatus, MutationResult};
use super::safety::MutationGuard;
use super::worker::{FileLockManager, ProgressUpdate, WorkItem, WorkQueue};
use super::Mutant;

/// Progress callback type for async execution.
pub type ProgressCallback = Box<dyn Fn(ProgressUpdate) + Send + Sync>;

/// Configuration for test execution.
#[derive(Clone)]
pub struct ExecutorConfig {
    /// Test command to run.
    pub test_command: String,
    /// Timeout in seconds for each test run.
    pub timeout_secs: u64,
    /// Working directory for test execution.
    pub working_dir: Option<std::path::PathBuf>,
    /// Whether to capture test output.
    pub capture_output: bool,
    /// Number of parallel workers (0 = num_cpus).
    pub jobs: usize,
    /// Progress callback (optional).
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl std::fmt::Debug for ExecutorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutorConfig")
            .field("test_command", &self.test_command)
            .field("timeout_secs", &self.timeout_secs)
            .field("working_dir", &self.working_dir)
            .field("capture_output", &self.capture_output)
            .field("jobs", &self.jobs)
            .field("progress_callback", &self.progress_callback.is_some())
            .finish()
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            test_command: "cargo test".to_string(),
            timeout_secs: 30,
            working_dir: None,
            capture_output: false,
            jobs: 0,
            progress_callback: None,
        }
    }
}

impl ExecutorConfig {
    /// Create a new executor config with the given test command.
    pub fn with_command(command: impl Into<String>) -> Self {
        Self {
            test_command: command.into(),
            ..Default::default()
        }
    }

    /// Set the timeout.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Enable output capture.
    pub fn capture_output(mut self, capture: bool) -> Self {
        self.capture_output = capture;
        self
    }

    /// Set the number of parallel jobs.
    pub fn jobs(mut self, jobs: usize) -> Self {
        self.jobs = jobs;
        self
    }

    /// Set the progress callback.
    pub fn progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(ProgressUpdate) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(Box::new(callback)));
        self
    }

    /// Get the effective number of jobs.
    pub fn effective_jobs(&self) -> usize {
        if self.jobs == 0 {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        } else {
            self.jobs
        }
    }
}

/// Executor for running tests against mutants.
pub struct MutantExecutor {
    config: ExecutorConfig,
}

impl MutantExecutor {
    /// Create a new executor with the given configuration.
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Execute tests against a single mutant.
    ///
    /// This applies the mutation, runs tests, and restores the original file.
    pub fn execute_mutant(&self, mutant: &Mutant, source: &[u8]) -> Result<MutationResult> {
        let start = Instant::now();

        // Create guard to ensure file is restored
        let mut guard = MutationGuard::new(&mutant.file_path)?;

        // Apply the mutation
        let mutated_content = mutant.apply(source);
        guard.apply(&mutated_content)?;

        // Run tests
        let status = self.run_tests();
        let duration_ms = start.elapsed().as_millis() as u64;

        // Guard will restore the file when dropped
        drop(guard);

        let result = MutationResult::new(mutant.clone(), status, duration_ms);

        if self.config.capture_output {
            // Output is captured in run_tests if needed
        }

        Ok(result)
    }

    /// Execute tests and return the mutant status.
    fn run_tests(&self) -> MutantStatus {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", &self.config.test_command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &self.config.test_command]);
            c
        };

        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let timeout = Duration::from_secs(self.config.timeout_secs);

        match execute_with_timeout(&mut cmd, timeout) {
            ExecutionResult::Success => MutantStatus::Survived,
            ExecutionResult::Failed => MutantStatus::Killed,
            ExecutionResult::Timeout => MutantStatus::Timeout,
            ExecutionResult::Error => MutantStatus::BuildError,
        }
    }

    /// Get the executor configuration.
    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }
}

/// Result of executing a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionResult {
    /// Command succeeded (exit code 0).
    Success,
    /// Command failed (non-zero exit code).
    Failed,
    /// Command timed out.
    Timeout,
    /// Error executing command.
    Error,
}

/// Async executor for parallel mutation testing.
///
/// This executor runs mutants in parallel using a configurable worker pool,
/// with file-level locking to ensure only one mutant per file at a time.
pub struct AsyncMutantExecutor {
    config: ExecutorConfig,
    /// Semaphore to limit concurrency.
    semaphore: Arc<Semaphore>,
    /// File lock manager for per-file synchronization.
    file_locks: Arc<FileLockManager>,
    /// Shutdown flag for graceful termination.
    shutdown: Arc<AtomicBool>,
}

impl AsyncMutantExecutor {
    /// Create a new async executor with the given configuration.
    pub fn new(config: ExecutorConfig) -> Self {
        let permits = config.effective_jobs();
        Self {
            config,
            semaphore: Arc::new(Semaphore::new(permits)),
            file_locks: Arc::new(FileLockManager::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Execute tests against multiple mutants in parallel.
    ///
    /// Mutants for the same file are serialized to prevent conflicts.
    pub async fn execute_mutants(
        &self,
        mutants: &[Mutant],
        sources: &HashMap<PathBuf, Vec<u8>>,
    ) -> Result<Vec<MutationResult>> {
        if mutants.is_empty() {
            return Ok(vec![]);
        }

        let total = mutants.len();
        let progress = Arc::new(parking_lot::Mutex::new(ProgressUpdate::new(total)));
        let results = Arc::new(parking_lot::Mutex::new(Vec::with_capacity(total)));

        // Create work items
        let work_items: Vec<WorkItem> = mutants
            .iter()
            .filter_map(|mutant| {
                sources
                    .get(&mutant.file_path)
                    .map(|source| WorkItem::new(mutant.clone(), Arc::new(source.clone())))
            })
            .collect();

        let queue = Arc::new(WorkQueue::new(work_items));

        // Spawn worker tasks
        let mut handles = Vec::new();
        let num_workers = self.config.effective_jobs();

        for _ in 0..num_workers {
            let semaphore = Arc::clone(&self.semaphore);
            let file_locks = Arc::clone(&self.file_locks);
            let shutdown = Arc::clone(&self.shutdown);
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let progress = Arc::clone(&progress);
            let config = self.config.clone();

            let handle = tokio::spawn(async move {
                while !shutdown.load(Ordering::Relaxed) {
                    // Try to steal work
                    let item = match queue.steal() {
                        Some(item) => item,
                        None => break,
                    };

                    // Acquire global concurrency permit
                    let _permit = semaphore.acquire().await.ok();

                    // Acquire file-specific lock
                    let file_lock = file_locks.get_lock(&item.mutant.file_path);
                    let _file_permit = file_lock.acquire().await.ok();

                    // Execute the mutant
                    let result = execute_mutant_async(&item.mutant, &item.source, &config).await;

                    // Update progress
                    {
                        let mut prog = progress.lock();
                        prog.update(result.status);
                        if let Some(ref callback) = config.progress_callback {
                            callback(prog.clone());
                        }
                    }

                    // Store result
                    results.lock().push(result);

                    // Mark work as complete
                    queue.complete();
                }
            });

            handles.push(handle);
        }

        // Wait for all workers to complete
        for handle in handles {
            let _ = handle.await;
        }

        Ok(Arc::try_unwrap(results).unwrap().into_inner())
    }

    /// Execute a single mutant asynchronously.
    pub async fn execute_mutant(&self, mutant: &Mutant, source: &[u8]) -> Result<MutationResult> {
        // Acquire file lock
        let file_lock = self.file_locks.get_lock(&mutant.file_path);
        let _file_permit = file_lock
            .acquire()
            .await
            .map_err(|_| crate::core::Error::analysis("Failed to acquire file lock"))?;

        Ok(execute_mutant_async(mutant, source, &self.config).await)
    }

    /// Signal graceful shutdown.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    /// Get the executor configuration.
    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }
}

/// Execute a single mutant asynchronously.
async fn execute_mutant_async(
    mutant: &Mutant,
    source: &[u8],
    config: &ExecutorConfig,
) -> MutationResult {
    let start = Instant::now();

    // Create guard to ensure file is restored
    let guard = match MutationGuard::new(&mutant.file_path) {
        Ok(g) => g,
        Err(_) => {
            return MutationResult::new(mutant.clone(), MutantStatus::BuildError, 0);
        }
    };

    // Apply the mutation
    let mutated_content = mutant.apply(source);
    let mut guard = guard;
    if guard.apply(&mutated_content).is_err() {
        return MutationResult::new(mutant.clone(), MutantStatus::BuildError, 0);
    }

    // Run tests asynchronously
    let status = run_tests_async(config).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    // Restore original file
    drop(guard);

    MutationResult::new(mutant.clone(), status, duration_ms)
}

/// Run tests asynchronously with timeout.
async fn run_tests_async(config: &ExecutorConfig) -> MutantStatus {
    let mut cmd = if cfg!(windows) {
        let mut c = AsyncCommand::new("cmd");
        c.args(["/C", &config.test_command]);
        c
    } else {
        let mut c = AsyncCommand::new("sh");
        c.args(["-c", &config.test_command]);
        c
    };

    if let Some(dir) = &config.working_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let timeout_duration = Duration::from_secs(config.timeout_secs);

    match timeout(timeout_duration, async {
        match cmd.spawn() {
            Ok(mut child) => match child.wait().await {
                Ok(status) => {
                    if status.success() {
                        ExecutionResult::Success
                    } else {
                        ExecutionResult::Failed
                    }
                }
                Err(_) => ExecutionResult::Error,
            },
            Err(_) => ExecutionResult::Error,
        }
    })
    .await
    {
        Ok(ExecutionResult::Success) => MutantStatus::Survived,
        Ok(ExecutionResult::Failed) => MutantStatus::Killed,
        Ok(ExecutionResult::Error) => MutantStatus::BuildError,
        Ok(ExecutionResult::Timeout) => MutantStatus::Timeout,
        Err(_) => MutantStatus::Timeout, // Timeout elapsed
    }
}

/// Execute a command with a timeout.
fn execute_with_timeout(cmd: &mut Command, timeout: Duration) -> ExecutionResult {
    let start = Instant::now();

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return ExecutionResult::Error,
    };

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return ExecutionResult::Success;
                } else {
                    return ExecutionResult::Failed;
                }
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ExecutionResult::Timeout;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return ExecutionResult::Error,
        }
    }
}

/// Detect the appropriate test command for a project.
pub fn detect_test_command(path: &Path) -> Option<String> {
    // Rust (Cargo)
    if path.join("Cargo.toml").exists() {
        return Some("cargo test".to_string());
    }

    // Go
    if path.join("go.mod").exists() {
        return Some("go test ./...".to_string());
    }

    // Node.js / JavaScript / TypeScript
    if path.join("package.json").exists() {
        // Check for common test runners
        if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
            if content.contains("\"jest\"") {
                return Some("npx jest".to_string());
            }
            if content.contains("\"vitest\"") {
                return Some("npx vitest run".to_string());
            }
            if content.contains("\"mocha\"") {
                return Some("npx mocha".to_string());
            }
        }
        return Some("npm test".to_string());
    }

    // Python
    if path.join("pytest.ini").exists()
        || path.join("pyproject.toml").exists()
        || path.join("setup.py").exists()
    {
        return Some("pytest".to_string());
    }
    if path.join("tox.ini").exists() {
        return Some("tox".to_string());
    }

    // Ruby
    if path.join("Gemfile").exists() {
        if path.join("spec").exists() {
            return Some("bundle exec rspec".to_string());
        }
        if path.join("test").exists() {
            return Some("bundle exec rake test".to_string());
        }
    }

    // Java (Maven)
    if path.join("pom.xml").exists() {
        return Some("mvn test".to_string());
    }

    // Java (Gradle)
    if path.join("build.gradle").exists() || path.join("build.gradle.kts").exists() {
        return Some("./gradlew test".to_string());
    }

    // .NET
    if path.join("*.csproj").exists() || path.join("*.sln").exists() {
        return Some("dotnet test".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicUsize;
    use tempfile::TempDir;

    #[test]
    fn test_executor_config_default() {
        let config = ExecutorConfig::default();
        assert_eq!(config.test_command, "cargo test");
        assert_eq!(config.timeout_secs, 30);
        assert!(!config.capture_output);
    }

    #[test]
    fn test_executor_config_builder() {
        let config = ExecutorConfig::with_command("npm test")
            .timeout(60)
            .capture_output(true);

        assert_eq!(config.test_command, "npm test");
        assert_eq!(config.timeout_secs, 60);
        assert!(config.capture_output);
    }

    #[test]
    fn test_detect_test_command_cargo() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("cargo test".to_string()));
    }

    #[test]
    fn test_detect_test_command_go() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("go.mod"), "module test").unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("go test ./...".to_string()));
    }

    #[test]
    fn test_detect_test_command_npm() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("package.json"), r#"{"name": "test"}"#).unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("npm test".to_string()));
    }

    #[test]
    fn test_detect_test_command_jest() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("npx jest".to_string()));
    }

    #[test]
    fn test_detect_test_command_pytest() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("pytest.ini"), "[pytest]").unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("pytest".to_string()));
    }

    #[test]
    fn test_detect_test_command_maven() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("pom.xml"), "<project/>").unwrap();

        let cmd = detect_test_command(temp_dir.path());
        assert_eq!(cmd, Some("mvn test".to_string()));
    }

    #[test]
    fn test_detect_test_command_none() {
        let temp_dir = TempDir::new().unwrap();
        let cmd = detect_test_command(temp_dir.path());
        assert!(cmd.is_none());
    }

    #[test]
    fn test_executor_new() {
        let config = ExecutorConfig::default();
        let executor = MutantExecutor::new(config);
        assert_eq!(executor.config().test_command, "cargo test");
    }

    #[test]
    fn test_execute_with_timeout_success() {
        let mut cmd = Command::new("true");
        let result = execute_with_timeout(&mut cmd, Duration::from_secs(5));
        assert!(matches!(result, ExecutionResult::Success));
    }

    #[test]
    fn test_execute_with_timeout_failed() {
        let mut cmd = Command::new("false");
        let result = execute_with_timeout(&mut cmd, Duration::from_secs(5));
        assert!(matches!(result, ExecutionResult::Failed));
    }

    #[test]
    fn test_execute_with_timeout_timeout() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let result = execute_with_timeout(&mut cmd, Duration::from_millis(100));
        assert!(matches!(result, ExecutionResult::Timeout));
    }

    #[test]
    fn test_execute_with_timeout_error() {
        let mut cmd = Command::new("nonexistent_command_12345");
        let result = execute_with_timeout(&mut cmd, Duration::from_secs(5));
        assert!(matches!(result, ExecutionResult::Error));
    }

    // --- ExecutorConfig additional tests ---

    #[test]
    fn test_executor_config_jobs() {
        let config = ExecutorConfig::with_command("cargo test").jobs(8);
        assert_eq!(config.jobs, 8);
    }

    #[test]
    fn test_executor_config_effective_jobs() {
        let config = ExecutorConfig::with_command("cargo test").jobs(4);
        assert_eq!(config.effective_jobs(), 4);

        let config = ExecutorConfig::with_command("cargo test").jobs(0);
        assert!(config.effective_jobs() >= 1);
    }

    #[test]
    fn test_executor_config_progress_callback() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let config =
            ExecutorConfig::with_command("cargo test").progress_callback(move |_progress| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            });

        assert!(config.progress_callback.is_some());
    }

    #[test]
    fn test_executor_config_debug() {
        let config = ExecutorConfig::with_command("cargo test")
            .jobs(4)
            .timeout(60)
            .capture_output(true);

        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("cargo test"));
        assert!(debug_str.contains("60"));
        assert!(debug_str.contains("4"));
    }

    // --- AsyncMutantExecutor tests ---

    #[test]
    fn test_async_executor_new() {
        let config = ExecutorConfig::with_command("cargo test").jobs(4);
        let executor = AsyncMutantExecutor::new(config);

        assert_eq!(executor.config().test_command, "cargo test");
        assert!(!executor.is_shutdown());
    }

    #[test]
    fn test_async_executor_shutdown() {
        let config = ExecutorConfig::default();
        let executor = AsyncMutantExecutor::new(config);

        assert!(!executor.is_shutdown());
        executor.shutdown();
        assert!(executor.is_shutdown());
    }

    #[tokio::test]
    async fn test_async_executor_execute_empty() {
        let config = ExecutorConfig::with_command("true").jobs(2);
        let executor = AsyncMutantExecutor::new(config);

        let results = executor
            .execute_mutants(&[], &HashMap::new())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_async_executor_execute_single() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"let x = 42;").unwrap();

        let mutant = Mutant::new(
            "mut-1",
            &file_path,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (8, 10),
        );

        let config = ExecutorConfig::with_command("true").jobs(1).timeout(5);
        let executor = AsyncMutantExecutor::new(config);

        let result = executor
            .execute_mutant(&mutant, b"let x = 42;")
            .await
            .unwrap();

        // "true" always succeeds, so mutant survives
        assert_eq!(result.status, MutantStatus::Survived);
    }

    #[tokio::test]
    async fn test_async_executor_execute_killed() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"let x = 42;").unwrap();

        let mutant = Mutant::new(
            "mut-1",
            &file_path,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (8, 10),
        );

        let config = ExecutorConfig::with_command("false").jobs(1).timeout(5);
        let executor = AsyncMutantExecutor::new(config);

        let result = executor
            .execute_mutant(&mutant, b"let x = 42;")
            .await
            .unwrap();

        // "false" always fails, so mutant is killed
        assert_eq!(result.status, MutantStatus::Killed);
    }

    #[tokio::test]
    async fn test_async_executor_execute_multiple_parallel() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple files
        let file1 = temp_dir.path().join("test1.rs");
        let file2 = temp_dir.path().join("test2.rs");
        fs::write(&file1, b"let a = 1;").unwrap();
        fs::write(&file2, b"let b = 2;").unwrap();

        let mutants = vec![
            Mutant::new("mut-1", &file1, "CRR", 1, 1, "1", "0", "desc", (8, 9)),
            Mutant::new("mut-2", &file2, "CRR", 1, 1, "2", "0", "desc", (8, 9)),
        ];

        let mut sources = HashMap::new();
        sources.insert(file1.clone(), b"let a = 1;".to_vec());
        sources.insert(file2.clone(), b"let b = 2;".to_vec());

        let config = ExecutorConfig::with_command("true").jobs(2);
        let executor = AsyncMutantExecutor::new(config);

        let results = executor.execute_mutants(&mutants, &sources).await.unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_async_executor_file_restoration() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let original = b"let x = 42;";
        fs::write(&file_path, original).unwrap();

        let mutant = Mutant::new(
            "mut-1",
            &file_path,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (8, 10),
        );

        let config = ExecutorConfig::with_command("true").jobs(1);
        let executor = AsyncMutantExecutor::new(config);

        let _ = executor.execute_mutant(&mutant, original).await;

        // File should be restored to original
        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, original);
    }

    #[tokio::test]
    async fn test_async_executor_progress_callback() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"let x = 42;").unwrap();

        let mutant = Mutant::new(
            "mut-1",
            &file_path,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (8, 10),
        );

        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);

        let mut sources = HashMap::new();
        sources.insert(file_path.clone(), b"let x = 42;".to_vec());

        let config = ExecutorConfig::with_command("true")
            .jobs(1)
            .progress_callback(move |_| {
                callback_count_clone.fetch_add(1, Ordering::Relaxed);
            });

        let executor = AsyncMutantExecutor::new(config);
        let _ = executor.execute_mutants(&[mutant], &sources).await;

        assert_eq!(callback_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_async_executor_timeout() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"let x = 42;").unwrap();

        let mutant = Mutant::new(
            "mut-1",
            &file_path,
            "CRR",
            1,
            1,
            "42",
            "0",
            "test mutation",
            (8, 10),
        );

        // Command that sleeps longer than timeout
        let config = ExecutorConfig::with_command("sleep 10").jobs(1).timeout(1);
        let executor = AsyncMutantExecutor::new(config);

        let result = executor
            .execute_mutant(&mutant, b"let x = 42;")
            .await
            .unwrap();

        assert_eq!(result.status, MutantStatus::Timeout);
    }

    #[tokio::test]
    async fn test_run_tests_async_success() {
        let config = ExecutorConfig::with_command("true");
        let status = run_tests_async(&config).await;
        assert_eq!(status, MutantStatus::Survived);
    }

    #[tokio::test]
    async fn test_run_tests_async_failed() {
        let config = ExecutorConfig::with_command("false");
        let status = run_tests_async(&config).await;
        assert_eq!(status, MutantStatus::Killed);
    }

    #[tokio::test]
    async fn test_run_tests_async_timeout() {
        let config = ExecutorConfig::with_command("sleep 10").timeout(1);
        let status = run_tests_async(&config).await;
        assert_eq!(status, MutantStatus::Timeout);
    }

    #[tokio::test]
    async fn test_run_tests_async_error() {
        let config = ExecutorConfig::with_command("nonexistent_command_12345");
        let status = run_tests_async(&config).await;
        // Shell returns non-zero exit code for command not found, which counts as Killed
        assert_eq!(status, MutantStatus::Killed);
    }
}
