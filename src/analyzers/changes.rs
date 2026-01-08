//! JIT change risk analysis (Kamei et al. 2013).
//!
//! Predicts which commits are likely to introduce bugs using
//! Just-in-Time quality assurance features.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};
use crate::git::GitRepo;

/// Weights for change-level defect prediction features.
/// Based on Kamei et al. (2013) and Zeng et al. (2021).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    /// Is bug fix commit?
    pub fix: f64,
    /// Change entropy across files
    pub entropy: f64,
    /// Lines added
    pub la: f64,
    /// Number of unique prior commits
    pub nuc: f64,
    /// Number of files modified
    pub nf: f64,
    /// Lines deleted
    pub ld: f64,
    /// Number of developers
    pub ndev: f64,
    /// Author experience (inverted)
    pub exp: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            fix: 0.25,
            entropy: 0.20,
            la: 0.20,
            nuc: 0.10,
            nf: 0.10,
            ld: 0.05,
            ndev: 0.05,
            exp: 0.05,
        }
    }
}

/// Percentile thresholds for risk level classification.
const HIGH_RISK_PERCENTILE: usize = 95;
const MEDIUM_RISK_PERCENTILE: usize = 80;

/// Changes/JIT analyzer.
pub struct Analyzer {
    days: u32,
    weights: Weights,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            days: 30,
            weights: Weights::default(),
        }
    }

    pub fn with_days(mut self, days: u32) -> Self {
        if days > 0 {
            self.days = days;
        }
        self
    }

    pub fn with_weights(mut self, weights: Weights) -> Self {
        self.weights = weights;
        self
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "changes"
    }

    fn description(&self) -> &'static str {
        "Predict which commits are likely to introduce bugs (JIT)"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let git_path = ctx
            .git_path
            .ok_or_else(|| crate::core::Error::git("Changes analyzer requires a git repository"))?;

        // Get commits from last N days
        let raw_commits = collect_commit_data(git_path, self.days)?;

        if raw_commits.is_empty() {
            return Ok(Analysis {
                generated_at: Utc::now(),
                period_days: self.days as i32,
                commits: Vec::new(),
                summary: Summary::default(),
                weights: self.weights.clone(),
                normalization: NormalizationStats::default(),
                risk_thresholds: RiskThresholds::default(),
            });
        }

        // Compute state-dependent features in chronological order (oldest first)
        let mut commits_chronological = raw_commits;
        commits_chronological.reverse();
        let commits = compute_state_dependent_features(commits_chronological);

        // Calculate normalization stats
        let normalization = calculate_normalization_stats(&commits);

        // First pass: compute all risk scores
        let scores: Vec<f64> = commits
            .iter()
            .map(|c| calculate_risk(c, &self.weights, &normalization))
            .collect();

        // Calculate percentile-based thresholds
        let mut sorted_scores = scores.clone();
        sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let risk_thresholds = RiskThresholds {
            high_threshold: percentile(&sorted_scores, HIGH_RISK_PERCENTILE),
            medium_threshold: percentile(&sorted_scores, MEDIUM_RISK_PERCENTILE),
        };

        // Second pass: build commit risks with risk levels
        let mut total_score = 0.0;
        let mut high_risk_count = 0;
        let mut medium_risk_count = 0;
        let mut low_risk_count = 0;
        let mut bug_fix_count = 0;

        let mut commit_risks: Vec<CommitRisk> = commits
            .iter()
            .zip(scores.iter())
            .map(|(features, &score)| {
                total_score += score;
                if features.is_fix {
                    bug_fix_count += 1;
                }

                let risk_level = get_risk_level(score, &risk_thresholds);
                match risk_level {
                    RiskLevel::High => high_risk_count += 1,
                    RiskLevel::Medium => medium_risk_count += 1,
                    RiskLevel::Low => low_risk_count += 1,
                }

                build_commit_risk(
                    features,
                    score,
                    &self.weights,
                    &normalization,
                    &risk_thresholds,
                )
            })
            .collect();

        // Sort by risk score descending
        commit_risks.sort_by(|a, b| {
            b.risk_score
                .partial_cmp(&a.risk_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_commits = commits.len();
        let avg_risk_score = if total_commits > 0 {
            total_score / total_commits as f64
        } else {
            0.0
        };

        Ok(Analysis {
            generated_at: Utc::now(),
            period_days: self.days as i32,
            commits: commit_risks,
            summary: Summary {
                total_commits,
                high_risk_count,
                medium_risk_count,
                low_risk_count,
                bug_fix_count,
                avg_risk_score,
                p50_risk_score: percentile(&sorted_scores, 50),
                p95_risk_score: percentile(&sorted_scores, 95),
            },
            weights: self.weights.clone(),
            normalization,
            risk_thresholds,
        })
    }
}

/// Raw commit data from git log.
#[derive(Debug, Clone)]
struct RawCommit {
    features: CommitFeatures,
}

/// Commit features for change risk analysis.
#[derive(Debug, Clone, Default)]
struct CommitFeatures {
    commit_hash: String,
    author: String,
    message: String,
    timestamp: DateTime<Utc>,
    is_fix: bool,
    is_automated: bool,
    entropy: f64,
    lines_added: i32,
    lines_deleted: i32,
    num_files: i32,
    unique_changes: i32,
    num_developers: i32,
    author_experience: i32,
    files_modified: Vec<String>,
}

// Output types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: DateTime<Utc>,
    pub period_days: i32,
    pub commits: Vec<CommitRisk>,
    pub summary: Summary,
    pub weights: Weights,
    pub normalization: NormalizationStats,
    pub risk_thresholds: RiskThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRisk {
    pub commit_hash: String,
    pub author: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub risk_score: f64,
    pub risk_level: RiskLevel,
    pub contributing_factors: HashMap<String, f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
    pub files_modified: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_commits: usize,
    pub high_risk_count: usize,
    pub medium_risk_count: usize,
    pub low_risk_count: usize,
    pub bug_fix_count: usize,
    pub avg_risk_score: f64,
    pub p50_risk_score: f64,
    pub p95_risk_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizationStats {
    pub max_lines_added: i32,
    pub max_lines_deleted: i32,
    pub max_num_files: i32,
    pub max_unique_changes: i32,
    pub max_num_developers: i32,
    pub max_author_experience: i32,
    pub max_entropy: f64,
}

impl Default for NormalizationStats {
    fn default() -> Self {
        Self {
            max_lines_added: 1,
            max_lines_deleted: 1,
            max_num_files: 1,
            max_unique_changes: 1,
            max_num_developers: 1,
            max_author_experience: 1,
            max_entropy: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskThresholds {
    pub high_threshold: f64,
    pub medium_threshold: f64,
}

impl Default for RiskThresholds {
    fn default() -> Self {
        Self {
            high_threshold: 0.6,
            medium_threshold: 0.3,
        }
    }
}

// Bug fix patterns (Mockus & Votta 2000)
fn is_bug_fix_commit(message: &str) -> bool {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\bfix(es|ed|ing)?\b").unwrap(),
            Regex::new(r"(?i)\bbug\b").unwrap(),
            Regex::new(r"(?i)\bbugfix\b").unwrap(),
            Regex::new(r"(?i)\bpatch(es|ed|ing)?\b").unwrap(),
            Regex::new(r"(?i)\bresolve[sd]?\b").unwrap(),
            Regex::new(r"(?i)\bclose[sd]?\s+#\d+").unwrap(),
            Regex::new(r"(?i)\bfixes?\s+#\d+").unwrap(),
            Regex::new(r"(?i)\bdefect\b").unwrap(),
            Regex::new(r"(?i)\bissue\b").unwrap(),
            Regex::new(r"(?i)\berror\b").unwrap(),
            Regex::new(r"(?i)\bcrash(es|ed|ing)?\b").unwrap(),
        ]
    });

    patterns.iter().any(|p| p.is_match(message))
}

// Automated/trivial commit patterns
fn is_automated_commit(message: &str) -> bool {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)^\s*chore:\s*updated?\s+(image\s+)?tag").unwrap(),
            Regex::new(r"(?i)\[skip ci\]").unwrap(),
            Regex::new(r"(?i)^\s*Merge\s+(pull\s+request|branch)").unwrap(),
            Regex::new(r"(?i)^\s*chore\(deps\):").unwrap(),
            Regex::new(r"(?i)^\s*chore:\s*bump\s+version").unwrap(),
            Regex::new(r"(?i)^\s*ci:").unwrap(),
            Regex::new(r"(?i)^\s*docs?:").unwrap(),
            Regex::new(r"(?i)^\s*style:").unwrap(),
        ]
    });

    patterns.iter().any(|p| p.is_match(message))
}

