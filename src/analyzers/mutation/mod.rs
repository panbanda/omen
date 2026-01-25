//! Mutation testing analyzer.
//!
//! This module provides native AST-based mutation testing using tree-sitter.
//! It generates mutants from source code, executes tests against them, and
//! reports mutation scores.
//!
//! # Overview
//!
//! Mutation testing works by introducing small, deliberate changes (mutations)
//! to source code and checking if the test suite catches them. A mutation that
//! is detected by tests is "killed"; one that isn't is "survived".
//!
//! **Mutation Score** = Killed / (Killed + Survived)
//!
//! A higher mutation score indicates more effective tests.
//!
//! # Safety
//!
//! This module uses RAII guards to ensure source files are always restored
//! to their original state, even if a panic occurs during testing.

pub mod ci;
pub mod coverage;
pub mod equivalent;
mod executor;
mod generator;
pub mod ml_predictor;
mod mutant;
mod operator;
pub mod operators;
mod safety;
pub mod worker;

pub use executor::{
    detect_test_command, AsyncMutantExecutor, ExecutionResult, ExecutorConfig, MutantExecutor,
    ProgressCallback,
};
pub use generator::{filter_by_lines, filter_by_operators, MutantGenerator};
pub use mutant::{Mutant, MutantStatus, MutationResult};
pub use operator::{MutationOperator, OperatorRegistry};
pub use operators::{
    default_registry, fast_registry, full_registry, ArithmeticOperator, AssignmentOperator,
    BitwiseOperator, BoundaryOperator, ConditionalOperator, LiteralOperator, RelationalOperator,
    ReturnValueOperator, StatementOperator, UnaryOperator,
};
pub use safety::{atomic_write, has_uncommitted_changes, MutationGuard};
pub use worker::{
    FileLockManager, ProgressUpdate, WorkItem, WorkQueue, WorkerPoolConfig, WorkerPoolHandle,
};

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Result};

/// Mutation testing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MutationMode {
    /// All operators, all mutants.
    #[default]
    All,
    /// Skip equivalent, limit per-location.
    Fast,
    /// All + language-specific operators.
    Thorough,
}

/// Mutation testing analyzer.
pub struct Analyzer {
    /// Enabled operator names.
    operators: Vec<String>,
    /// Test command to run.
    test_command: Option<String>,
    /// Timeout in seconds.
    timeout_secs: u64,
    /// Whether to only generate mutants (no execution).
    dry_run: bool,
    /// Minimum mutation score threshold for check mode.
    min_score: Option<f64>,
    /// Number of parallel workers (0 = auto-detect).
    jobs: usize,
    /// Path to coverage JSON for filtering.
    coverage_path: Option<PathBuf>,
    /// Incremental mode (only changed files).
    incremental: bool,
    /// Baseline file for regression detection.
    baseline_path: Option<PathBuf>,
    /// Skip likely-equivalent mutants.
    skip_equivalent: bool,
    /// Mutation mode.
    mode: MutationMode,
    /// Output path for surviving mutants.
    output_survivors: Option<PathBuf>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new mutation testing analyzer.
    pub fn new() -> Self {
        Self {
            operators: vec!["CRR".to_string(), "ROR".to_string(), "AOR".to_string()],
            test_command: None,
            timeout_secs: 30,
            dry_run: false,
            min_score: None,
            jobs: 0,
            coverage_path: None,
            incremental: false,
            baseline_path: None,
            skip_equivalent: false,
            mode: MutationMode::All,
            output_survivors: None,
        }
    }

    /// Set the enabled operators.
    pub fn operators(mut self, operators: Vec<String>) -> Self {
        self.operators = operators;
        self
    }

    /// Set the test command.
    pub fn test_command(mut self, cmd: Option<String>) -> Self {
        self.test_command = cmd;
        self
    }

    /// Set the timeout.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Enable dry-run mode (generate only, no execution).
    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Set minimum score threshold.
    pub fn min_score(mut self, score: Option<f64>) -> Self {
        self.min_score = score;
        self
    }

    /// Set number of parallel workers (0 = auto-detect CPU count).
    pub fn jobs(mut self, jobs: usize) -> Self {
        self.jobs = jobs;
        self
    }

    /// Set path to coverage JSON file for filtering mutants.
    pub fn coverage_path(mut self, path: Option<PathBuf>) -> Self {
        self.coverage_path = path;
        self
    }

    /// Enable incremental mode (only test changed files).
    pub fn incremental(mut self, incremental: bool) -> Self {
        self.incremental = incremental;
        self
    }

    /// Set baseline file path for regression detection.
    pub fn baseline_path(mut self, path: Option<PathBuf>) -> Self {
        self.baseline_path = path;
        self
    }

