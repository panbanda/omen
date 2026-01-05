//! Code ownership and bus factor analysis.
//!
//! Analyzes git blame data to determine code ownership concentration
//! and calculate bus factor (minimum contributors needed to cover 50%
//! of the codebase).

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Result};
use crate::git::GitRepo;

/// Default threshold for considering a contributor "significant" (5%).
pub const SIGNIFICANT_CONTRIBUTOR_THRESHOLD: f64 = 5.0;

/// High concentration threshold (single owner has > 80%).
pub const HIGH_CONCENTRATION_THRESHOLD: f64 = 0.8;

/// Medium concentration threshold (> 60%).
pub const MEDIUM_CONCENTRATION_THRESHOLD: f64 = 0.6;

/// Ownership analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether to exclude trivial lines from analysis.
    pub exclude_trivial: bool,
    /// Minimum lines for a file to be analyzed.
    pub min_lines: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            exclude_trivial: true,
            min_lines: 1,
        }
    }
}

/// Ownership analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Creates a new ownership analyzer with default config.
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Creates a new analyzer with the specified config.
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Sets whether to include trivial lines.
    pub fn with_include_trivial(mut self) -> Self {
        self.config.exclude_trivial = false;
        self
    }

    /// Analyzes ownership in a repository.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let git_repo = GitRepo::open(repo_path)?;

        // Collect all files
        let files = self.collect_files(repo_path)?;

        // Analyze each file
        let mut file_ownerships = Vec::new();
        let mut all_contributors: HashMap<String, u32> = HashMap::new();

        for file in &files {
            if let Ok(Some(ownership)) = self.analyze_file(&git_repo, file, repo_path) {
                // Aggregate contributor lines across all files
                for contributor in &ownership.contributors {
                    *all_contributors
                        .entry(contributor.name.clone())
                        .or_insert(0) += contributor.lines_owned;
                }
                file_ownerships.push(ownership);
            }
        }

        // Sort by concentration (highest first - most risky)
        file_ownerships.sort_by(|a, b| {
            b.concentration
                .partial_cmp(&a.concentration)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let summary = calculate_summary(&file_ownerships, &all_contributors);

        Ok(Analysis {
            generated_at: Utc::now().to_rfc3339(),
            files: file_ownerships,
            summary,
        })
    }

    /// Collects all files in a repository.
    fn collect_files(&self, root: &Path) -> Result<Vec<std::path::PathBuf>> {
        let mut files = Vec::new();
        for entry in WalkBuilder::new(root).hidden(false).build() {
            let entry = entry.map_err(|e| Error::analysis(e.to_string()))?;
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                files.push(entry.into_path());
            }
        }
        Ok(files)
    }

    /// Analyzes ownership for a single file.
    fn analyze_file(
        &self,
        git_repo: &GitRepo,
        file: &Path,
        _root: &Path,
    ) -> Result<Option<FileOwnership>> {
        let blame_info = match git_repo.blame(file) {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };

        // Filter trivial lines if configured
        let lines: Vec<_> = if self.config.exclude_trivial {
            blame_info
                .lines
                .iter()
                .filter(|_line| {
                    // TODO: Filter trivial lines based on content
                    // For now, include all lines
                    true
                })
                .collect()
        } else {
            blame_info.lines.iter().collect()
        };

        let total_lines = lines.len();
        if total_lines < self.config.min_lines {
            return Ok(None);
        }

        // Build contributors from blame info
        let contributors: Vec<Contributor> = blame_info
            .authors
            .iter()
            .map(|(name, stats)| Contributor {
                name: name.clone(),
                email: String::new(), // Not available in current BlameInfo
                lines_owned: stats.lines,
                percentage: stats.percentage,
            })
            .collect();

        // Handle empty contributors (no blame data)
        if contributors.is_empty() {
            return Ok(None);
        }

        // Sort by lines owned (descending)
        let mut sorted_contributors = contributors.clone();
        sorted_contributors.sort_by(|a, b| b.lines_owned.cmp(&a.lines_owned));

        let primary_owner = sorted_contributors
            .first()
            .map(|c| c.name.clone())
            .unwrap_or_default();

        let ownership_percent = sorted_contributors
            .first()
            .map(|c| c.percentage)
            .unwrap_or(0.0);

        let concentration = calculate_concentration(&sorted_contributors);
        let is_silo = sorted_contributors.len() == 1;

        let risk_level = classify_risk(concentration, sorted_contributors.len());

        Ok(Some(FileOwnership {
            path: file.to_string_lossy().to_string(),
            primary_owner,
            ownership_percent,
            concentration,
            total_lines: total_lines as u32,
            contributors: sorted_contributors,
            is_silo,
            risk_level,
        }))
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "ownership"
    }

    fn description(&self) -> &'static str {
        "Analyze knowledge concentration and team risk"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let git_path = ctx.git_path.as_ref().ok_or_else(|| {
            Error::git("Ownership analysis requires git history")
        })?;

        self.analyze_repo(git_path)
    }
}

