//! CI/CD integration for mutation testing.
//!
//! This module provides tools for integrating mutation testing into CI/CD pipelines:
//!
//! - **Baseline tracking**: Store and compare mutation scores across commits
//! - **Incremental mode**: Only test mutants in changed files/lines
//! - **GitHub integration**: Post results as PR comments and check runs
//!
//! # Example
//!
//! ```no_run
//! use omen::analyzers::mutation::ci::{
//!     MutationBaseline, IncrementalMutation, GitHubReporter,
//! };
//! use std::path::Path;
//!
//! // Load previous baseline
//! let baseline = MutationBaseline::load(Path::new(".omen/mutation-baseline.json")).ok();
//!
//! // Filter to only changed files
//! let incremental = IncrementalMutation::new("origin/main");
//! // let changed_files = incremental.get_changed_files(Path::new("."))?;
//!
//! // Report to GitHub if in CI
//! if let Some(reporter) = GitHubReporter::from_env() {
//!     // let comment = reporter.format_pr_comment(&analysis, baseline.as_ref());
//!     // reporter.post_pr_comment(pr_number, &comment).await?;
//! }
//! ```

mod baseline;
mod github;
mod incremental;

pub use baseline::{BaselineComparison, MutationBaseline, OperatorScore};
pub use github::{get_pr_number_from_event, GitHubReporter};
pub use incremental::IncrementalMutation;
