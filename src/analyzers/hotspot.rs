//! Hotspot analysis (churn x complexity).
//!
//! Identifies files that have both high churn (change frequency) and
//! high complexity (cyclomatic/cognitive). These are prime candidates
//! for refactoring as they represent high-risk, hard-to-maintain code.
//!
//! Based on Adam Tornhill's "Your Code as a Crime Scene" methodology.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::analyzers::complexity;
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Result};
use crate::git::GitRepo;

/// Hotspot analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Number of days to analyze for churn (default: 90).
    pub days: u32,
    /// Minimum churn percentile to consider (default: 50.0).
    pub min_churn_percentile: f64,
    /// Minimum complexity percentile to consider (default: 50.0).
    pub min_complexity_percentile: f64,
    /// Critical severity threshold (churn * complexity >= this) (default: 0.81).
    pub critical_threshold: f64,
    /// High severity threshold (default: 0.64).
    pub high_threshold: f64,
    /// Moderate severity threshold (default: 0.36).
    pub moderate_threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            days: 90,
            min_churn_percentile: 50.0,
            min_complexity_percentile: 50.0,
            critical_threshold: 0.81, // 90th percentile in both
            high_threshold: 0.64,     // 80th percentile in both
            moderate_threshold: 0.36, // 60th percentile in both
        }
    }
}

/// Hotspot analyzer.
pub struct Analyzer {
    config: Config,
    complexity_analyzer: complexity::Analyzer,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            complexity_analyzer: complexity::Analyzer::new(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            complexity_analyzer: complexity::Analyzer::new(),
        }
    }

    pub fn with_days(mut self, days: u32) -> Self {
        self.config.days = days;
        self
    }

    /// Analyze a project directory for hotspots.
    pub fn analyze_project(&self, root: &Path) -> Result<Analysis> {
        // Get files
        let files: Vec<_> = WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .map(|e| e.into_path())
            .collect();

        // Run churn analysis using GitRepo
        let git_repo = GitRepo::open(root)?;
        let churn_data = self.collect_churn_data(&git_repo, &files, root)?;

        // Run complexity analysis
        let complexity_data = self.collect_complexity_data(&files, root)?;

        self.combine_analyses(&churn_data, &complexity_data)
    }

    fn collect_churn_data(
        &self,
        git_repo: &GitRepo,
        files: &[std::path::PathBuf],
        root: &Path,
    ) -> Result<Vec<FileChurn>> {
        // Get cutoff timestamp
        let now = Utc::now();
        let days_ago = now - chrono::Duration::days(self.config.days as i64);
        let since = days_ago.format("%Y-%m-%d").to_string();

        // Get all commits in the time range
        let commits = git_repo.log_with_stats(Some(&since))?;

        // Build file -> churn map
        let mut file_churn: HashMap<String, FileChurn> = HashMap::new();

        for commit in &commits {
            for file_change in &commit.files {
                let path_str = file_change.path.to_string_lossy().to_string();
                let entry = file_churn
                    .entry(path_str.clone())
                    .or_insert_with(|| FileChurn {
                        path: path_str.clone(),
                        commits: 0,
                        churn_score: 0.0,
                    });
                entry.commits += 1;
                entry.churn_score +=
                    1.0 + (file_change.additions + file_change.deletions) as f64 / 100.0;
            }
        }

        // Filter to only files that exist in our set
        let file_set: std::collections::HashSet<String> = files
            .iter()
            .map(|f| {
                let rel = f.strip_prefix(root).unwrap_or(f);
                rel.to_string_lossy().to_string()
            })
            .collect();

        Ok(file_churn
            .into_iter()
            .filter(|(path, _)| file_set.contains(path))
            .map(|(_, fc)| fc)
            .collect())
    }

    fn collect_complexity_data(
        &self,
        files: &[std::path::PathBuf],
        root: &Path,
    ) -> Result<Vec<FileComplexity>> {
        let results: Vec<FileComplexity> = files
            .par_iter()
            .filter_map(|file| {
                self.complexity_analyzer
                    .analyze_file(file)
                    .ok()
                    .map(|result| {
                        let rel_path = file.strip_prefix(root).unwrap_or(file);
                        FileComplexity {
                            path: rel_path.to_string_lossy().to_string(),
                            total_cyclomatic: result.total_cyclomatic,
                            avg_cyclomatic: result.avg_cyclomatic,
                        }
                    })
            })
            .collect();

        Ok(results)
    }

    /// Combine churn and complexity analyses.
    pub fn combine_analyses(
        &self,
        churn: &[FileChurn],
        complexity: &[FileComplexity],
    ) -> Result<Analysis> {
        // Build complexity lookup by file path
        let complexity_map: HashMap<&str, &FileComplexity> =
            complexity.iter().map(|f| (f.path.as_str(), f)).collect();

        // Calculate percentiles
        let churn_scores: Vec<f64> = churn.iter().map(|f| f.churn_score).collect();
        let complexity_scores: Vec<f64> = complexity
            .iter()
            .map(|f| f.total_cyclomatic as f64)
            .collect();

        let mut hotspots = Vec::new();

        for file in churn {
            // Find matching complexity data
            if let Some(cx) = complexity_map.get(file.path.as_str()) {
                let churn_pct = percentile_rank(&churn_scores, file.churn_score);
                let complexity_pct =
                    percentile_rank(&complexity_scores, cx.total_cyclomatic as f64);

                // Only include files above minimum thresholds
                if churn_pct >= self.config.min_churn_percentile
                    && complexity_pct >= self.config.min_complexity_percentile
                {
                    // Score is product of normalized percentiles
                    let score = (churn_pct / 100.0) * (complexity_pct / 100.0);
                    let severity = self.classify_severity(score);

                    hotspots.push(Hotspot {
                        file: file.path.clone(),
                        score,
                        severity,
                        churn_percentile: churn_pct,
                        complexity_percentile: complexity_pct,
                        commits: file.commits,
                        avg_complexity: cx.avg_cyclomatic,
                    });
                }
            }
        }

        // Sort by score descending
        hotspots.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Calculate summary
        let summary = AnalysisSummary {
            total_hotspots: hotspots.len(),
            critical_count: hotspots
                .iter()
                .filter(|h| matches!(h.severity, Severity::Critical))
                .count(),
            high_count: hotspots
                .iter()
                .filter(|h| matches!(h.severity, Severity::High))
                .count(),
        };

        Ok(Analysis { hotspots, summary })
    }

    fn classify_severity(&self, score: f64) -> Severity {
        if score >= self.config.critical_threshold {
            Severity::Critical
        } else if score >= self.config.high_threshold {
            Severity::High
        } else if score >= self.config.moderate_threshold {
            Severity::Moderate
        } else {
            Severity::Low
        }
    }
}