/// Calculates ownership concentration (0-1).
/// Uses the primary owner's percentage as concentration.
pub fn calculate_concentration(contributors: &[Contributor]) -> f64 {
    if contributors.is_empty() {
        return 0.0;
    }
    if contributors.len() == 1 {
        return 1.0; // Single owner = maximum concentration
    }

    // Find max percentage
    let max_pct = contributors
        .iter()
        .map(|c| c.percentage)
        .fold(0.0, f64::max);

    max_pct / 100.0
}

/// Classifies risk level based on concentration and contributor count.
fn classify_risk(concentration: f64, contributor_count: usize) -> RiskLevel {
    // High risk: single contributor OR very high concentration
    if contributor_count == 1 || concentration >= HIGH_CONCENTRATION_THRESHOLD {
        RiskLevel::High
    // Medium risk: moderate concentration OR very few contributors
    } else if concentration >= MEDIUM_CONCENTRATION_THRESHOLD || contributor_count <= 2 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

/// Calculates bus factor - minimum contributors needed to cover 50% of codebase.
fn calculate_bus_factor(contributor_lines: &HashMap<String, u32>) -> usize {
    if contributor_lines.is_empty() {
        return 0;
    }

    let total: u32 = contributor_lines.values().sum();
    if total == 0 {
        return 0;
    }

    // Sort contributors by lines (descending)
    let mut sorted: Vec<_> = contributor_lines.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    // Count how many needed to reach 50%
    let threshold = total / 2;
    let mut accumulated = 0u32;
    for (i, (_, lines)) in sorted.iter().enumerate() {
        accumulated += *lines;
        if accumulated >= threshold {
            return i + 1;
        }
    }

    sorted.len()
}

/// Gets top N contributors by total lines.
fn get_top_contributors(contributor_lines: &HashMap<String, u32>, n: usize) -> Vec<String> {
    let mut sorted: Vec<_> = contributor_lines.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    sorted.into_iter().take(n).map(|(name, _)| name.clone()).collect()
}

/// Calculates summary statistics.
fn calculate_summary(
    files: &[FileOwnership],
    all_contributors: &HashMap<String, u32>,
) -> Summary {
    if files.is_empty() {
        return Summary::default();
    }

    let total_files = files.len();
    let silo_count = files.iter().filter(|f| f.is_silo).count();
    let high_risk_count = files
        .iter()
        .filter(|f| matches!(f.risk_level, RiskLevel::High))
        .count();

    let total_contributors: usize = files.iter().map(|f| f.contributors.len()).sum();
    let avg_contributors = total_contributors as f64 / files.len() as f64;

    let max_concentration = files
        .iter()
        .map(|f| f.concentration)
        .fold(0.0, f64::max);

    let bus_factor = calculate_bus_factor(all_contributors);
    let top_contributors = get_top_contributors(all_contributors, 5);

    Summary {
        total_files,
        bus_factor,
        silo_count,
        high_risk_count,
        avg_contributors,
        max_concentration,
        top_contributors,
    }
}

/// Ownership analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// When the analysis was generated.
    pub generated_at: String,
    /// Per-file ownership data.
    pub files: Vec<FileOwnership>,
    /// Summary statistics.
    pub summary: Summary,
}

