//! Temporal coupling analysis.
//!
//! Identifies files that frequently change together in version control.
//! Files with high temporal coupling that don't have explicit import
//! relationships may indicate hidden dependencies or poor module boundaries.
//!
//! # References
//!
//! - Ball, T., Kim, J., Porter, A., Siy, H. (1997) "If Your Version Control
//!   System Could Talk", Proceedings of the ICSE'97 Workshop
//!
//! # Coupling Strength
//!
//! Uses a symmetric formula: `cochanges / max(commits_a, commits_b)`
//! - 0.5 threshold for "strong" coupling is a heuristic
//! - Min cochanges (default 3) filters statistical noise

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Result};
use crate::git::GitRepo;

/// Default minimum number of co-changes to consider files coupled.
pub const DEFAULT_MIN_COCHANGES: u32 = 3;

/// Default number of days to analyze.
pub const DEFAULT_DAYS: u32 = 30;

/// Threshold for considering coupling "strong" (>= 0.5).
pub const STRONG_COUPLING_THRESHOLD: f64 = 0.5;

/// Temporal coupling analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Number of days of history to analyze.
    pub days: u32,
    /// Minimum co-change count to report.
    pub min_cochanges: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            days: DEFAULT_DAYS,
            min_cochanges: DEFAULT_MIN_COCHANGES,
        }
    }
}

/// Temporal coupling analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Creates a new temporal coupling analyzer with default config.
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Creates a new analyzer with the specified config.
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Sets the number of days of history to analyze.
    pub fn with_days(mut self, days: u32) -> Self {
        self.config.days = days;
        self
    }

    /// Sets the minimum co-change count threshold.
    pub fn with_min_cochanges(mut self, min: u32) -> Self {
        self.config.min_cochanges = min;
        self
    }

    /// Analyzes temporal coupling in a repository.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let git_repo = GitRepo::open(repo_path)?;
        self.analyze_with_git(&git_repo, repo_path)
    }

    /// Analyzes temporal coupling using an existing git repo.
    fn analyze_with_git(&self, git_repo: &GitRepo, _root: &Path) -> Result<Analysis> {
        // Format since for git log (git accepts "N days" format)
        let since_str = format!("{} days", self.config.days);

        // Get commit log with file changes
        let commits = git_repo.log_with_stats(Some(&since_str))?;

        // Track co-changes: normalized pair -> count
        let mut cochanges: HashMap<FilePair, u32> = HashMap::new();
        // Track individual file commits: file -> count
        let mut file_commits: HashMap<String, u32> = HashMap::new();

        for commit in &commits {
            let changed_files: Vec<String> = commit
                .files
                .iter()
                .map(|f| f.path.to_string_lossy().to_string())
                .collect();

            // Update individual file commit counts
            for file in &changed_files {
                *file_commits.entry(file.clone()).or_insert(0) += 1;
            }

            // Record co-changes for all pairs
            for i in 0..changed_files.len() {
                for j in (i + 1)..changed_files.len() {
                    let pair = FilePair::new(&changed_files[i], &changed_files[j]);
                    *cochanges.entry(pair).or_insert(0) += 1;
                }
            }
        }

        // Build coupling results, filtering by minimum threshold
        let mut couplings: Vec<FileCoupling> = cochanges
            .into_iter()
            .filter(|(_, count)| *count >= self.config.min_cochanges)
            .map(|(pair, cochange_count)| {
                let commits_a = file_commits.get(&pair.a).copied().unwrap_or(0);
                let commits_b = file_commits.get(&pair.b).copied().unwrap_or(0);
                let coupling_strength =
                    calculate_coupling_strength(cochange_count, commits_a, commits_b);

                FileCoupling {
                    file_a: pair.a,
                    file_b: pair.b,
                    cochange_count,
                    coupling_strength,
                    commits_a,
                    commits_b,
                }
            })
            .collect();

        // Sort by coupling strength (highest first)
        couplings.sort_by(|a, b| {
            b.coupling_strength
                .partial_cmp(&a.coupling_strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let generated_at = Utc::now();
        let total_files = file_commits.len();
        let summary = calculate_summary(&couplings, total_files);

        Ok(Analysis {
            generated_at: generated_at.to_rfc3339(),
            period_days: self.config.days,
            min_cochanges: self.config.min_cochanges,
            couplings,
            summary,
        })
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "temporal"
    }

    fn description(&self) -> &'static str {
        "Find files that change together (hidden dependencies)"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let git_path = ctx
            .git_path
            .as_ref()
            .ok_or_else(|| Error::git("Temporal coupling analysis requires git history"))?;

        let git_repo = GitRepo::open(git_path)?;
        self.analyze_with_git(&git_repo, ctx.root)
    }
}

/// Represents an unordered pair of files (normalized alphabetically).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FilePair {
    a: String,
    b: String,
}