    /// Enable skipping of likely-equivalent mutants.
    pub fn skip_equivalent(mut self, skip: bool) -> Self {
        self.skip_equivalent = skip;
        self
    }

    /// Set mutation mode.
    pub fn mode(mut self, mode: MutationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set output path for surviving mutants.
    pub fn output_survivors(mut self, path: Option<PathBuf>) -> Self {
        self.output_survivors = path;
        self
    }

    /// Get the appropriate registry based on mutation mode.
    #[allow(dead_code)]
    fn get_registry(&self) -> OperatorRegistry {
        match self.mode {
            MutationMode::All => default_registry(),
            MutationMode::Fast => fast_registry(),
            MutationMode::Thorough => full_registry(),
        }
    }

    /// Get effective number of workers.
    #[allow(dead_code)]
    fn effective_jobs(&self) -> usize {
        if self.jobs == 0 {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        } else {
            self.jobs
        }
    }

    /// Generate mutants for all files without executing tests.
    pub fn generate_only(&self, ctx: &AnalysisContext<'_>) -> Result<Analysis> {
        let start = Instant::now();

        // Build operator list
        let registry = default_registry();
        let operator_names: Vec<&str> = self.operators.iter().map(|s| s.as_str()).collect();
        let operators = registry.get_by_names(&operator_names);

        let generator = MutantGenerator::new(
            operators
                .into_iter()
                .map(|op| -> Box<dyn MutationOperator> {
                    match op.name() {
                        "CRR" => Box::new(LiteralOperator),
                        "ROR" => Box::new(RelationalOperator),
                        "AOR" => Box::new(ArithmeticOperator),
                        _ => Box::new(LiteralOperator), // Fallback
                    }
                })
                .collect(),
        );

        let total_files = ctx.files.len();
        let counter = Arc::new(AtomicUsize::new(0));

        // Generate mutants for all files in parallel
        let all_mutants: Vec<Mutant> = ctx
            .files
            .files()
            .par_iter()
            .filter_map(|path| {
                let result = generator.generate_for_file(path).ok()?;
                let current = counter.fetch_add(1, Ordering::Relaxed) + 1;
                ctx.report_progress(current, total_files);
                Some(result)
            })
            .flatten()
            .collect();

        let duration = start.elapsed();

        // Build file-level results (all pending)
        let mut file_results: HashMap<PathBuf, FileResult> = HashMap::new();
        for mutant in &all_mutants {
            let entry = file_results
                .entry(mutant.file_path.clone())
                .or_insert_with(|| FileResult {
                    path: mutant.file_path.to_string_lossy().to_string(),
                    mutants: Vec::new(),
                    killed: 0,
                    survived: 0,
                    timeout: 0,
                    error: 0,
                    score: 0.0,
                });

            entry.mutants.push(MutationResult::new(
                mutant.clone(),
                MutantStatus::Pending,
                0,
            ));
        }

        let files: Vec<FileResult> = file_results.into_values().collect();
        let summary = build_summary(&files, duration.as_millis() as u64);

        Ok(Analysis { files, summary })
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "mutation"
    }

    fn description(&self) -> &'static str {
        "Mutation testing to evaluate test suite effectiveness"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // If dry run, just generate mutants
        if self.dry_run {
            return self.generate_only(ctx);
        }

        let start = Instant::now();

        // Detect or use provided test command
        let project_root = ctx.files.root();
        let test_cmd = self
            .test_command
            .clone()
            .or_else(|| detect_test_command(project_root))
            .ok_or_else(|| {
                Error::analysis("Could not detect test command. Please provide --test-command")
            })?;

        // Build operator list
        let registry = default_registry();
        let operator_names: Vec<&str> = self.operators.iter().map(|s| s.as_str()).collect();
        let operators = registry.get_by_names(&operator_names);

        let generator = MutantGenerator::new(
            operators
                .into_iter()
                .map(|op| -> Box<dyn MutationOperator> {
                    match op.name() {
                        "CRR" => Box::new(LiteralOperator),
                        "ROR" => Box::new(RelationalOperator),
                        "AOR" => Box::new(ArithmeticOperator),
                        _ => Box::new(LiteralOperator),
                    }
                })
                .collect(),
        );

        let executor_config = ExecutorConfig::with_command(&test_cmd)
            .timeout(self.timeout_secs)
            .working_dir(project_root);
        let executor = MutantExecutor::new(executor_config);

        let total_files = ctx.files.len();
        let counter = Arc::new(AtomicUsize::new(0));

        // Process files sequentially (mutations need to be applied one at a time per file)
        let mut file_results = Vec::new();

        for path in ctx.files.files() {
            let mutants = match generator.generate_for_file(path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            if mutants.is_empty() {
                continue;
            }

            // Read the original source
            let source = match fs::read(path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut file_result = FileResult {
                path: path.to_string_lossy().to_string(),
                mutants: Vec::new(),
                killed: 0,
                survived: 0,
                timeout: 0,
                error: 0,
                score: 0.0,
            };

            // Execute each mutant
            for mutant in mutants {
                let result = match executor.execute_mutant(&mutant, &source) {
                    Ok(r) => r,
                    Err(_) => MutationResult::new(mutant, MutantStatus::BuildError, 0),
                };

                match result.status {
                    MutantStatus::Killed => file_result.killed += 1,
                    MutantStatus::Survived => file_result.survived += 1,
                    MutantStatus::Timeout => file_result.timeout += 1,
                    MutantStatus::BuildError | MutantStatus::Equivalent => file_result.error += 1,
                    MutantStatus::Pending => {}
                }

                file_result.mutants.push(result);
            }

            // Calculate score for this file
            let total_scored = file_result.killed + file_result.survived;
            if total_scored > 0 {
                file_result.score = file_result.killed as f64 / total_scored as f64;
            }

            file_results.push(file_result);

            let current = counter.fetch_add(1, Ordering::Relaxed) + 1;
            ctx.report_progress(current, total_files);
        }

        let duration = start.elapsed();
        let summary = build_summary(&file_results, duration.as_millis() as u64);

        // Check threshold if in check mode
        if let Some(min_score) = self.min_score {
            if summary.mutation_score < min_score {
                return Err(Error::threshold_violation(
                    format!(
                        "Mutation score {:.1}% is below minimum {:.1}%",
                        summary.mutation_score * 100.0,
                        min_score * 100.0
                    ),
                    summary.mutation_score,
                ));
            }
        }

        Ok(Analysis {
            files: file_results,
            summary,
        })
    }

    fn configure(&mut self, _config: &Config) -> Result<()> {
        // Could read from config file in the future
        Ok(())
    }
}

/// Mutation testing analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// Per-file results.
    pub files: Vec<FileResult>,
    /// Summary statistics.
    pub summary: Summary,
}

/// Per-file mutation testing result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResult {
    /// File path.
    pub path: String,
    /// Individual mutant results.
    pub mutants: Vec<MutationResult>,
    /// Number of killed mutants.
    pub killed: usize,
    /// Number of survived mutants.
    pub survived: usize,
    /// Number of timed-out mutants.
    pub timeout: usize,
    /// Number of error mutants.
    pub error: usize,
    /// Mutation score for this file.
    pub score: f64,
}