/// Ownership metrics for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOwnership {
    /// File path.
    pub path: String,
    /// Primary owner (highest contributor).
    pub primary_owner: String,
    /// Ownership percentage of primary owner (0-100).
    pub ownership_percent: f64,
    /// Concentration score (0-1, higher = more concentrated).
    pub concentration: f64,
    /// Total lines in file.
    pub total_lines: u32,
    /// Contributors to the file.
    pub contributors: Vec<Contributor>,
    /// Whether file has only one contributor.
    pub is_silo: bool,
    /// Risk level based on concentration.
    pub risk_level: RiskLevel,
}

/// A contributor to a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    /// Contributor name.
    pub name: String,
    /// Contributor email.
    pub email: String,
    /// Lines owned by this contributor.
    pub lines_owned: u32,
    /// Percentage of file owned (0-100).
    pub percentage: f64,
}

/// Risk level for a file based on ownership concentration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// High risk: single owner or >80% concentration.
    High,
    /// Medium risk: >60% concentration or <=2 contributors.
    Medium,
    /// Low risk: well-distributed ownership.
    Low,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::High => write!(f, "high"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::Low => write!(f, "low"),
        }
    }
}

/// Aggregate statistics for ownership analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    /// Total files analyzed.
    pub total_files: usize,
    /// Bus factor (contributors needed for 50% coverage).
    pub bus_factor: usize,
    /// Number of files with single contributor.
    pub silo_count: usize,
    /// Number of high-risk files.
    pub high_risk_count: usize,
    /// Average contributors per file.
    pub avg_contributors: f64,
    /// Maximum concentration across all files.
    pub max_concentration: f64,
    /// Top contributors by total lines.
    pub top_contributors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.exclude_trivial);
        assert_eq!(config.min_lines, 1);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert!(analyzer.config.exclude_trivial);
    }

    #[test]
    fn test_analyzer_with_include_trivial() {
        let analyzer = Analyzer::new().with_include_trivial();
        assert!(!analyzer.config.exclude_trivial);
    }

    #[test]
    fn test_concentration_empty() {
        assert_eq!(calculate_concentration(&[]), 0.0);
    }

    #[test]
    fn test_concentration_single() {
        let contributors = vec![Contributor {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            lines_owned: 100,
            percentage: 100.0,
        }];
        assert_eq!(calculate_concentration(&contributors), 1.0);
    }

    #[test]
    fn test_concentration_multiple() {
        let contributors = vec![
            Contributor {
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
                lines_owned: 80,
                percentage: 80.0,
            },
            Contributor {
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
                lines_owned: 20,
                percentage: 20.0,
            },
        ];
        assert!((calculate_concentration(&contributors) - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_concentration_even_split() {
        let contributors = vec![
            Contributor {
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
                lines_owned: 50,
                percentage: 50.0,
            },
            Contributor {
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
                lines_owned: 50,
                percentage: 50.0,
            },
        ];
        assert!((calculate_concentration(&contributors) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_risk_level_single_contributor() {
        let risk = classify_risk(1.0, 1);
        assert_eq!(risk, RiskLevel::High);
    }

    #[test]
    fn test_risk_level_high_concentration() {
        let risk = classify_risk(0.85, 3);
        assert_eq!(risk, RiskLevel::High);
    }

    #[test]
    fn test_risk_level_medium_concentration() {
        let risk = classify_risk(0.65, 3);
        assert_eq!(risk, RiskLevel::Medium);
    }

    #[test]
    fn test_risk_level_few_contributors() {
        let risk = classify_risk(0.5, 2);
        assert_eq!(risk, RiskLevel::Medium);
    }

    #[test]
    fn test_risk_level_low() {
        let risk = classify_risk(0.4, 5);
        assert_eq!(risk, RiskLevel::Low);
    }

    #[test]
    fn test_bus_factor_empty() {
        let contributors: HashMap<String, u32> = HashMap::new();
        assert_eq!(calculate_bus_factor(&contributors), 0);
    }

    #[test]
    fn test_bus_factor_single() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 100);
        assert_eq!(calculate_bus_factor(&contributors), 1);
    }

    #[test]
    fn test_bus_factor_even_split() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 50);
        contributors.insert("Bob".to_string(), 50);
        // 50% threshold requires one person
        assert_eq!(calculate_bus_factor(&contributors), 1);
    }

    #[test]
    fn test_bus_factor_three_way() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 40);
        contributors.insert("Bob".to_string(), 35);
        contributors.insert("Carol".to_string(), 25);
        // 50 lines needed, Alice (40) + Bob (35) covers it
        assert_eq!(calculate_bus_factor(&contributors), 2);
    }

    #[test]
    fn test_bus_factor_dominant() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 90);
        contributors.insert("Bob".to_string(), 5);
        contributors.insert("Carol".to_string(), 5);
        // Alice alone covers 50%
        assert_eq!(calculate_bus_factor(&contributors), 1);
    }

    #[test]
    fn test_get_top_contributors() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 100);
        contributors.insert("Bob".to_string(), 50);
        contributors.insert("Carol".to_string(), 25);
        contributors.insert("Dave".to_string(), 10);

        let top = get_top_contributors(&contributors, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], "Alice");
        assert_eq!(top[1], "Bob");
    }

    #[test]
    fn test_get_top_contributors_less_than_n() {
        let mut contributors = HashMap::new();
        contributors.insert("Alice".to_string(), 100);

        let top = get_top_contributors(&contributors, 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0], "Alice");
    }

    #[test]
    fn test_summary_empty() {
        let summary = calculate_summary(&[], &HashMap::new());
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.bus_factor, 0);
        assert_eq!(summary.silo_count, 0);
    }

    #[test]
    fn test_summary_with_files() {
        let files = vec![
            FileOwnership {
                path: "a.rs".to_string(),
                primary_owner: "Alice".to_string(),
                ownership_percent: 80.0,
                concentration: 0.8,
                total_lines: 100,
                contributors: vec![
                    Contributor {
                        name: "Alice".to_string(),
                        email: "".to_string(),
                        lines_owned: 80,
                        percentage: 80.0,
                    },
                    Contributor {
                        name: "Bob".to_string(),
                        email: "".to_string(),
                        lines_owned: 20,
                        percentage: 20.0,
                    },
                ],
                is_silo: false,
                risk_level: RiskLevel::High,
            },
            FileOwnership {
                path: "b.rs".to_string(),
                primary_owner: "Carol".to_string(),
                ownership_percent: 100.0,
                concentration: 1.0,
                total_lines: 50,
                contributors: vec![Contributor {
                    name: "Carol".to_string(),
                    email: "".to_string(),
                    lines_owned: 50,
                    percentage: 100.0,
                }],
                is_silo: true,
                risk_level: RiskLevel::High,
            },
        ];

        let mut all_contributors = HashMap::new();
        all_contributors.insert("Alice".to_string(), 80);
        all_contributors.insert("Bob".to_string(), 20);
        all_contributors.insert("Carol".to_string(), 50);

        let summary = calculate_summary(&files, &all_contributors);
        assert_eq!(summary.total_files, 2);
        assert_eq!(summary.silo_count, 1);
        assert_eq!(summary.high_risk_count, 2);
        assert!((summary.avg_contributors - 1.5).abs() < 0.001);
        assert!((summary.max_concentration - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_risk_level_display() {
        assert_eq!(format!("{}", RiskLevel::High), "high");
        assert_eq!(format!("{}", RiskLevel::Medium), "medium");
        assert_eq!(format!("{}", RiskLevel::Low), "low");
    }

    #[test]
    fn test_file_ownership_serialization() {
        let ownership = FileOwnership {
            path: "test.rs".to_string(),
            primary_owner: "Alice".to_string(),
            ownership_percent: 75.0,
            concentration: 0.75,
            total_lines: 100,
            contributors: vec![Contributor {
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
                lines_owned: 75,
                percentage: 75.0,
            }],
            is_silo: false,
            risk_level: RiskLevel::Medium,
        };

        let json = serde_json::to_string(&ownership).unwrap();
        assert!(json.contains("\"path\":\"test.rs\""));
        assert!(json.contains("\"concentration\":0.75"));
        assert!(json.contains("\"risk_level\":\"Medium\""));
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "ownership");
        assert!(analyzer.description().contains("knowledge"));
        assert!(analyzer.requires_git());
    }

    #[test]
    fn test_contributor_fields() {
        let contributor = Contributor {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            lines_owned: 150,
            percentage: 60.0,
        };
        assert_eq!(contributor.name, "Alice");
        assert_eq!(contributor.email, "alice@example.com");
        assert_eq!(contributor.lines_owned, 150);
        assert!((contributor.percentage - 60.0).abs() < 0.001);
    }
}