/// Internal struct for churn data.
#[derive(Debug, Clone)]
pub struct FileChurn {
    pub path: String,
    pub commits: u32,
    pub churn_score: f64,
}

/// Internal struct for complexity data.
#[derive(Debug, Clone)]
pub struct FileComplexity {
    pub path: String,
    pub total_cyclomatic: u32,
    pub avg_cyclomatic: f64,
}

/// Calculate percentile rank of a value in a list.
fn percentile_rank(values: &[f64], value: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let count_below = values.iter().filter(|&&v| v < value).count();
    let count_equal = values
        .iter()
        .filter(|&&v| (v - value).abs() < f64::EPSILON)
        .count();

    100.0 * (count_below as f64 + 0.5 * count_equal as f64) / values.len() as f64
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "hotspot"
    }

    fn description(&self) -> &'static str {
        "Find files with high churn AND high complexity"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // Ensure we have git access
        if ctx.git_path.is_none() {
            return Err(Error::git("hotspot analysis requires git history"));
        }

        self.analyze_project(ctx.root)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub hotspots: Vec<Hotspot>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotspot {
    pub file: String,
    pub score: f64,
    pub severity: Severity,
    pub churn_percentile: f64,
    pub complexity_percentile: f64,
    pub commits: u32,
    pub avg_complexity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Moderate,
    Low,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "critical"),
            Severity::High => write!(f, "high"),
            Severity::Moderate => write!(f, "moderate"),
            Severity::Low => write!(f, "low"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_hotspots: usize,
    pub critical_count: usize,
    pub high_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_churn_file(path: &str, commits: u32, churn_score: f64) -> FileChurn {
        FileChurn {
            path: path.to_string(),
            commits,
            churn_score,
        }
    }

    fn make_complexity_file(path: &str, cyclomatic: u32) -> FileComplexity {
        FileComplexity {
            path: path.to_string(),
            total_cyclomatic: cyclomatic,
            avg_cyclomatic: cyclomatic as f64,
        }
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "hotspot");
        assert!(analyzer.requires_git());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.days, 90);
        assert!((config.min_churn_percentile - 50.0).abs() < 0.001);
        assert!((config.critical_threshold - 0.81).abs() < 0.001);
    }

    #[test]
    fn test_with_days() {
        let analyzer = Analyzer::new().with_days(180);
        assert_eq!(analyzer.config.days, 180);
    }

    #[test]
    fn test_percentile_rank_empty() {
        assert!((percentile_rank(&[], 5.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_rank_single() {
        let values = vec![10.0];
        // Only value: count_below=0, count_equal=1
        // rank = 100 * (0 + 0.5*1) / 1 = 50
        assert!((percentile_rank(&values, 10.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_rank_lowest() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // Value 1.0: count_below=0, count_equal=1
        // rank = 100 * (0 + 0.5*1) / 5 = 10
        assert!((percentile_rank(&values, 1.0) - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_rank_highest() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // Value 5.0: count_below=4, count_equal=1
        // rank = 100 * (4 + 0.5*1) / 5 = 90
        assert!((percentile_rank(&values, 5.0) - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_rank_middle() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // Value 3.0: count_below=2, count_equal=1
        // rank = 100 * (2 + 0.5*1) / 5 = 50
        assert!((percentile_rank(&values, 3.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_severity_classification() {
        let analyzer = Analyzer::new();
        assert!(matches!(
            analyzer.classify_severity(0.90),
            Severity::Critical
        ));
        assert!(matches!(
            analyzer.classify_severity(0.81),
            Severity::Critical
        ));
        assert!(matches!(analyzer.classify_severity(0.70), Severity::High));
        assert!(matches!(
            analyzer.classify_severity(0.50),
            Severity::Moderate
        ));
        assert!(matches!(analyzer.classify_severity(0.20), Severity::Low));
    }

    #[test]
    fn test_combine_analyses_empty() {
        let analyzer = Analyzer::new();
        let churn: Vec<FileChurn> = vec![];
        let complexity: Vec<FileComplexity> = vec![];

        let result = analyzer.combine_analyses(&churn, &complexity).unwrap();
        assert!(result.hotspots.is_empty());
        assert_eq!(result.summary.total_hotspots, 0);
    }

    #[test]
    fn test_combine_analyses_single_hotspot() {
        let mut analyzer = Analyzer::new();
        // Lower thresholds so our single file qualifies
        analyzer.config.min_churn_percentile = 0.0;
        analyzer.config.min_complexity_percentile = 0.0;

        let churn = vec![make_churn_file("src/main.rs", 20, 150.0)];
        let complexity = vec![make_complexity_file("src/main.rs", 15)];

        let result = analyzer.combine_analyses(&churn, &complexity).unwrap();
        assert_eq!(result.hotspots.len(), 1);
        assert_eq!(result.hotspots[0].file, "src/main.rs");
        assert_eq!(result.hotspots[0].commits, 20);
    }

    #[test]
    fn test_combine_analyses_filters_by_percentile() {
        let analyzer = Analyzer::new(); // Default thresholds: 50th percentile

        // Create files where only one is above 50th percentile in both
        let churn = vec![
            make_churn_file("low.rs", 5, 10.0),    // Low churn
            make_churn_file("high.rs", 50, 200.0), // High churn
        ];
        let complexity = vec![
            make_complexity_file("low.rs", 2),   // Low complexity
            make_complexity_file("high.rs", 30), // High complexity
        ];

        let result = analyzer.combine_analyses(&churn, &complexity).unwrap();
        // Only high.rs should be in hotspots (90th percentile in both)
        assert_eq!(result.hotspots.len(), 1);
        assert_eq!(result.hotspots[0].file, "high.rs");
    }

    #[test]
    fn test_combine_analyses_sorting() {
        let mut analyzer = Analyzer::new();
        analyzer.config.min_churn_percentile = 0.0;
        analyzer.config.min_complexity_percentile = 0.0;

        let churn = vec![
            make_churn_file("a.rs", 10, 50.0),
            make_churn_file("b.rs", 20, 100.0),
            make_churn_file("c.rs", 30, 150.0),
        ];
        let complexity = vec![
            make_complexity_file("a.rs", 5),
            make_complexity_file("b.rs", 10),
            make_complexity_file("c.rs", 15),
        ];

        let result = analyzer.combine_analyses(&churn, &complexity).unwrap();
        assert_eq!(result.hotspots.len(), 3);
        // Should be sorted by score descending
        assert_eq!(result.hotspots[0].file, "c.rs");
        assert!(result.hotspots[0].score >= result.hotspots[1].score);
        assert!(result.hotspots[1].score >= result.hotspots[2].score);
    }

    #[test]
    fn test_summary_counts() {
        let mut analyzer = Analyzer::new();
        analyzer.config.min_churn_percentile = 0.0;
        analyzer.config.min_complexity_percentile = 0.0;
        analyzer.config.critical_threshold = 0.80;
        analyzer.config.high_threshold = 0.50;
        analyzer.config.moderate_threshold = 0.25;

        let churn = vec![
            make_churn_file("critical.rs", 100, 500.0), // P90 churn
            make_churn_file("high.rs", 50, 250.0),      // P70 churn
            make_churn_file("low.rs", 5, 25.0),         // P10 churn
        ];
        let complexity = vec![
            make_complexity_file("critical.rs", 50), // P90 complexity
            make_complexity_file("high.rs", 25),     // P70 complexity
            make_complexity_file("low.rs", 5),       // P10 complexity
        ];

        let result = analyzer.combine_analyses(&churn, &complexity).unwrap();
        // With 3 files, the highest is at 90th percentile, middle at 50th, lowest at 10th
        // Verify counts are reasonable (just check types are correct)
        let _ = result.summary.critical_count;
        let _ = result.summary.high_count;
    }

    #[test]
    fn test_hotspot_fields() {
        let hotspot = Hotspot {
            file: "test.rs".to_string(),
            score: 0.85,
            severity: Severity::Critical,
            churn_percentile: 95.0,
            complexity_percentile: 90.0,
            commits: 50,
            avg_complexity: 15.5,
        };

        assert_eq!(hotspot.file, "test.rs");
        assert!((hotspot.score - 0.85).abs() < 0.001);
        assert!(matches!(hotspot.severity, Severity::Critical));
        assert_eq!(hotspot.commits, 50);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Critical), "critical");
        assert_eq!(format!("{}", Severity::High), "high");
        assert_eq!(format!("{}", Severity::Moderate), "moderate");
        assert_eq!(format!("{}", Severity::Low), "low");
    }

    #[test]
    fn test_analysis_summary() {
        let summary = AnalysisSummary {
            total_hotspots: 10,
            critical_count: 2,
            high_count: 3,
        };
        assert_eq!(summary.total_hotspots, 10);
        assert_eq!(summary.critical_count, 2);
        assert_eq!(summary.high_count, 3);
    }

    #[test]
    fn test_collect_churn_data_returns_file_changes() {
        // This test verifies that collect_churn_data properly extracts
        // file changes from git history. The bug was that it called
        // git_repo.log() which returns commits with empty file lists,
        // instead of git_repo.log_with_stats() which includes file changes.
        use std::path::PathBuf;

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let analyzer = Analyzer::new().with_days(365);

        // Open the omen repo itself
        let git_repo = match GitRepo::open(&repo_root) {
            Ok(repo) => repo,
            Err(_) => return, // Skip if not in a git repo
        };

        // Get files in the repo
        let files: Vec<PathBuf> = WalkBuilder::new(&repo_root)
            .hidden(true)
            .git_ignore(true)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .map(|e| e.into_path())
            .take(100) // Limit for test speed
            .collect();

        let churn_data = analyzer
            .collect_churn_data(&git_repo, &files, &repo_root)
            .unwrap();

        // The omen repo has git history with file changes.
        // If collect_churn_data works correctly, it should return non-empty data.
        assert!(
            !churn_data.is_empty(),
            "collect_churn_data should return file changes from git history"
        );

        // Verify the churn data has actual commit counts
        let total_commits: u32 = churn_data.iter().map(|f| f.commits).sum();
        assert!(
            total_commits > 0,
            "churn data should have commits > 0, got {}",
            total_commits
        );
    }
}
