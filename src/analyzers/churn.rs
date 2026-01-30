//! Git churn analyzer - file change frequency and hotspot detection.
//!
//! Analyzes git history to identify files with high change frequency (churn),
//! which often correlates with bug-prone or complex code. omen:ignore
//!
//! # References
//!
//! - Nagappan, N., Ball, T. (2005) "Use of Relative Code Churn Measures to
//!   Predict System Defect Density" ICSE 2005 (foundational churn research)
//!
//! # Scoring
//!
//! Churn score = commit_factor * 0.6 + change_factor * 0.4 (normalized to 0-1)
//! These weights are heuristics that prioritize commit frequency over raw
//! line changes, as frequent small changes often indicate instability.
//! The original Nagappan & Ball research uses relative churn (churn / LOC)
//! rather than absolute weights.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

#[cfg(test)]
use std::io::{BufRead, BufReader};

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Result};
use crate::git::GitRepo;

/// Churn analyzer using git log.
pub struct Analyzer {
    /// Number of days of history to analyze.
    days: u32,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new churn analyzer with default 30-day window.
    pub fn new() -> Self {
        Self { days: 30 }
    }

    /// Set the number of days to analyze.
    pub fn with_days(mut self, days: u32) -> Self {
        self.days = days;
        self
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "churn"
    }

    fn description(&self) -> &'static str {
        "Analyze git history for file churn and change frequency"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        let git_path = ctx.git_path.unwrap_or(ctx.root);
        let repo_root = git_path
            .to_str()
            .ok_or_else(|| Error::git("Invalid repository path"))?;

        // Open repository with gix
        let repo = GitRepo::open(git_path)?;

        // Calculate since date (u32::MAX means "all history" -- no time limit)
        let since = if self.days == u32::MAX {
            None
        } else {
            Some(format!("{} days", self.days))
        };

        // Get commits with file changes using gix
        let commits = repo.log_with_stats(since.as_deref(), None)?;

        // Convert to file metrics
        let file_metrics = commits_to_file_metrics(&commits);

        // Build analysis from metrics
        let analysis = build_analysis(file_metrics, repo_root, self.days);

        tracing::info!(
            "Churn analysis completed in {:?}: {} files",
            start.elapsed(),
            analysis.files.len()
        );

        Ok(analysis)
    }
}

/// Convert commits to file metrics map.
fn commits_to_file_metrics(commits: &[crate::git::Commit]) -> HashMap<String, FileMetrics> {
    let mut file_metrics: HashMap<String, FileMetrics> = HashMap::new();

    for commit in commits {
        let author = commit.author.clone();
        let timestamp = Utc.timestamp_opt(commit.timestamp, 0).single();

        for file_change in &commit.files {
            let path_str = file_change.path.to_string_lossy().to_string();

            let fm = file_metrics
                .entry(path_str.clone())
                .or_insert_with(|| FileMetrics {
                    path: format!("./{path_str}"),
                    relative_path: path_str.clone(),
                    commits: 0,
                    unique_authors: Vec::new(),
                    author_counts: HashMap::new(),
                    lines_added: 0,
                    lines_deleted: 0,
                    churn_score: 0.0,
                    first_commit: timestamp,
                    last_commit: timestamp,
                    total_loc: 0,
                    relative_churn: 0.0,
                    churn_rate: 0.0,
                    change_frequency: 0.0,
                    days_active: 0,
                });

            fm.commits += 1;
            *fm.author_counts.entry(author.clone()).or_insert(0) += 1;
            fm.lines_added += file_change.additions;
            fm.lines_deleted += file_change.deletions;

            // Update time range
            if let Some(t) = timestamp {
                match fm.first_commit {
                    Some(first) if t < first => fm.first_commit = Some(t),
                    None => fm.first_commit = Some(t),
                    _ => {}
                }
                match fm.last_commit {
                    Some(last) if t > last => fm.last_commit = Some(t),
                    None => fm.last_commit = Some(t),
                    _ => {}
                }
            }
        }
    }

    file_metrics
}