/// Collect commit data from git log using gix.
fn collect_commit_data(git_path: &Path, days: u32) -> Result<Vec<RawCommit>> {
    let repo = GitRepo::open(git_path)?;
    let since = format!("{} days", days);
    let commits = repo.log_with_stats(Some(&since))?;

    commits_to_raw_commits(&commits)
}

/// Convert gix Commits to RawCommits for risk analysis.
fn commits_to_raw_commits(commits: &[crate::git::Commit]) -> Result<Vec<RawCommit>> {
    let mut raw_commits = Vec::new();

    for commit in commits {
        let timestamp = Utc.timestamp_opt(commit.timestamp, 0).single();
        let message = truncate_message(&commit.message);

        let mut lines_per_file: HashMap<String, i32> = HashMap::new();
        let mut lines_added = 0i32;
        let mut lines_deleted = 0i32;
        let mut files_modified = Vec::new();

        for file_change in &commit.files {
            let path_str = file_change.path.to_string_lossy().to_string();
            let added = file_change.additions as i32;
            let deleted = file_change.deletions as i32;

            lines_added += added;
            lines_deleted += deleted;
            files_modified.push(path_str.clone());
            *lines_per_file.entry(path_str).or_insert(0) += added + deleted;
        }

        let entropy = calculate_entropy(&lines_per_file);

        raw_commits.push(RawCommit {
            features: CommitFeatures {
                commit_hash: commit.sha.clone(),
                author: commit.author.clone(),
                message: message.clone(),
                timestamp: timestamp.unwrap_or_else(Utc::now),
                is_fix: is_bug_fix_commit(&message),
                is_automated: is_automated_commit(&message),
                entropy,
                lines_added,
                lines_deleted,
                num_files: files_modified.len() as i32,
                files_modified,
                ..Default::default()
            },
        });
    }

    Ok(raw_commits)
}