/// Summary statistics for mutation testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Total files analyzed.
    pub total_files: usize,
    /// Total mutants generated.
    pub total_mutants: usize,
    /// Total killed mutants.
    pub killed: usize,
    /// Total survived mutants.
    pub survived: usize,
    /// Total timed-out mutants.
    pub timeout: usize,
    /// Total error mutants.
    pub error: usize,
    /// Overall mutation score (killed / (killed + survived)).
    pub mutation_score: f64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Mutants by operator.
    pub by_operator: HashMap<String, OperatorStats>,
}

/// Statistics for a single operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorStats {
    /// Total mutants from this operator.
    pub total: usize,
    /// Killed mutants.
    pub killed: usize,
    /// Survived mutants.
    pub survived: usize,
}

/// Build summary from file results.
fn build_summary(files: &[FileResult], duration_ms: u64) -> Summary {
    let mut summary = Summary {
        total_files: files.len(),
        total_mutants: 0,
        killed: 0,
        survived: 0,
        timeout: 0,
        error: 0,
        mutation_score: 0.0,
        duration_ms,
        by_operator: HashMap::new(),
    };

    for file in files {
        summary.total_mutants += file.mutants.len();
        summary.killed += file.killed;
        summary.survived += file.survived;
        summary.timeout += file.timeout;
        summary.error += file.error;

        // Aggregate by operator
        for result in &file.mutants {
            let op = &result.mutant.operator;
            let entry = summary
                .by_operator
                .entry(op.clone())
                .or_insert(OperatorStats {
                    total: 0,
                    killed: 0,
                    survived: 0,
                });
            entry.total += 1;
            if result.status == MutantStatus::Killed {
                entry.killed += 1;
            } else if result.status == MutantStatus::Survived {
                entry.survived += 1;
            }
        }
    }

    let total_scored = summary.killed + summary.survived;
    if total_scored > 0 {
        summary.mutation_score = summary.killed as f64 / total_scored as f64;
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_new() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "mutation");
        assert!(!analyzer.dry_run);
    }

    #[test]
    fn test_analyzer_builder() {
        let analyzer = Analyzer::new()
            .operators(vec!["CRR".to_string()])
            .test_command(Some("cargo test".to_string()))
            .timeout(60)
            .dry_run(true)
            .min_score(Some(0.8));

        assert_eq!(analyzer.operators, vec!["CRR"]);
        assert_eq!(analyzer.test_command, Some("cargo test".to_string()));
        assert_eq!(analyzer.timeout_secs, 60);
        assert!(analyzer.dry_run);
        assert_eq!(analyzer.min_score, Some(0.8));
    }

    #[test]
    fn test_build_summary() {
        let files = vec![FileResult {
            path: "a.rs".to_string(),
            mutants: vec![
                MutationResult::new(
                    Mutant::new("1", "a.rs", "CRR", 1, 1, "1", "0", "desc", (0, 1)),
                    MutantStatus::Killed,
                    100,
                ),
                MutationResult::new(
                    Mutant::new("2", "a.rs", "CRR", 2, 1, "2", "0", "desc", (10, 11)),
                    MutantStatus::Survived,
                    100,
                ),
            ],
            killed: 1,
            survived: 1,
            timeout: 0,
            error: 0,
            score: 0.5,
        }];

        let summary = build_summary(&files, 1000);

        assert_eq!(summary.total_files, 1);
        assert_eq!(summary.total_mutants, 2);
        assert_eq!(summary.killed, 1);
        assert_eq!(summary.survived, 1);
        assert!((summary.mutation_score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_build_summary_empty() {
        let summary = build_summary(&[], 0);

        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_mutants, 0);
        assert_eq!(summary.mutation_score, 0.0);
    }

    #[test]
    fn test_build_summary_all_killed() {
        let files = vec![FileResult {
            path: "a.rs".to_string(),
            mutants: vec![MutationResult::new(
                Mutant::new("1", "a.rs", "CRR", 1, 1, "1", "0", "desc", (0, 1)),
                MutantStatus::Killed,
                100,
            )],
            killed: 1,
            survived: 0,
            timeout: 0,
            error: 0,
            score: 1.0,
        }];

        let summary = build_summary(&files, 100);
        assert!((summary.mutation_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_summary_by_operator() {
        let files = vec![FileResult {
            path: "a.rs".to_string(),
            mutants: vec![
                MutationResult::new(
                    Mutant::new("1", "a.rs", "CRR", 1, 1, "1", "0", "desc", (0, 1)),
                    MutantStatus::Killed,
                    100,
                ),
                MutationResult::new(
                    Mutant::new("2", "a.rs", "ROR", 2, 1, "<", ">", "desc", (10, 11)),
                    MutantStatus::Survived,
                    100,
                ),
            ],
            killed: 1,
            survived: 1,
            timeout: 0,
            error: 0,
            score: 0.5,
        }];

        let summary = build_summary(&files, 100);

        assert_eq!(summary.by_operator.len(), 2);
        assert_eq!(summary.by_operator["CRR"].killed, 1);
        assert_eq!(summary.by_operator["ROR"].survived, 1);
    }

    #[test]
    fn test_analyzer_new_options() {
        let analyzer = Analyzer::new()
            .jobs(4)
            .coverage_path(Some(PathBuf::from("coverage.json")))
            .incremental(true)
            .baseline_path(Some(PathBuf::from("baseline.json")))
            .skip_equivalent(true)
            .mode(MutationMode::Fast)
            .output_survivors(Some(PathBuf::from("survivors.json")));

        assert_eq!(analyzer.jobs, 4);
        assert_eq!(analyzer.coverage_path, Some(PathBuf::from("coverage.json")));
        assert!(analyzer.incremental);
        assert_eq!(analyzer.baseline_path, Some(PathBuf::from("baseline.json")));
        assert!(analyzer.skip_equivalent);
        assert_eq!(analyzer.mode, MutationMode::Fast);
        assert_eq!(
            analyzer.output_survivors,
            Some(PathBuf::from("survivors.json"))
        );
    }

    #[test]
    fn test_mutation_mode_default() {
        let mode = MutationMode::default();
        assert_eq!(mode, MutationMode::All);
    }

    #[test]
    fn test_get_registry_by_mode() {
        let analyzer = Analyzer::new().mode(MutationMode::All);
        let registry = analyzer.get_registry();
        assert_eq!(registry.operators().len(), 3);

        let analyzer = Analyzer::new().mode(MutationMode::Fast);
        let registry = analyzer.get_registry();
        assert_eq!(registry.operators().len(), 2);

        let analyzer = Analyzer::new().mode(MutationMode::Thorough);
        let registry = analyzer.get_registry();
        assert!(registry.operators().len() >= 3);
    }

    #[test]
    fn test_effective_jobs() {
        let analyzer = Analyzer::new().jobs(4);
        assert_eq!(analyzer.effective_jobs(), 4);

        let analyzer = Analyzer::new().jobs(0);
        assert!(analyzer.effective_jobs() > 0); // Should be num_cpus
    }
}