impl FilePair {
    /// Creates a normalized file pair (alphabetically ordered).
    fn new(a: &str, b: &str) -> Self {
        if a <= b {
            Self {
                a: a.to_string(),
                b: b.to_string(),
            }
        } else {
            Self {
                a: b.to_string(),
                b: a.to_string(),
            }
        }
    }
}

/// Calculates the coupling strength between two files.
/// Strength = cochanges / max(commits_a, commits_b), capped at 1.0.
pub fn calculate_coupling_strength(cochanges: u32, commits_a: u32, commits_b: u32) -> f64 {
    let max_commits = commits_a.max(commits_b);
    if max_commits == 0 {
        return 0.0;
    }
    let strength = f64::from(cochanges) / f64::from(max_commits);
    strength.min(1.0)
}

/// Calculates summary statistics from couplings.
fn calculate_summary(couplings: &[FileCoupling], total_files: usize) -> Summary {
    let total_couplings = couplings.len();

    if couplings.is_empty() {
        return Summary {
            total_couplings: 0,
            strong_couplings: 0,
            avg_coupling_strength: 0.0,
            max_coupling_strength: 0.0,
            total_files_analyzed: total_files,
        };
    }

    // Couplings are sorted by strength descending, so first is max
    let max_coupling_strength = couplings[0].coupling_strength;

    let mut sum = 0.0;
    let mut strong_count = 0;
    for c in couplings {
        sum += c.coupling_strength;
        if c.coupling_strength >= STRONG_COUPLING_THRESHOLD {
            strong_count += 1;
        }
    }

    Summary {
        total_couplings,
        strong_couplings: strong_count,
        avg_coupling_strength: sum / total_couplings as f64,
        max_coupling_strength,
        total_files_analyzed: total_files,
    }
}

/// Temporal coupling analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// When the analysis was generated.
    pub generated_at: String,
    /// Number of days of history analyzed.
    pub period_days: u32,
    /// Minimum co-change threshold used.
    pub min_cochanges: u32,
    /// File couplings found, sorted by strength descending.
    pub couplings: Vec<FileCoupling>,
    /// Summary statistics.
    pub summary: Summary,
}

/// Represents the temporal coupling between two files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoupling {
    /// First file in the pair.
    pub file_a: String,
    /// Second file in the pair.
    pub file_b: String,
    /// Number of times files changed together.
    pub cochange_count: u32,
    /// Coupling strength (0.0 - 1.0).
    pub coupling_strength: f64,
    /// Total commits touching file_a.
    pub commits_a: u32,
    /// Total commits touching file_b.
    pub commits_b: u32,
}

