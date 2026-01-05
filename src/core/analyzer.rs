//! Analyzer trait and common types.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use super::{FileSet, Result};
use crate::config::Config;
use crate::git::GitRepo;

/// Trait implemented by all analyzers.
pub trait Analyzer: Send + Sync {
    /// The result type produced by this analyzer.
    type Output: Serialize + Send;

    /// Unique identifier for this analyzer.
    fn name(&self) -> &'static str;

    /// Human-readable description.
    fn description(&self) -> &'static str;

    /// Run analysis and return results.
    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output>;

    /// Whether this analyzer requires git history.
    fn requires_git(&self) -> bool {
        false
    }

    /// Configure the analyzer from config.
    fn configure(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}

/// Context shared by all analyzers during analysis.
pub struct AnalysisContext<'a> {
    /// Root directory being analyzed.
    pub root: &'a Path,
    /// Set of files to analyze.
    pub files: &'a FileSet,
    /// Git repository path (for creating thread-local repos).
    pub git_path: Option<&'a Path>,
    /// Configuration.
    pub config: &'a Config,
    /// Progress callback.
    pub on_progress: Option<Box<dyn Fn(usize, usize) + Send + Sync + 'a>>,
}

impl<'a> AnalysisContext<'a> {
    /// Create a new analysis context.
    pub fn new(files: &'a FileSet, config: &'a Config, root: Option<&'a Path>) -> Self {
        Self {
            root: root.unwrap_or_else(|| files.root()),
            files,
            git_path: None,
            config,
            on_progress: None,
        }
    }

    /// Add git repository path to context.
    pub fn with_git_path(mut self, git_path: &'a Path) -> Self {
        self.git_path = Some(git_path);
        self
    }

    /// Open a thread-local git repository (for parallel operations).
    pub fn open_git(&self) -> Result<Option<GitRepo>> {
        if let Some(path) = self.git_path {
            Ok(Some(GitRepo::open(path)?))
        } else {
            Ok(None)
        }
    }

    /// Add progress callback.
    pub fn with_progress<F>(mut self, f: F) -> Self
    where
        F: Fn(usize, usize) + Send + Sync + 'a,
    {
        self.on_progress = Some(Box::new(f));
        self
    }

    /// Report progress if callback is set.
    pub fn report_progress(&self, current: usize, total: usize) {
        if let Some(ref f) = self.on_progress {
            f(current, total);
        }
    }
}

/// Type-erased analysis result container.
#[derive(Debug)]
pub struct AnalysisResult {
    /// Name of the analyzer that produced this result.
    pub analyzer: &'static str,
    /// Serialized result data (JSON).
    pub data: serde_json::Value,
    /// Quick summary statistics.
    pub summary: Summary,
}

impl AnalysisResult {
    /// Create a new analysis result.
    pub fn new<T: Serialize>(analyzer: &'static str, data: &T, summary: Summary) -> Result<Self> {
        Ok(Self {
            analyzer,
            data: serde_json::to_value(data)?,
            summary,
        })
    }
}

/// Quick summary statistics for display.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    /// Number of files analyzed.
    pub files_analyzed: usize,
    /// Number of issues found.
    pub issues_found: usize,
    /// Analysis duration.
    #[serde(with = "duration_serde")]
    pub duration: Duration,
}

impl Summary {
    /// Create a new summary.
    pub fn new(files_analyzed: usize, issues_found: usize, duration: Duration) -> Self {
        Self {
            files_analyzed,
            issues_found,
            duration,
        }
    }
}

mod duration_serde {
    use std::time::Duration;

    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(duration.as_secs_f64())
    }

    #[allow(dead_code)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_new() {
        let summary = Summary::new(10, 5, Duration::from_millis(100));
        assert_eq!(summary.files_analyzed, 10);
        assert_eq!(summary.issues_found, 5);
        assert_eq!(summary.duration, Duration::from_millis(100));
    }
}