/// Parse git log --numstat output (kept for tests).
#[cfg(test)]
fn parse_git_log_numstat(output: &[u8]) -> Result<HashMap<String, FileMetrics>> {
    let mut file_metrics: HashMap<String, FileMetrics> = HashMap::new();
    let reader = BufReader::new(output);

    let mut current_author = String::new();
    let mut current_time: Option<DateTime<Utc>> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| Error::git(format!("Failed to read git output: {e}")))?;

        if line.is_empty() {
            continue;
        }

        // Check if this is a commit line (contains |)
        if line.contains('|') {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() == 3 {
                // parts[0] is hash (unused)
                current_author = parts[1].to_string();
                current_time = DateTime::parse_from_rfc3339(parts[2])
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc));
                continue;
            }
        }

        // This is a numstat line: added\tdeleted\tfilepath
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }

        let (added_str, deleted_str, relative_path) = (parts[0], parts[1], parts[2]);

        // Skip binary files (shown as "-")
        if added_str == "-" || deleted_str == "-" {
            continue;
        }

        let added: u32 = added_str.parse().unwrap_or(0);
        let deleted: u32 = deleted_str.parse().unwrap_or(0);

        let fm = file_metrics
            .entry(relative_path.to_string())
            .or_insert_with(|| FileMetrics {
                path: format!("./{relative_path}"),
                relative_path: relative_path.to_string(),
                commits: 0,
                unique_authors: Vec::new(),
                author_counts: HashMap::new(),
                lines_added: 0,
                lines_deleted: 0,
                churn_score: 0.0,
                first_commit: current_time,
                last_commit: current_time,
                total_loc: 0,
                relative_churn: 0.0,
                churn_rate: 0.0,
                change_frequency: 0.0,
                days_active: 0,
            });

        fm.commits += 1;
        *fm.author_counts.entry(current_author.clone()).or_insert(0) += 1;
        fm.lines_added += added;
        fm.lines_deleted += deleted;

        // Update time range
        if let Some(t) = current_time {
            match fm.first_commit {
                Some(first) if t < first => fm.first_commit = Some(t),
                None => fm.first_commit = Some(t),
                _ => {}
            }
            match fm.last_commit {
                Some(last) if t > last => fm.last_commit = Some(t),
                None => fm.last_commit = Some(t),
                _ => {}
            }
        }
    }

    Ok(file_metrics)
}