/// Parse git log output into raw commits.
#[cfg(test)]
fn parse_git_log(output: &str) -> Result<Vec<RawCommit>> {
    let mut commits = Vec::new();
    let mut current: Option<RawCommit> = None;
    let mut current_lines_per_file: HashMap<String, i32> = HashMap::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        // Check if this is a commit header line
        if line.contains('|') && !line.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
            // Save previous commit
            if let Some(mut commit) = current.take() {
                commit.features.num_files = commit.features.files_modified.len() as i32;
                commit.features.entropy = calculate_entropy(&current_lines_per_file);
                commits.push(commit);
                current_lines_per_file.clear();
            }

            // Parse new commit header
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() >= 4 {
                let hash = parts[0].to_string();
                let author = parts[1].to_string();
                let timestamp = DateTime::parse_from_rfc3339(parts[2])
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let message = truncate_message(parts[3]);

                current = Some(RawCommit {
                    features: CommitFeatures {
                        commit_hash: hash,
                        author,
                        message: message.clone(),
                        timestamp,
                        is_fix: is_bug_fix_commit(&message),
                        is_automated: is_automated_commit(&message),
                        ..Default::default()
                    },
                });
            }
        } else if let Some(ref mut commit) = current {
            // Parse numstat line: added\tdeleted\tfilename
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let added: i32 = parts[0].parse().unwrap_or(0);
                let deleted: i32 = parts[1].parse().unwrap_or(0);
                let file = parts[2].to_string();

                commit.features.lines_added += added;
                commit.features.lines_deleted += deleted;
                commit.features.files_modified.push(file.clone());
                *current_lines_per_file.entry(file).or_insert(0) += added + deleted;
            }
        }
    }

    // Don't forget the last commit
    if let Some(mut commit) = current.take() {
        commit.features.num_files = commit.features.files_modified.len() as i32;
        commit.features.entropy = calculate_entropy(&current_lines_per_file);
        commits.push(commit);
    }

    Ok(commits)
}

/// Truncate commit message to first line or 80 chars.
fn truncate_message(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or(message);
    if first_line.len() > 80 {
        format!("{}...", &first_line[..77])
    } else {
        first_line.trim().to_string()
    }
}