/// Aggregate statistics for temporal coupling analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    /// Total number of file couplings found.
    pub total_couplings: usize,
    /// Number of strong couplings (strength >= 0.5).
    pub strong_couplings: usize,
    /// Average coupling strength across all pairs.
    pub avg_coupling_strength: f64,
    /// Maximum coupling strength found.
    pub max_coupling_strength: f64,
    /// Total number of files analyzed.
    pub total_files_analyzed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.days, DEFAULT_DAYS);
        assert_eq!(config.min_cochanges, DEFAULT_MIN_COCHANGES);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.config.days, DEFAULT_DAYS);
        assert_eq!(analyzer.config.min_cochanges, DEFAULT_MIN_COCHANGES);
    }

    #[test]
    fn test_analyzer_with_days() {
        let analyzer = Analyzer::new().with_days(60);
        assert_eq!(analyzer.config.days, 60);
    }

    #[test]
    fn test_analyzer_with_min_cochanges() {
        let analyzer = Analyzer::new().with_min_cochanges(5);
        assert_eq!(analyzer.config.min_cochanges, 5);
    }

    #[test]
    fn test_analyzer_with_config() {
        let config = Config {
            days: 90,
            min_cochanges: 10,
        };
        let analyzer = Analyzer::with_config(config);
        assert_eq!(analyzer.config.days, 90);
        assert_eq!(analyzer.config.min_cochanges, 10);
    }

    #[test]
    fn test_file_pair_normalization() {
        let pair1 = FilePair::new("b.rs", "a.rs");
        let pair2 = FilePair::new("a.rs", "b.rs");
        assert_eq!(pair1.a, "a.rs");
        assert_eq!(pair1.b, "b.rs");
        assert_eq!(pair1, pair2);
    }

    #[test]
    fn test_file_pair_same_order() {
        let pair = FilePair::new("a.rs", "z.rs");
        assert_eq!(pair.a, "a.rs");
        assert_eq!(pair.b, "z.rs");
    }

    #[test]
    fn test_coupling_strength_zero_commits() {
        let strength = calculate_coupling_strength(5, 0, 0);
        assert_eq!(strength, 0.0);
    }

    #[test]
    fn test_coupling_strength_normal() {
        // 3 cochanges, file A changed 10 times, file B changed 5 times
        // strength = 3 / max(10, 5) = 3/10 = 0.3
        let strength = calculate_coupling_strength(3, 10, 5);
        assert!((strength - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_coupling_strength_perfect() {
        // 5 cochanges, both files changed 5 times = perfect coupling
        let strength = calculate_coupling_strength(5, 5, 5);
        assert!((strength - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_coupling_strength_capped_at_one() {
        // Edge case: more cochanges than individual commits (shouldn't happen but handle it)
        let strength = calculate_coupling_strength(10, 5, 5);
        assert_eq!(strength, 1.0);
    }

    #[test]
    fn test_coupling_strength_asymmetric() {
        // 4 cochanges, file A changed 4 times, file B changed 8 times
        // strength = 4 / max(4, 8) = 4/8 = 0.5
        let strength = calculate_coupling_strength(4, 4, 8);
        assert!((strength - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_summary_empty() {
        let summary = calculate_summary(&[], 0);
        assert_eq!(summary.total_couplings, 0);
        assert_eq!(summary.strong_couplings, 0);
        assert_eq!(summary.avg_coupling_strength, 0.0);
        assert_eq!(summary.max_coupling_strength, 0.0);
        assert_eq!(summary.total_files_analyzed, 0);
    }

    #[test]
    fn test_summary_single_coupling() {
        let couplings = vec![FileCoupling {
            file_a: "a.rs".to_string(),
            file_b: "b.rs".to_string(),
            cochange_count: 5,
            coupling_strength: 0.8,
            commits_a: 5,
            commits_b: 6,
        }];
        let summary = calculate_summary(&couplings, 2);
        assert_eq!(summary.total_couplings, 1);
        assert_eq!(summary.strong_couplings, 1); // 0.8 >= 0.5
        assert!((summary.avg_coupling_strength - 0.8).abs() < 0.001);
        assert!((summary.max_coupling_strength - 0.8).abs() < 0.001);
        assert_eq!(summary.total_files_analyzed, 2);
    }

    #[test]
    fn test_summary_multiple_couplings() {
        let couplings = vec![
            FileCoupling {
                file_a: "a.rs".to_string(),
                file_b: "b.rs".to_string(),
                cochange_count: 5,
                coupling_strength: 0.9,
                commits_a: 5,
                commits_b: 5,
            },
            FileCoupling {
                file_a: "c.rs".to_string(),
                file_b: "d.rs".to_string(),
                cochange_count: 3,
                coupling_strength: 0.5,
                commits_a: 6,
                commits_b: 6,
            },
            FileCoupling {
                file_a: "e.rs".to_string(),
                file_b: "f.rs".to_string(),
                cochange_count: 3,
                coupling_strength: 0.3,
                commits_a: 10,
                commits_b: 10,
            },
        ];
        let summary = calculate_summary(&couplings, 6);
        assert_eq!(summary.total_couplings, 3);
        assert_eq!(summary.strong_couplings, 2); // 0.9 and 0.5 are >= 0.5
        assert!((summary.avg_coupling_strength - ((0.9 + 0.5 + 0.3) / 3.0)).abs() < 0.001);
        assert!((summary.max_coupling_strength - 0.9).abs() < 0.001);
        assert_eq!(summary.total_files_analyzed, 6);
    }

    #[test]
    fn test_summary_no_strong_couplings() {
        let couplings = vec![
            FileCoupling {
                file_a: "a.rs".to_string(),
                file_b: "b.rs".to_string(),
                cochange_count: 3,
                coupling_strength: 0.3,
                commits_a: 10,
                commits_b: 10,
            },
            FileCoupling {
                file_a: "c.rs".to_string(),
                file_b: "d.rs".to_string(),
                cochange_count: 3,
                coupling_strength: 0.2,
                commits_a: 15,
                commits_b: 15,
            },
        ];
        let summary = calculate_summary(&couplings, 4);
        assert_eq!(summary.total_couplings, 2);
        assert_eq!(summary.strong_couplings, 0); // Neither >= 0.5
    }

    #[test]
    fn test_file_coupling_fields() {
        let coupling = FileCoupling {
            file_a: "src/lib.rs".to_string(),
            file_b: "src/main.rs".to_string(),
            cochange_count: 10,
            coupling_strength: 0.75,
            commits_a: 12,
            commits_b: 13,
        };
        assert_eq!(coupling.file_a, "src/lib.rs");
        assert_eq!(coupling.file_b, "src/main.rs");
        assert_eq!(coupling.cochange_count, 10);
        assert!((coupling.coupling_strength - 0.75).abs() < 0.001);
        assert_eq!(coupling.commits_a, 12);
        assert_eq!(coupling.commits_b, 13);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            period_days: 30,
            min_cochanges: 3,
            couplings: vec![FileCoupling {
                file_a: "a.rs".to_string(),
                file_b: "b.rs".to_string(),
                cochange_count: 5,
                coupling_strength: 0.8,
                commits_a: 5,
                commits_b: 6,
            }],
            summary: Summary {
                total_couplings: 1,
                strong_couplings: 1,
                avg_coupling_strength: 0.8,
                max_coupling_strength: 0.8,
                total_files_analyzed: 2,
            },
        };

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("\"period_days\":30"));
        assert!(json.contains("\"min_cochanges\":3"));
        assert!(json.contains("\"coupling_strength\":0.8"));
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "temporal");
        assert!(analyzer.description().contains("change together"));
        assert!(analyzer.requires_git());
    }

    #[test]
    fn test_strong_coupling_threshold() {
        // Test that the threshold constant is correct
        assert!((STRONG_COUPLING_THRESHOLD - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_file_pair_hash_equality() {
        // Test that equal pairs have equal hashes (for HashMap)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let pair1 = FilePair::new("z.rs", "a.rs");
        let pair2 = FilePair::new("a.rs", "z.rs");

        let mut hasher1 = DefaultHasher::new();
        pair1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        pair2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_analyze_with_git_collects_file_changes() {
        // Verifies that analyze_with_git uses log_with_stats() to get
        // file-level changes, not log() which returns empty file lists.
        use crate::git::GitRepo;
        use std::path::PathBuf;

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Open the omen repo itself
        let git_repo = match GitRepo::open(&repo_root) {
            Ok(repo) => repo,
            Err(_) => return, // Skip if not in a git repo
        };

        // Use a longer time window to ensure we have file changes
        let analyzer = Analyzer::new().with_days(365).with_min_cochanges(1);
        let result = analyzer.analyze_with_git(&git_repo, &repo_root).unwrap();

        // The omen repo has git history with file changes.
        // If analyze_with_git works correctly and there are files that changed together,
        // we should have some data in file_commits tracking.
        // With min_cochanges=1, any files that changed together at least once should appear.
        // Even if no couplings meet the threshold, the function should process commits with files.

        // The key assertion: if commits have files, we can verify by checking
        // that the summary shows files were analyzed (even if no couplings found).
        // With empty file lists, total_files_analyzed would be 0.
        assert!(
            result.summary.total_files_analyzed > 0 || !result.couplings.is_empty(),
            "temporal analysis should process file changes from git history"
        );
    }
}