/// Build analysis from collected file metrics.
fn build_analysis(
    mut file_metrics: HashMap<String, FileMetrics>,
    repo_root: &str,
    days: u32,
) -> Analysis {
    // Find max values for normalization
    let mut max_commits = 0u32;
    let mut max_changes = 0u32;

    for fm in file_metrics.values() {
        if fm.commits > max_commits {
            max_commits = fm.commits;
        }
        let changes = fm.lines_added + fm.lines_deleted;
        if changes > max_changes {
            max_changes = changes;
        }
    }

    // Calculate scores and collect stats
    let mut total_commits = 0u32;
    let mut total_added = 0u32;
    let mut total_deleted = 0u32;
    let mut author_contributions: HashMap<String, u32> = HashMap::new();

    let now = Utc::now();
    let mut files: Vec<FileMetrics> = Vec::with_capacity(file_metrics.len());

    for (_, fm) in file_metrics.iter_mut() {
        // Populate unique authors
        fm.unique_authors = fm.author_counts.keys().cloned().collect();

        // Calculate churn score
        calculate_churn_score(fm, max_commits, max_changes);

        // Calculate relative churn metrics
        calculate_relative_churn(fm, repo_root, now);

        // Accumulate stats
        total_commits += fm.commits;
        total_added += fm.lines_added;
        total_deleted += fm.lines_deleted;

        for (author, count) in &fm.author_counts {
            *author_contributions.entry(author.clone()).or_insert(0) += count;
        }

        files.push(fm.clone());
    }

    // Sort by churn score descending
    files.sort_by(|a, b| {
        b.churn_score
            .partial_cmp(&a.churn_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build summary
    let mut summary = Summary {
        total_file_changes: total_commits as usize,
        total_files_changed: files.len(),
        total_additions: total_added as usize,
        total_deletions: total_deleted as usize,
        avg_commits_per_file: if files.is_empty() {
            0.0
        } else {
            total_commits as f64 / files.len() as f64
        },
        max_churn_score: files.first().map(|f| f.churn_score).unwrap_or(0.0),
        mean_churn_score: 0.0,
        variance_churn_score: 0.0,
        stddev_churn_score: 0.0,
        p50_churn_score: 0.0,
        p95_churn_score: 0.0,
        hotspot_files: Vec::new(),
        stable_files: Vec::new(),
        author_contributions,
    };

    calculate_statistics(&mut summary, &files);
    identify_hotspot_and_stable(&mut summary, &files);

    Analysis {
        generated_at: now,
        period_days: days,
        repository_root: repo_root.to_string(),
        files,
        summary,
    }
}

/// Calculate churn score for a file.
fn calculate_churn_score(fm: &mut FileMetrics, max_commits: u32, max_changes: u32) {
    let commit_factor = if max_commits > 0 {
        (fm.commits as f64 / max_commits as f64).min(1.0)
    } else {
        0.0
    };

    let change_factor = if max_changes > 0 {
        ((fm.lines_added + fm.lines_deleted) as f64 / max_changes as f64).min(1.0)
    } else {
        0.0
    };

    fm.churn_score = (commit_factor * 0.6 + change_factor * 0.4).min(1.0);
}

/// Calculate relative churn metrics.
fn calculate_relative_churn(fm: &mut FileMetrics, repo_root: &str, _now: DateTime<Utc>) {
    // Calculate days active
    if let (Some(first), Some(last)) = (fm.first_commit, fm.last_commit) {
        let days = (last - first).num_days().max(1) as u32;
        fm.days_active = days;
    }

    // Read file LOC
    let file_path = Path::new(repo_root).join(&fm.relative_path);
    if let Ok(content) = std::fs::read_to_string(&file_path) {
        fm.total_loc = content.lines().count() as u32;
    }

    // Calculate relative churn: (added + deleted) / total_loc
    if fm.total_loc > 0 {
        fm.relative_churn = (fm.lines_added + fm.lines_deleted) as f64 / fm.total_loc as f64;
    }

    // Calculate churn rate: relative_churn / days_active
    if fm.days_active > 0 {
        fm.churn_rate = fm.relative_churn / fm.days_active as f64;
        fm.change_frequency = fm.commits as f64 / fm.days_active as f64;
    }
}

/// Calculate summary statistics.
fn calculate_statistics(summary: &mut Summary, files: &[FileMetrics]) {
    if files.is_empty() {
        return;
    }

    // Mean
    let sum: f64 = files.iter().map(|f| f.churn_score).sum();
    summary.mean_churn_score = sum / files.len() as f64;

    // Variance
    let variance_sum: f64 = files
        .iter()
        .map(|f| {
            let diff = f.churn_score - summary.mean_churn_score;
            diff * diff
        })
        .sum();
    summary.variance_churn_score = variance_sum / files.len() as f64;
    summary.stddev_churn_score = summary.variance_churn_score.sqrt();

    // Percentiles
    let mut scores: Vec<f64> = files.iter().map(|f| f.churn_score).collect();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    summary.p50_churn_score = percentile(&scores, 50);
    summary.p95_churn_score = percentile(&scores, 95);
}

/// Calculate percentile from sorted slice.
fn percentile(sorted: &[f64], p: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * sorted.len()) / 100;
    sorted[idx.min(sorted.len() - 1)]
}

/// Identify hotspot and stable files.
fn identify_hotspot_and_stable(summary: &mut Summary, files: &[FileMetrics]) {
    const HOTSPOT_THRESHOLD: f64 = 0.5;
    const STABLE_THRESHOLD: f64 = 0.1;

    // Top 10 files with churn > 0.5
    let candidate_count = 10.min(files.len());
    for file in files.iter().take(candidate_count) {
        if file.churn_score > HOTSPOT_THRESHOLD {
            summary.hotspot_files.push(file.path.clone());
        }
    }

    // Bottom 10 files with churn < 0.1 and commits > 0
    let start_idx = files.len().saturating_sub(10);
    for file in files.iter().skip(start_idx).rev() {
        if file.churn_score < STABLE_THRESHOLD && file.commits > 0 {
            summary.stable_files.push(file.path.clone());
        }
    }
}

/// Churn metrics for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetrics {
    pub path: String,
    pub relative_path: String,
    pub commits: u32,
    pub unique_authors: Vec<String>,
    #[serde(skip)]
    pub author_counts: HashMap<String, u32>,
    #[serde(rename = "additions")]
    pub lines_added: u32,
    #[serde(rename = "deletions")]
    pub lines_deleted: u32,
    pub churn_score: f64,
    pub first_commit: Option<DateTime<Utc>>,
    pub last_commit: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub total_loc: u32,
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub relative_churn: f64,
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub churn_rate: f64,
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub change_frequency: f64,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub days_active: u32,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// Summary statistics for churn analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_file_changes: usize,
    pub total_files_changed: usize,
    pub total_additions: usize,
    pub total_deletions: usize,
    pub avg_commits_per_file: f64,
    pub max_churn_score: f64,
    pub mean_churn_score: f64,
    pub variance_churn_score: f64,
    pub stddev_churn_score: f64,
    pub p50_churn_score: f64,
    pub p95_churn_score: f64,
    pub hotspot_files: Vec<String>,
    pub stable_files: Vec<String>,
    pub author_contributions: HashMap<String, u32>,
}

/// Full churn analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: DateTime<Utc>,
    pub period_days: u32,
    pub repository_root: String,
    pub files: Vec<FileMetrics>,
    pub summary: Summary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "churn");
        assert_eq!(analyzer.days, 30);
    }

    #[test]
    fn test_analyzer_with_days() {
        let analyzer = Analyzer::new().with_days(90);
        assert_eq!(analyzer.days, 90);
    }

    #[test]
    fn test_churn_score_calculation() {
        let mut fm = FileMetrics {
            path: "./test.go".to_string(),
            relative_path: "test.go".to_string(),
            commits: 10,
            unique_authors: vec![],
            author_counts: HashMap::new(),
            lines_added: 100,
            lines_deleted: 50,
            churn_score: 0.0,
            first_commit: None,
            last_commit: None,
            total_loc: 0,
            relative_churn: 0.0,
            churn_rate: 0.0,
            change_frequency: 0.0,
            days_active: 0,
        };

        calculate_churn_score(&mut fm, 10, 150);
        // commit_factor = 10/10 = 1.0, change_factor = 150/150 = 1.0
        // score = 1.0 * 0.6 + 1.0 * 0.4 = 1.0
        assert!((fm.churn_score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile() {
        let sorted = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&sorted, 50) - 6.0).abs() < 0.001);
        assert!((percentile(&sorted, 90) - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_empty() {
        let sorted: Vec<f64> = vec![];
        assert_eq!(percentile(&sorted, 50), 0.0);
    }

    #[test]
    fn test_parse_git_log_numstat() {
        let output = b"abc123|John Doe|2024-01-15T10:00:00+00:00\n\
                       10\t5\tsrc/main.go\n\
                       20\t3\tsrc/lib.go\n";

        let metrics = parse_git_log_numstat(output).unwrap();
        assert_eq!(metrics.len(), 2);
        assert!(metrics.contains_key("src/main.go"));
        assert!(metrics.contains_key("src/lib.go"));

        let main = &metrics["src/main.go"];
        assert_eq!(main.commits, 1);
        assert_eq!(main.lines_added, 10);
        assert_eq!(main.lines_deleted, 5);
    }

    /// Helper to construct a FileMetrics with sensible defaults.
    fn make_file_metrics(
        path: &str,
        commits: u32,
        added: u32,
        deleted: u32,
        churn_score: f64,
    ) -> FileMetrics {
        FileMetrics {
            path: format!("./{path}"),
            relative_path: path.to_string(),
            commits,
            unique_authors: vec![],
            author_counts: HashMap::new(),
            lines_added: added,
            lines_deleted: deleted,
            churn_score,
            first_commit: None,
            last_commit: None,
            total_loc: 0,
            relative_churn: 0.0,
            churn_rate: 0.0,
            change_frequency: 0.0,
            days_active: 0,
        }
    }

    // --- build_analysis tests ---

    #[test]
    fn test_build_analysis_empty() {
        let file_metrics: HashMap<String, FileMetrics> = HashMap::new();
        let analysis = build_analysis(file_metrics, "/tmp/fake", 30);

        assert!(analysis.files.is_empty());
        assert_eq!(analysis.summary.total_files_changed, 0);
        assert_eq!(analysis.summary.total_file_changes, 0);
        assert_eq!(analysis.summary.total_additions, 0);
        assert_eq!(analysis.summary.total_deletions, 0);
        assert_eq!(analysis.summary.avg_commits_per_file, 0.0);
        assert_eq!(analysis.summary.max_churn_score, 0.0);
        assert_eq!(analysis.summary.mean_churn_score, 0.0);
        assert!(analysis.summary.hotspot_files.is_empty());
        assert!(analysis.summary.stable_files.is_empty());
    }

    #[test]
    fn test_build_analysis_single_file() {
        let mut metrics = HashMap::new();
        let mut fm = make_file_metrics("src/main.rs", 5, 100, 20, 0.0);
        fm.author_counts.insert("Alice".to_string(), 3);
        fm.author_counts.insert("Bob".to_string(), 2);
        metrics.insert("src/main.rs".to_string(), fm);

        let analysis = build_analysis(metrics, "/tmp/fake", 30);

        assert_eq!(analysis.files.len(), 1);
        assert_eq!(analysis.summary.total_files_changed, 1);
        assert_eq!(analysis.summary.total_file_changes, 5);
        assert_eq!(analysis.summary.total_additions, 100);
        assert_eq!(analysis.summary.total_deletions, 20);
        assert_eq!(analysis.summary.avg_commits_per_file, 5.0);

        // Single file with max commits and max changes: score = 1.0
        let file = &analysis.files[0];
        assert!((file.churn_score - 1.0).abs() < 0.001);

        // unique_authors should be populated
        assert_eq!(file.unique_authors.len(), 2);

        // author_contributions should aggregate both authors
        assert_eq!(analysis.summary.author_contributions.len(), 2);
    }

    #[test]
    fn test_build_analysis_multiple_files() {
        let mut metrics = HashMap::new();

        // High-churn file
        let mut high = make_file_metrics("hot.rs", 20, 500, 200, 0.0);
        high.author_counts.insert("Alice".to_string(), 20);
        metrics.insert("hot.rs".to_string(), high);

        // Medium-churn file
        let mut mid = make_file_metrics("mid.rs", 10, 100, 50, 0.0);
        mid.author_counts.insert("Bob".to_string(), 10);
        metrics.insert("mid.rs".to_string(), mid);

        // Low-churn file
        let mut low = make_file_metrics("cold.rs", 1, 5, 2, 0.0);
        low.author_counts.insert("Carol".to_string(), 1);
        metrics.insert("cold.rs".to_string(), low);

        let analysis = build_analysis(metrics, "/tmp/fake", 30);

        assert_eq!(analysis.files.len(), 3);

        // Files should be sorted by churn_score descending
        assert!(analysis.files[0].churn_score >= analysis.files[1].churn_score);
        assert!(analysis.files[1].churn_score >= analysis.files[2].churn_score);

        // The highest-churn file should be first
        assert_eq!(analysis.files[0].relative_path, "hot.rs");

        // Percentiles should be computed (p50 and p95 set)
        // With 3 files sorted ascending, p50 index = (50*3)/100 = 1
        assert!(analysis.summary.p50_churn_score > 0.0);
    }

    // --- calculate_statistics tests ---

    #[test]
    fn test_calculate_statistics_basic() {
        let mut summary = Summary::default();
        let files = vec![
            make_file_metrics("a.rs", 0, 0, 0, 0.2),
            make_file_metrics("b.rs", 0, 0, 0, 0.4),
            make_file_metrics("c.rs", 0, 0, 0, 0.6),
            make_file_metrics("d.rs", 0, 0, 0, 0.8),
        ];

        calculate_statistics(&mut summary, &files);

        // Mean = (0.2 + 0.4 + 0.6 + 0.8) / 4 = 0.5
        assert!((summary.mean_churn_score - 0.5).abs() < 1e-9);

        // Variance = ((0.3^2 + 0.1^2 + 0.1^2 + 0.3^2) / 4) = 0.05
        assert!((summary.variance_churn_score - 0.05).abs() < 1e-9);

        // Stddev = sqrt(0.05)
        assert!((summary.stddev_churn_score - 0.05_f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn test_calculate_statistics_single_value() {
        let mut summary = Summary::default();
        let files = vec![make_file_metrics("a.rs", 0, 0, 0, 0.7)];

        calculate_statistics(&mut summary, &files);

        assert!((summary.mean_churn_score - 0.7).abs() < 1e-9);
        assert!((summary.variance_churn_score).abs() < 1e-9);
        assert!((summary.stddev_churn_score).abs() < 1e-9);
    }

    #[test]
    fn test_calculate_statistics_empty() {
        let mut summary = Summary::default();
        let files: Vec<FileMetrics> = vec![];

        calculate_statistics(&mut summary, &files);

        assert_eq!(summary.mean_churn_score, 0.0);
        assert_eq!(summary.variance_churn_score, 0.0);
        assert_eq!(summary.stddev_churn_score, 0.0);
        assert_eq!(summary.p50_churn_score, 0.0);
        assert_eq!(summary.p95_churn_score, 0.0);
    }

    // --- identify_hotspot_and_stable tests ---

    #[test]
    fn test_identify_hotspots() {
        let mut summary = Summary::default();

        // Files sorted descending by churn_score (as build_analysis would produce)
        let files = vec![
            make_file_metrics("a.rs", 1, 0, 0, 0.9),
            make_file_metrics("b.rs", 1, 0, 0, 0.7),
            make_file_metrics("c.rs", 1, 0, 0, 0.3),
            make_file_metrics("d.rs", 1, 0, 0, 0.05),
        ];

        identify_hotspot_and_stable(&mut summary, &files);

        // Hotspot threshold is > 0.5, so a.rs and b.rs qualify
        assert_eq!(summary.hotspot_files.len(), 2);
        assert!(summary.hotspot_files.contains(&"./a.rs".to_string()));
        assert!(summary.hotspot_files.contains(&"./b.rs".to_string()));
    }

    #[test]
    fn test_identify_stable_files() {
        let mut summary = Summary::default();

        // Sorted descending. Stable = bottom 10 with score < 0.1 and commits > 0.
        let files = vec![
            make_file_metrics("a.rs", 1, 0, 0, 0.9),
            make_file_metrics("b.rs", 1, 0, 0, 0.5),
            make_file_metrics("c.rs", 1, 0, 0, 0.05),
            make_file_metrics("d.rs", 1, 0, 0, 0.02),
        ];

        identify_hotspot_and_stable(&mut summary, &files);

        // c.rs and d.rs are below 0.1 and have commits > 0
        assert_eq!(summary.stable_files.len(), 2);
        assert!(summary.stable_files.contains(&"./c.rs".to_string()));
        assert!(summary.stable_files.contains(&"./d.rs".to_string()));
    }

    #[test]
    fn test_identify_stable_excludes_zero_commits() {
        let mut summary = Summary::default();

        let files = vec![
            make_file_metrics("a.rs", 0, 0, 0, 0.01), // commits=0, excluded
            make_file_metrics("b.rs", 1, 0, 0, 0.01), // commits=1, included
        ];

        identify_hotspot_and_stable(&mut summary, &files);

        assert_eq!(summary.stable_files.len(), 1);
        assert!(summary.stable_files.contains(&"./b.rs".to_string()));
    }

    // --- calculate_relative_churn edge cases ---

    #[test]
    fn test_relative_churn_zero_loc() {
        // total_loc stays 0 because the file path doesn't exist on disk.
        // relative_churn should remain 0.0 (no division by zero).
        let mut fm = make_file_metrics("nonexistent_file.rs", 5, 100, 50, 0.5);
        fm.total_loc = 0;

        calculate_relative_churn(&mut fm, "/tmp/fake_repo_does_not_exist", Utc::now());

        assert_eq!(fm.total_loc, 0);
        assert_eq!(fm.relative_churn, 0.0);
    }

    #[test]
    fn test_churn_rate_zero_days() {
        // When first_commit == last_commit, days_active is clamped to 1 (not 0).
        let t = Utc::now();
        let mut fm = make_file_metrics("test.rs", 3, 30, 10, 0.5);
        fm.first_commit = Some(t);
        fm.last_commit = Some(t);
        fm.total_loc = 100;

        // Manually set total_loc since the file won't exist on disk
        calculate_relative_churn(&mut fm, "/tmp/fake_repo_does_not_exist", Utc::now());

        // days_active should be max(0, 1) = 1 due to .max(1)
        assert_eq!(fm.days_active, 1);
        // No division by zero: churn_rate and change_frequency should be finite
        assert!(fm.churn_rate.is_finite());
        assert!(fm.change_frequency.is_finite());
    }

    #[test]
    fn test_relative_churn_no_timestamps() {
        // If first_commit and last_commit are both None, days_active stays 0.
        // churn_rate and change_frequency should remain 0 (guarded by if days_active > 0).
        let mut fm = make_file_metrics("test.rs", 2, 20, 10, 0.3);
        fm.first_commit = None;
        fm.last_commit = None;

        calculate_relative_churn(&mut fm, "/tmp/fake_repo_does_not_exist", Utc::now());

        assert_eq!(fm.days_active, 0);
        assert_eq!(fm.churn_rate, 0.0);
        assert_eq!(fm.change_frequency, 0.0);
    }

    // --- percentile tests ---

    #[test]
    fn test_percentile_calculation() {
        // 20 values: 0.05, 0.10, ..., 1.00
        let sorted: Vec<f64> = (1..=20).map(|i| i as f64 * 0.05).collect();

        // p50: index = (50 * 20) / 100 = 10 -> sorted[10] = 0.55
        assert!((percentile(&sorted, 50) - 0.55).abs() < 1e-9);

        // p95: index = (95 * 20) / 100 = 19 -> sorted[19] = 1.0
        assert!((percentile(&sorted, 95) - 1.0).abs() < 1e-9);

        // p0: index = 0 -> sorted[0] = 0.05
        assert!((percentile(&sorted, 0) - 0.05).abs() < 1e-9);

        // p100: index = 20, clamped to 19 -> sorted[19] = 1.0
        assert!((percentile(&sorted, 100) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_single_element() {
        let sorted = vec![0.42];
        assert!((percentile(&sorted, 0) - 0.42).abs() < 1e-9);
        assert!((percentile(&sorted, 50) - 0.42).abs() < 1e-9);
        assert!((percentile(&sorted, 100) - 0.42).abs() < 1e-9);
    }

    // --- churn score normalization ---

    #[test]
    fn test_churn_score_normalization() {
        // All computed churn scores should be in [0.0, 1.0].
        let test_cases: Vec<(u32, u32, u32, u32, u32)> = vec![
            // (commits, added, deleted, max_commits, max_changes)
            (0, 0, 0, 10, 100),        // zero everything
            (10, 100, 50, 10, 150),    // equal to max
            (5, 50, 25, 10, 150),      // half of max
            (1, 1, 0, 1, 1),           // minimum non-zero
            (100, 1000, 500, 50, 200), // exceeds max (should clamp)
        ];

        for (commits, added, deleted, max_c, max_ch) in test_cases {
            let mut fm = make_file_metrics("test.rs", commits, added, deleted, 0.0);
            calculate_churn_score(&mut fm, max_c, max_ch);
            assert!(
                fm.churn_score >= 0.0 && fm.churn_score <= 1.0,
                "Score {} out of [0,1] for commits={}, added={}, deleted={}, max_c={}, max_ch={}",
                fm.churn_score,
                commits,
                added,
                deleted,
                max_c,
                max_ch,
            );
        }
    }

    #[test]
    fn test_churn_score_zero_maxes() {
        // When max_commits and max_changes are both 0 (empty repo), score should be 0.
        let mut fm = make_file_metrics("test.rs", 0, 0, 0, 0.0);
        calculate_churn_score(&mut fm, 0, 0);
        assert_eq!(fm.churn_score, 0.0);
    }
}