/// Calculate Shannon entropy of changes across files.
fn calculate_entropy(lines_per_file: &HashMap<String, i32>) -> f64 {
    if lines_per_file.is_empty() {
        return 0.0;
    }

    let total: i32 = lines_per_file.values().sum();
    if total == 0 {
        return 0.0;
    }

    let mut entropy = 0.0;
    for &lines in lines_per_file.values() {
        if lines > 0 {
            let p = lines as f64 / total as f64;
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Compute state-dependent features in chronological order.
fn compute_state_dependent_features(raw_commits: Vec<RawCommit>) -> Vec<CommitFeatures> {
    let mut author_commits: HashMap<String, i32> = HashMap::new();
    let mut file_changes: HashMap<String, i32> = HashMap::new();
    let mut file_authors: HashMap<String, HashSet<String>> = HashMap::new();

    let mut commits = Vec::new();

    for raw in raw_commits {
        let mut features = raw.features;
        let author = features.author.clone();

        // Look up state BEFORE this commit
        features.author_experience = *author_commits.get(&author).unwrap_or(&0);

        // Calculate NumDevelopers and UniqueChanges from state BEFORE this commit
        let mut unique_devs: HashSet<&str> = HashSet::new();
        let mut prior_commits = 0;

        for file_path in &features.files_modified {
            prior_commits += file_changes.get(file_path).unwrap_or(&0);
            if let Some(authors) = file_authors.get(file_path) {
                for auth in authors {
                    unique_devs.insert(auth);
                }
            }
        }

        features.num_developers = unique_devs.len() as i32;
        features.unique_changes = prior_commits;

        commits.push(features.clone());

        // Update state AFTER processing (for future commits)
        *author_commits.entry(author.clone()).or_insert(0) += 1;
        for file in &features.files_modified {
            *file_changes.entry(file.clone()).or_insert(0) += 1;
            file_authors
                .entry(file.clone())
                .or_default()
                .insert(author.clone());
        }
    }

    commits
}

/// Calculate 95th percentile normalization stats.
fn calculate_normalization_stats(commits: &[CommitFeatures]) -> NormalizationStats {
    if commits.is_empty() {
        return NormalizationStats::default();
    }

    let mut lines_added: Vec<f64> = commits.iter().map(|c| c.lines_added as f64).collect();
    let mut lines_deleted: Vec<f64> = commits.iter().map(|c| c.lines_deleted as f64).collect();
    let mut num_files: Vec<f64> = commits.iter().map(|c| c.num_files as f64).collect();
    let mut unique_changes: Vec<f64> = commits.iter().map(|c| c.unique_changes as f64).collect();
    let mut num_developers: Vec<f64> = commits.iter().map(|c| c.num_developers as f64).collect();
    let mut author_experience: Vec<f64> =
        commits.iter().map(|c| c.author_experience as f64).collect();
    let mut entropy: Vec<f64> = commits.iter().map(|c| c.entropy).collect();

    lines_added.sort_by(|a, b| a.partial_cmp(b).unwrap());
    lines_deleted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    num_files.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique_changes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    num_developers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    author_experience.sort_by(|a, b| a.partial_cmp(b).unwrap());
    entropy.sort_by(|a, b| a.partial_cmp(b).unwrap());

    NormalizationStats {
        max_lines_added: percentile(&lines_added, 95).max(1.0) as i32,
        max_lines_deleted: percentile(&lines_deleted, 95).max(1.0) as i32,
        max_num_files: percentile(&num_files, 95).max(1.0) as i32,
        max_unique_changes: percentile(&unique_changes, 95).max(1.0) as i32,
        max_num_developers: percentile(&num_developers, 95).max(1.0) as i32,
        max_author_experience: percentile(&author_experience, 95).max(1.0) as i32,
        max_entropy: percentile(&entropy, 95).max(1.0),
    }
}

/// Calculate risk score for a commit.
fn calculate_risk(features: &CommitFeatures, weights: &Weights, norm: &NormalizationStats) -> f64 {
    // Automated commits are inherently low risk
    if features.is_automated {
        return 0.05;
    }

    let fix_norm = if features.is_fix { 1.0 } else { 0.0 };
    let entropy_norm = safe_normalize(features.entropy, norm.max_entropy);
    let la_norm = safe_normalize_int(features.lines_added, norm.max_lines_added);
    let ld_norm = safe_normalize_int(features.lines_deleted, norm.max_lines_deleted);
    let nf_norm = safe_normalize_int(features.num_files, norm.max_num_files);
    let nuc_norm = safe_normalize_int(features.unique_changes, norm.max_unique_changes);
    let ndev_norm = safe_normalize_int(features.num_developers, norm.max_num_developers);
    // Experience is inverted: less experience = more risk
    let exp_norm = 1.0 - safe_normalize_int(features.author_experience, norm.max_author_experience);

    let score = weights.fix * fix_norm
        + weights.entropy * entropy_norm
        + weights.la * la_norm
        + weights.ld * ld_norm
        + weights.nf * nf_norm
        + weights.nuc * nuc_norm
        + weights.ndev * ndev_norm
        + weights.exp * exp_norm;

    score.clamp(0.0, 1.0)
}

fn safe_normalize(value: f64, max: f64) -> f64 {
    if max <= 0.0 {
        return 0.0;
    }
    if value >= max {
        return 1.0;
    }
    value / max
}

fn safe_normalize_int(value: i32, max: i32) -> f64 {
    if max <= 0 {
        return 0.0;
    }
    if value >= max {
        return 1.0;
    }
    value as f64 / max as f64
}

fn get_risk_level(score: f64, thresholds: &RiskThresholds) -> RiskLevel {
    if score >= thresholds.high_threshold {
        RiskLevel::High
    } else if score >= thresholds.medium_threshold {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

fn build_commit_risk(
    features: &CommitFeatures,
    score: f64,
    weights: &Weights,
    norm: &NormalizationStats,
    thresholds: &RiskThresholds,
) -> CommitRisk {
    let risk_level = get_risk_level(score, thresholds);

    let mut factors = HashMap::new();
    let fix_contrib = if features.is_fix { 1.0 } else { 0.0 } * weights.fix;
    factors.insert("fix".to_string(), fix_contrib);
    factors.insert(
        "entropy".to_string(),
        safe_normalize(features.entropy, norm.max_entropy) * weights.entropy,
    );
    factors.insert(
        "lines_added".to_string(),
        safe_normalize_int(features.lines_added, norm.max_lines_added) * weights.la,
    );
    factors.insert(
        "lines_deleted".to_string(),
        safe_normalize_int(features.lines_deleted, norm.max_lines_deleted) * weights.ld,
    );
    factors.insert(
        "num_files".to_string(),
        safe_normalize_int(features.num_files, norm.max_num_files) * weights.nf,
    );
    factors.insert(
        "unique_changes".to_string(),
        safe_normalize_int(features.unique_changes, norm.max_unique_changes) * weights.nuc,
    );
    factors.insert(
        "num_developers".to_string(),
        safe_normalize_int(features.num_developers, norm.max_num_developers) * weights.ndev,
    );
    factors.insert(
        "experience".to_string(),
        (1.0 - safe_normalize_int(features.author_experience, norm.max_author_experience))
            * weights.exp,
    );

    let recommendations = generate_recommendations(features, score, &factors);

    CommitRisk {
        commit_hash: features.commit_hash.clone(),
        author: features.author.clone(),
        message: features.message.clone(),
        timestamp: features.timestamp,
        risk_score: score,
        risk_level,
        contributing_factors: factors,
        recommendations,
        files_modified: features.files_modified.clone(),
    }
}

fn generate_recommendations(
    features: &CommitFeatures,
    score: f64,
    factors: &HashMap<String, f64>,
) -> Vec<String> {
    let mut recs = Vec::new();

    if features.is_fix {
        recs.push("Bug fix commit - ensure comprehensive testing of the fix".to_string());
    }

    if factors.get("entropy").copied().unwrap_or(0.0) > 0.15 {
        recs.push("High change entropy - review each modified file carefully".to_string());
    }

    if factors.get("lines_added").copied().unwrap_or(0.0) > 0.15 {
        recs.push("Large addition - consider splitting into smaller commits".to_string());
    }

    if factors.get("num_files").copied().unwrap_or(0.0) > 0.08 {
        recs.push("Many files modified - ensure changes are logically related".to_string());
    }

    if factors.get("experience").copied().unwrap_or(0.0) > 0.04 {
        recs.push(
            "Author has limited experience with these files - request senior review".to_string(),
        );
    }

    if score >= 0.7 {
        recs.push("HIGH RISK: Prioritize code review and add comprehensive tests".to_string());
    } else if score >= 0.5 {
        recs.push("Elevated risk: Consider additional testing before merge".to_string());
    }

    if recs.is_empty() {
        recs.push("Low risk commit - standard review process recommended".to_string());
    }

    recs
}

/// Calculate percentile from sorted slice.
fn percentile(sorted: &[f64], p: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p as f64 / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_weights() {
        let weights = Weights::default();
        let total = weights.fix
            + weights.entropy
            + weights.la
            + weights.nuc
            + weights.nf
            + weights.ld
            + weights.ndev
            + weights.exp;
        assert!((total - 1.0).abs() < 0.001, "Weights should sum to 1.0");
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new().with_days(60);
        assert_eq!(analyzer.days, 60);
    }

    #[test]
    fn test_bug_fix_detection() {
        assert!(is_bug_fix_commit("Fix crash on startup"));
        assert!(is_bug_fix_commit("Fixes #123"));
        assert!(is_bug_fix_commit("bugfix: memory leak"));
        assert!(is_bug_fix_commit("Resolve issue with login"));
        assert!(is_bug_fix_commit("Patch security vulnerability"));
        assert!(!is_bug_fix_commit("Add new feature"));
        assert!(!is_bug_fix_commit("Update dependencies"));
    }

    #[test]
    fn test_automated_commit_detection() {
        assert!(is_automated_commit("Merge pull request #123"));
        assert!(is_automated_commit("chore(deps): bump lodash"));
        assert!(is_automated_commit("ci: update workflow"));
        assert!(is_automated_commit("docs: update README"));
        assert!(is_automated_commit("[skip ci] minor change"));
        assert!(!is_automated_commit("Add authentication feature"));
        assert!(!is_automated_commit("Fix bug in parser"));
    }

    #[test]
    fn test_entropy_calculation() {
        // Single file: entropy = 0
        let mut single = HashMap::new();
        single.insert("file.rs".to_string(), 100);
        assert_eq!(calculate_entropy(&single), 0.0);

        // Two equal files: entropy = 1.0
        let mut two_equal = HashMap::new();
        two_equal.insert("a.rs".to_string(), 50);
        two_equal.insert("b.rs".to_string(), 50);
        assert!((calculate_entropy(&two_equal) - 1.0).abs() < 0.001);

        // Empty: entropy = 0
        let empty: HashMap<String, i32> = HashMap::new();
        assert_eq!(calculate_entropy(&empty), 0.0);
    }

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        // P50 with 10 items: idx = round(0.5 * 9) = 5, value = 6.0
        assert!((percentile(&values, 50) - 6.0).abs() < 0.001);
        // P95 with 10 items: idx = round(0.95 * 9) = 9, value = 10.0
        assert!((percentile(&values, 95) - 10.0).abs() < 0.001);
        assert_eq!(percentile(&[], 50), 0.0);
    }

    #[test]
    fn test_safe_normalize() {
        assert_eq!(safe_normalize(50.0, 100.0), 0.5);
        assert_eq!(safe_normalize(100.0, 100.0), 1.0);
        assert_eq!(safe_normalize(150.0, 100.0), 1.0);
        assert_eq!(safe_normalize(50.0, 0.0), 0.0);
    }

    #[test]
    fn test_safe_normalize_int() {
        assert_eq!(safe_normalize_int(50, 100), 0.5);
        assert_eq!(safe_normalize_int(100, 100), 1.0);
        assert_eq!(safe_normalize_int(150, 100), 1.0);
        assert_eq!(safe_normalize_int(50, 0), 0.0);
    }

    #[test]
    fn test_risk_level() {
        let thresholds = RiskThresholds {
            high_threshold: 0.8,
            medium_threshold: 0.5,
        };
        assert_eq!(get_risk_level(0.9, &thresholds), RiskLevel::High);
        assert_eq!(get_risk_level(0.6, &thresholds), RiskLevel::Medium);
        assert_eq!(get_risk_level(0.3, &thresholds), RiskLevel::Low);
    }

    #[test]
    fn test_risk_score_automated() {
        let features = CommitFeatures {
            is_automated: true,
            ..Default::default()
        };
        let weights = Weights::default();
        let norm = NormalizationStats::default();
        let score = calculate_risk(&features, &weights, &norm);
        assert_eq!(score, 0.05);
    }

    #[test]
    fn test_risk_score_fix() {
        let features = CommitFeatures {
            is_fix: true,
            ..Default::default()
        };
        let weights = Weights::default();
        let norm = NormalizationStats::default();
        let score = calculate_risk(&features, &weights, &norm);
        // fix contributes 0.25, exp contributes 0.05 (1.0 - 0 = 1.0 * 0.05)
        assert!(score > 0.25);
    }

    #[test]
    fn test_truncate_message() {
        assert_eq!(truncate_message("Short message"), "Short message");
        assert_eq!(truncate_message("Line 1\nLine 2"), "Line 1");
        let long = "a".repeat(100);
        let truncated = truncate_message(&long);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 80);
    }

    #[test]
    fn test_recommendations_bug_fix() {
        let features = CommitFeatures {
            is_fix: true,
            ..Default::default()
        };
        let factors = HashMap::new();
        let recs = generate_recommendations(&features, 0.3, &factors);
        assert!(recs.iter().any(|r| r.contains("Bug fix")));
    }

    #[test]
    fn test_recommendations_high_risk() {
        let features = CommitFeatures::default();
        let factors = HashMap::new();
        let recs = generate_recommendations(&features, 0.75, &factors);
        assert!(recs.iter().any(|r| r.contains("HIGH RISK")));
    }

    #[test]
    fn test_recommendations_low_risk() {
        let features = CommitFeatures::default();
        let factors = HashMap::new();
        let recs = generate_recommendations(&features, 0.1, &factors);
        assert!(recs.iter().any(|r| r.contains("Low risk")));
    }

    #[test]
    fn test_parse_git_log_empty() {
        let result = parse_git_log("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_git_log_single_commit() {
        let log = "abc123|John Doe|2024-01-15T10:30:00+00:00|Fix bug in parser\n\
                   10\t5\tsrc/parser.rs\n\
                   3\t1\ttests/parser_test.rs\n";
        let commits = parse_git_log(log).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].features.commit_hash, "abc123");
        assert_eq!(commits[0].features.author, "John Doe");
        assert_eq!(commits[0].features.lines_added, 13);
        assert_eq!(commits[0].features.lines_deleted, 6);
        assert_eq!(commits[0].features.num_files, 2);
        assert!(commits[0].features.is_fix);
    }

    #[test]
    fn test_normalization_stats() {
        let commits = vec![
            CommitFeatures {
                lines_added: 10,
                lines_deleted: 5,
                num_files: 2,
                entropy: 0.5,
                ..Default::default()
            },
            CommitFeatures {
                lines_added: 100,
                lines_deleted: 50,
                num_files: 10,
                entropy: 2.0,
                ..Default::default()
            },
        ];
        let norm = calculate_normalization_stats(&commits);
        assert!(norm.max_lines_added >= 10);
        assert!(norm.max_entropy >= 0.5);
    }
}

// ============================================================================
// Diff Analysis - Branch diff risk for PR review
// ============================================================================

/// Result of analyzing a branch diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub generated_at: DateTime<Utc>,
    pub source_branch: String,
    pub target_branch: String,
    pub merge_base: String,
    pub score: f64,
    pub level: RiskLevel,
    pub lines_added: i32,
    pub lines_deleted: i32,
    pub files_modified: i32,
    pub commits: i32,
    pub factors: HashMap<String, f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

impl Analyzer {
    /// Analyze the diff between current branch and target branch.
    /// If target is empty, auto-detects the default branch (main/master).
    pub fn analyze_diff(&self, repo_path: &Path, target: Option<&str>) -> Result<DiffResult> {
        // Get current branch name
        let source_branch = get_current_branch(repo_path)?;

        // Auto-detect target if not specified
        let target_branch = match target {
            Some(t) => t.to_string(),
            None => detect_default_branch(repo_path)?,
        };

        // Find merge-base
        let merge_base = get_merge_base(repo_path, &target_branch, "HEAD")?;

        // Get diff stats
        let (lines_added, lines_deleted, files_modified) = get_diff_stats(repo_path, &merge_base)?;

        // Count commits between merge-base and HEAD
        let commit_count = get_commit_count(repo_path, &merge_base)?;

        // Get lines per file for entropy calculation
        let lines_per_file = get_lines_per_file(repo_path, &merge_base)?;
        let entropy = calculate_entropy(&lines_per_file);

        // Use fixed thresholds for diff analysis (sensible PR size limits)
        let norm = diff_normalization();

        // Build features for risk calculation
        let features = CommitFeatures {
            lines_added,
            lines_deleted,
            num_files: files_modified,
            entropy,
            unique_changes: commit_count,
            is_fix: false,
            is_automated: false,
            ..Default::default()
        };

        // Calculate risk score
        let score = calculate_risk(&features, &self.weights, &norm);
        let thresholds = RiskThresholds::default();
        let level = get_risk_level(score, &thresholds);

        // Build contributing factors
        let mut factors = HashMap::new();
        factors.insert(
            "entropy".to_string(),
            safe_normalize(entropy, norm.max_entropy) * self.weights.entropy,
        );
        factors.insert(
            "lines_added".to_string(),
            safe_normalize_int(lines_added, norm.max_lines_added) * self.weights.la,
        );
        factors.insert(
            "lines_deleted".to_string(),
            safe_normalize_int(lines_deleted, norm.max_lines_deleted) * self.weights.ld,
        );
        factors.insert(
            "num_files".to_string(),
            safe_normalize_int(files_modified, norm.max_num_files) * self.weights.nf,
        );
        factors.insert(
            "commits".to_string(),
            safe_normalize_int(commit_count, norm.max_unique_changes) * self.weights.nuc,
        );

        // Generate recommendations
        let recommendations = generate_diff_recommendations(
            lines_added,
            lines_deleted,
            files_modified,
            commit_count,
            score,
            &factors,
        );

        Ok(DiffResult {
            generated_at: Utc::now(),
            source_branch,
            target_branch,
            merge_base,
            score,
            level,
            lines_added,
            lines_deleted,
            files_modified,
            commits: commit_count,
            factors,
            recommendations,
        })
    }
}

/// Fixed thresholds for branch diff analysis.
/// These represent sensible PR size limits where exceeding them indicates high risk.
fn diff_normalization() -> NormalizationStats {
    NormalizationStats {
        max_lines_added: 400,   // PRs > 400 lines are hard to review
        max_lines_deleted: 200, // Large deletions warrant attention
        max_num_files: 15,      // PRs touching > 15 files are risky
        max_unique_changes: 10, // > 10 commits suggests scope creep
        max_num_developers: 3,  // Multiple authors can indicate coordination issues
        max_author_experience: 100,
        max_entropy: 3.0, // Lower threshold - scattered changes are risky
    }
}

fn get_current_branch(repo_path: &Path) -> Result<String> {
    let repo = GitRepo::open(repo_path)?;
    repo.current_branch()
}

fn detect_default_branch(repo_path: &Path) -> Result<String> {
    let repo = GitRepo::open(repo_path)?;
    // Try main first
    for branch in ["main", "master", "origin/main", "origin/master"] {
        if repo.ref_exists(branch) {
            return Ok(branch.to_string());
        }
    }

    Err(crate::core::Error::git(
        "Could not detect default branch (main/master)",
    ))
}

fn get_merge_base(repo_path: &Path, ref1: &str, ref2: &str) -> Result<String> {
    let repo = GitRepo::open(repo_path)?;
    repo.merge_base(ref1, ref2)
}

fn get_diff_stats(repo_path: &Path, merge_base: &str) -> Result<(i32, i32, i32)> {
    let repo = GitRepo::open(repo_path)?;
    let changes = repo.diff_stats(merge_base, "HEAD")?;

    let mut added = 0i32;
    let mut deleted = 0i32;

    for change in &changes {
        added += change.additions as i32;
        deleted += change.deletions as i32;
    }

    Ok((added, deleted, changes.len() as i32))
}

fn get_commit_count(repo_path: &Path, merge_base: &str) -> Result<i32> {
    let repo = GitRepo::open(repo_path)?;
    repo.commit_count(merge_base, "HEAD")
}

fn get_lines_per_file(repo_path: &Path, merge_base: &str) -> Result<HashMap<String, i32>> {
    let repo = GitRepo::open(repo_path)?;
    let changes = repo.diff_stats(merge_base, "HEAD")?;

    let mut result = HashMap::new();
    for change in changes {
        let path_str = change.path.to_string_lossy().to_string();
        let lines = change.additions as i32 + change.deletions as i32;
        result.insert(path_str, lines);
    }

    Ok(result)
}

fn generate_diff_recommendations(
    lines_added: i32,
    lines_deleted: i32,
    files_modified: i32,
    commits: i32,
    score: f64,
    factors: &HashMap<String, f64>,
) -> Vec<String> {
    let mut recs = Vec::new();

    if lines_added > 400 {
        recs.push(format!(
            "Large PR with {} lines added - consider splitting into smaller changes",
            lines_added
        ));
    }

    if lines_deleted > 200 {
        recs.push(format!(
            "Large deletion of {} lines - ensure no accidental removals",
            lines_deleted
        ));
    }

    if files_modified > 15 {
        recs.push(format!(
            "PR touches {} files - may be difficult to review thoroughly",
            files_modified
        ));
    }

    if commits > 10 {
        recs.push(format!(
            "PR contains {} commits - consider squashing or splitting",
            commits
        ));
    }

    if factors.get("entropy").copied().unwrap_or(0.0) > 0.15 {
        recs.push(
            "Changes scattered across many files - ensure they're logically related".to_string(),
        );
    }

    if score >= 0.7 {
        recs.push(
            "HIGH RISK: Consider extra review scrutiny and comprehensive testing".to_string(),
        );
    } else if score >= 0.5 {
        recs.push("Elevated risk: Add additional reviewers or testing".to_string());
    }

    if recs.is_empty() {
        recs.push("PR is well-scoped - standard review process recommended".to_string());
    }

    recs
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    #[test]
    fn test_diff_normalization() {
        let norm = diff_normalization();
        assert_eq!(norm.max_lines_added, 400);
        assert_eq!(norm.max_lines_deleted, 200);
        assert_eq!(norm.max_num_files, 15);
        assert_eq!(norm.max_unique_changes, 10);
    }

    #[test]
    fn test_diff_recommendations_large_pr() {
        let factors = HashMap::new();
        let recs = generate_diff_recommendations(500, 50, 5, 3, 0.4, &factors);
        assert!(recs.iter().any(|r| r.contains("Large PR")));
    }

    #[test]
    fn test_diff_recommendations_many_files() {
        let factors = HashMap::new();
        let recs = generate_diff_recommendations(100, 50, 20, 3, 0.4, &factors);
        assert!(recs.iter().any(|r| r.contains("touches")));
    }

    #[test]
    fn test_diff_recommendations_many_commits() {
        let factors = HashMap::new();
        let recs = generate_diff_recommendations(100, 50, 5, 15, 0.4, &factors);
        assert!(recs.iter().any(|r| r.contains("commits")));
    }

    #[test]
    fn test_diff_recommendations_high_risk() {
        let factors = HashMap::new();
        let recs = generate_diff_recommendations(100, 50, 5, 3, 0.75, &factors);
        assert!(recs.iter().any(|r| r.contains("HIGH RISK")));
    }

    #[test]
    fn test_diff_recommendations_low_risk() {
        let factors = HashMap::new();
        let recs = generate_diff_recommendations(50, 20, 3, 2, 0.2, &factors);
        assert!(recs.iter().any(|r| r.contains("well-scoped")));
    }
}
