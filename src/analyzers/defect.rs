//! Defect prediction analyzer using PMAT-weighted metrics.
//!
//! Predicts defect probability based on:
//! - Churn: How frequently the file changes (from git history)
//! - Complexity: Cyclomatic/cognitive complexity (from complexity::Analyzer)
//! - Duplication: Code clone ratio (from duplicates::Analyzer)
//! - Coupling: Afferent coupling (from graph::Analyzer edge analysis)
//! - Ownership: Contributor diffusion (Bird et al. 2011, from git history)

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::analyzers::{complexity, duplicates, graph};
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};
use crate::git::GitRepo;

/// Risk level categories (PMAT-compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,    // < 0.3
    Medium, // 0.3 - 0.7
    High,   // >= 0.7
}

impl RiskLevel {
    fn from_probability(prob: f32) -> Self {
        if prob >= 0.7 {
            RiskLevel::High
        } else if prob >= 0.3 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        }
    }
}

/// PMAT weights for defect prediction factors.
/// Based on empirical research + ownership research (Bird et al. 2011).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub churn: f32,
    pub complexity: f32,
    pub duplication: f32,
    pub coupling: f32,
    pub ownership: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            churn: 0.30,
            complexity: 0.25,
            duplication: 0.20,
            coupling: 0.10,
            ownership: 0.15,
        }
    }
}

/// Configuration for defect analyzer.
#[derive(Debug, Clone)]
pub struct Config {
    pub weights: Weights,
    pub churn_days: u32,
    pub max_file_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            weights: Weights::default(),
            churn_days: 30,
            max_file_size: 0, // No limit
        }
    }
}

/// Defect prediction analyzer using PMAT weights.
pub struct Analyzer {
    config: Config,
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
        }
    }

    pub fn with_config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn with_churn_days(mut self, days: u32) -> Self {
        self.config.churn_days = days;
        self
    }

    pub fn with_weights(mut self, weights: Weights) -> Self {
        self.config.weights = weights;
        self
    }

    /// Compute complexity data from complexity::Analyzer.
    /// Returns a map of file path -> (cyclomatic, cognitive) complexity.
    fn compute_complexity_data(&self, ctx: &AnalysisContext<'_>) -> HashMap<String, (u32, u32)> {
        let mut data = HashMap::new();

        let analyzer = complexity::Analyzer::new();
        if let Ok(analysis) = analyzer.analyze(ctx) {
            for file_result in analysis.files {
                data.insert(
                    file_result.path.clone(),
                    (file_result.total_cyclomatic, file_result.total_cognitive),
                );
            }
        }

        data
    }

    /// Compute duplication data from duplicates::Analyzer.
    /// Returns a map of file path -> duplication ratio (0.0-1.0).
    fn compute_duplication_data(&self, ctx: &AnalysisContext<'_>) -> HashMap<String, f32> {
        let mut data = HashMap::new();

        let analyzer = duplicates::Analyzer::new();
        if let Ok(analysis) = analyzer.analyze(ctx) {
            // Count lines involved in clones per file
            let mut clone_lines: HashMap<String, usize> = HashMap::new();
            let mut total_lines: HashMap<String, usize> = HashMap::new();

            for clone in &analysis.clones {
                // File A
                *clone_lines.entry(clone.file_a.clone()).or_insert(0) += clone.lines_a;
                // File B
                *clone_lines.entry(clone.file_b.clone()).or_insert(0) += clone.lines_b;
            }

            // Get total lines per file (approximate from clone data)
            for clone in &analysis.clones {
                total_lines.entry(clone.file_a.clone()).or_insert_with(|| {
                    // Estimate total lines from file (read if needed)
                    std::fs::read_to_string(ctx.root.join(&clone.file_a))
                        .map(|c| c.lines().count())
                        .unwrap_or(1000)
                });
                total_lines.entry(clone.file_b.clone()).or_insert_with(|| {
                    std::fs::read_to_string(ctx.root.join(&clone.file_b))
                        .map(|c| c.lines().count())
                        .unwrap_or(1000)
                });
            }

            // Calculate ratio for each file that has clones
            for (file, lines) in clone_lines {
                let total = *total_lines.get(&file).unwrap_or(&1);
                let ratio = (lines as f32 / total as f32).min(1.0);
                data.insert(file, ratio);
            }
        }

        data
    }

    /// Compute coupling data from graph::Analyzer.
    /// Returns a map of file path -> (afferent_coupling, efferent_coupling).
    fn compute_coupling_data(&self, ctx: &AnalysisContext<'_>) -> HashMap<String, (f32, f32)> {
        let mut data = HashMap::new();

        let analyzer = graph::Analyzer::new();
        if let Ok(analysis) = analyzer.analyze(ctx) {
            // Count incoming and outgoing edges per node
            let mut incoming: HashMap<String, usize> = HashMap::new();
            let mut outgoing: HashMap<String, usize> = HashMap::new();

            for edge in &analysis.edges {
                *outgoing.entry(edge.from.clone()).or_insert(0) += 1;
                *incoming.entry(edge.to.clone()).or_insert(0) += 1;
            }

            // Collect all nodes
            let mut nodes: HashSet<String> = HashSet::new();
            for node in &analysis.nodes {
                nodes.insert(node.path.clone());
            }

            // Calculate coupling for each node
            for node in nodes {
                let ca = *incoming.get(&node).unwrap_or(&0) as f32;
                let ce = *outgoing.get(&node).unwrap_or(&0) as f32;
                data.insert(node, (ca, ce));
            }
        }

        data
    }

    /// Get file metrics for defect prediction.
    /// Uses pre-computed data from other analyzers when available.
    fn get_file_metrics(
        &self,
        git_path: &std::path::Path,
        file_path: &str,
        complexity_data: &HashMap<String, (u32, u32)>,
        duplication_data: &HashMap<String, f32>,
        coupling_data: &HashMap<String, (f32, f32)>,
    ) -> FileMetrics {
        let mut metrics = FileMetrics {
            file_path: file_path.to_string(),
            ..Default::default()
        };

        // Get churn and ownership metrics from gix
        if let Ok(repo) = GitRepo::open(git_path) {
            let since = format!("{} days", self.config.churn_days);
            if let Ok(commits) = repo.log_with_stats(Some(&since)) {
                // Count commits touching this file
                let file_pathbuf = PathBuf::from(file_path);
                let commit_count = commits
                    .iter()
                    .filter(|c| c.files.iter().any(|f| f.path == file_pathbuf))
                    .count();
                // Normalize churn score (max ~20 commits/month = high churn)
                metrics.churn_score = (commit_count as f32 / 20.0).min(1.0);

                // Count unique contributors to this file (from all history we have)
                let mut contributors: std::collections::HashSet<&str> =
                    std::collections::HashSet::new();
                for commit in &commits {
                    if commit.files.iter().any(|f| f.path == file_pathbuf) {
                        contributors.insert(&commit.author);
                    }
                }
                metrics.ownership_diffusion = contributors.len() as f32;
            }
        }

        // Get file LOC for confidence calculation
        if let Ok(content) = std::fs::read_to_string(git_path.join(file_path)) {
            metrics.lines_of_code = content.lines().count();
        }

        // Use pre-computed complexity data if available, otherwise estimate from LOC
        if let Some(&(cyclomatic, cognitive)) = complexity_data.get(file_path) {
            metrics.cyclomatic_complexity = cyclomatic;
            metrics.cognitive_complexity = cognitive;
            // Use cyclomatic complexity directly for the complexity metric
            metrics.complexity = cyclomatic as f32;
            metrics.uses_estimated_complexity = false;
        } else {
            // Fall back to LOC-based estimate
            metrics.complexity = (metrics.lines_of_code as f32 / 30.0).min(50.0);
            metrics.uses_estimated_complexity = true;
        }

        // Use pre-computed duplication data if available
        if let Some(&ratio) = duplication_data.get(file_path) {
            metrics.duplicate_ratio = ratio;
        }

        // Use pre-computed coupling data if available
        if let Some(&(afferent, efferent)) = coupling_data.get(file_path) {
            metrics.afferent_coupling = afferent;
            metrics.efferent_coupling = efferent;
        }

        metrics
    }

    /// Calculate defect probability from metrics.
    fn calculate_probability(&self, metrics: &FileMetrics) -> f32 {
        let w = &self.config.weights;

        let churn_norm = normalize_churn(metrics.churn_score);
        let complexity_norm = normalize_complexity(metrics.complexity);
        let duplicate_norm = normalize_duplication(metrics.duplicate_ratio);
        let coupling_norm = normalize_coupling(metrics.afferent_coupling);
        let ownership_norm = normalize_ownership(metrics.ownership_diffusion);

        let raw_score = w.churn * churn_norm
            + w.complexity * complexity_norm
            + w.duplication * duplicate_norm
            + w.coupling * coupling_norm
            + w.ownership * ownership_norm;

        sigmoid(raw_score)
    }

    /// Calculate confidence based on data availability.
    fn calculate_confidence(&self, metrics: &FileMetrics) -> f32 {
        let mut confidence = 1.0f32;

        // Reduce confidence for very small files
        if metrics.lines_of_code < 10 {
            confidence *= 0.5;
        } else if metrics.lines_of_code < 50 {
            confidence *= 0.8;
        }

        // Reduce confidence for very new files (no churn history)
        if metrics.churn_score == 0.0 {
            confidence *= 0.85;
        }

        // Reduce confidence when using LOC-based complexity estimate instead of
        // actual cyclomatic complexity from complexity::Analyzer
        if metrics.uses_estimated_complexity {
            confidence *= 0.85;
        }

        confidence.clamp(0.0, 1.0)
    }

    /// Generate recommendations based on metrics.
    fn generate_recommendations(&self, metrics: &FileMetrics, prob: f32) -> Vec<String> {
        let mut recs = Vec::new();

        if metrics.churn_score > 0.7 {
            recs.push(
                "High churn detected. Consider stabilizing with better test coverage.".to_string(),
            );
        }

        if metrics.complexity > 20.0 {
            recs.push("High complexity. Consider refactoring into smaller functions.".to_string());
        }

        if metrics.duplicate_ratio > 0.2 {
            recs.push("Significant code duplication. Extract common logic.".to_string());
        }

        if metrics.cyclomatic_complexity > 15 {
            recs.push("Complex control flow. Simplify conditional logic.".to_string());
        }

        if metrics.ownership_diffusion >= 8.0 {
            recs.push(
                "High contributor diffusion. Consider establishing clearer ownership.".to_string(),
            );
        } else if metrics.ownership_diffusion >= 5.0 {
            recs.push(
                "Moderate contributor diffusion. Document ownership responsibilities.".to_string(),
            );
        }

        if prob > 0.8 {
            recs.push(
                "CRITICAL: Very high defect probability. Prioritize review and testing."
                    .to_string(),
            );
        } else if prob > 0.6 {
            recs.push("HIGH RISK: Schedule a code review and add comprehensive tests.".to_string());
        }

        if recs.is_empty() {
            recs.push("No immediate concerns. Continue monitoring metrics.".to_string());
        }

        recs
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "defect"
    }

    fn description(&self) -> &'static str {
        "Predict defect probability using PMAT-weighted metrics"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let git_path = ctx
            .git_path
            .ok_or_else(|| crate::core::Error::git("Defect analyzer requires a git repository"))?;

        // Pre-compute data from other analyzers
        let complexity_data = self.compute_complexity_data(ctx);
        let duplication_data = self.compute_duplication_data(ctx);
        let coupling_data = self.compute_coupling_data(ctx);

        let mut files = Vec::new();
        let mut total_prob = 0.0f32;
        let mut high_count = 0;
        let mut medium_count = 0;
        let mut low_count = 0;

        for path in ctx.files.iter() {
            let file_path = path.strip_prefix(ctx.root).unwrap_or(path);
            let file_str = file_path.to_string_lossy();

            // Skip if file is too large
            if self.config.max_file_size > 0 {
                if let Ok(meta) = std::fs::metadata(path) {
                    if meta.len() as usize > self.config.max_file_size {
                        continue;
                    }
                }
            }

            let metrics = self.get_file_metrics(
                git_path,
                &file_str,
                &complexity_data,
                &duplication_data,
                &coupling_data,
            );
            let prob = self.calculate_probability(&metrics);
            let confidence = self.calculate_confidence(&metrics);
            let risk = RiskLevel::from_probability(prob);

            let w = &self.config.weights;
            let contributing_factors = HashMap::from([
                (
                    "churn".to_string(),
                    normalize_churn(metrics.churn_score) * w.churn,
                ),
                (
                    "complexity".to_string(),
                    normalize_complexity(metrics.complexity) * w.complexity,
                ),
                (
                    "duplication".to_string(),
                    normalize_duplication(metrics.duplicate_ratio) * w.duplication,
                ),
                (
                    "coupling".to_string(),
                    normalize_coupling(metrics.afferent_coupling) * w.coupling,
                ),
                (
                    "ownership".to_string(),
                    normalize_ownership(metrics.ownership_diffusion) * w.ownership,
                ),
            ]);

            let recommendations = self.generate_recommendations(&metrics, prob);

            files.push(FileScore {
                file_path: file_str.to_string(),
                probability: prob,
                confidence,
                risk_level: risk,
                contributing_factors,
                recommendations,
            });

            total_prob += prob;
            match risk {
                RiskLevel::High => high_count += 1,
                RiskLevel::Medium => medium_count += 1,
                RiskLevel::Low => low_count += 1,
            }
        }

        // Calculate summary statistics
        let mut summary = Summary {
            total_files: files.len(),
            high_risk_count: high_count,
            medium_risk_count: medium_count,
            low_risk_count: low_count,
            ..Default::default()
        };

        if !files.is_empty() {
            summary.avg_probability = total_prob / files.len() as f32;

            let mut probs: Vec<f32> = files.iter().map(|f| f.probability).collect();
            probs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            summary.p50_probability = percentile(&probs, 50.0);
            summary.p95_probability = percentile(&probs, 95.0);
        }

        Ok(Analysis {
            files,
            summary,
            weights: self.config.weights.clone(),
        })
    }
}

/// Internal file metrics.
#[derive(Debug, Clone, Default)]
struct FileMetrics {
    #[allow(dead_code)]
    file_path: String,
    churn_score: f32,
    complexity: f32,
    duplicate_ratio: f32,
    afferent_coupling: f32,
    efferent_coupling: f32,
    lines_of_code: usize,
    cyclomatic_complexity: u32,
    #[allow(dead_code)]
    cognitive_complexity: u32,
    ownership_diffusion: f32,
    #[allow(dead_code)]
    ownership_concentration: f32,
    /// True when complexity is estimated from LOC rather than actual cyclomatic analysis.
    /// Used to reduce confidence in the prediction.
    uses_estimated_complexity: bool,
}

// Output types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub files: Vec<FileScore>,
    pub summary: Summary,
    pub weights: Weights,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileScore {
    pub file_path: String,
    pub probability: f32,
    pub confidence: f32,
    pub risk_level: RiskLevel,
    pub contributing_factors: HashMap<String, f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_files: usize,
    pub high_risk_count: usize,
    pub medium_risk_count: usize,
    pub low_risk_count: usize,
    pub avg_probability: f32,
    pub p50_probability: f32,
    pub p95_probability: f32,
}

// PMAT-compatible CDF percentile tables for normalization

const CHURN_PERCENTILES: &[[f32; 2]] = &[
    [0.0, 0.0],
    [0.1, 0.05],
    [0.2, 0.15],
    [0.3, 0.30],
    [0.4, 0.50],
    [0.5, 0.70],
    [0.6, 0.85],
    [0.7, 0.93],
    [0.8, 0.97],
    [1.0, 1.0],
];

const COMPLEXITY_PERCENTILES: &[[f32; 2]] = &[
    [1.0, 0.1],
    [2.0, 0.2],
    [3.0, 0.3],
    [5.0, 0.5],
    [7.0, 0.7],
    [10.0, 0.8],
    [15.0, 0.9],
    [20.0, 0.95],
    [30.0, 0.98],
    [50.0, 1.0],
];

const COUPLING_PERCENTILES: &[[f32; 2]] = &[
    [0.0, 0.1],
    [1.0, 0.3],
    [2.0, 0.5],
    [3.0, 0.7],
    [5.0, 0.8],
    [8.0, 0.9],
    [12.0, 0.95],
    [20.0, 1.0],
];

const OWNERSHIP_PERCENTILES: &[[f32; 2]] = &[
    [1.0, 0.1],   // Single owner = low risk
    [2.0, 0.3],   // 2 contributors = moderate
    [3.0, 0.5],   // 3 contributors = medium
    [5.0, 0.7],   // 5 contributors = elevated
    [8.0, 0.85],  // 8 contributors = high
    [12.0, 0.95], // 12+ contributors = very high
    [20.0, 1.0],  // 20+ contributors = maximum risk
];

/// Linear interpolation on CDF percentile tables.
fn interpolate_cdf(percentiles: &[[f32; 2]], value: f32) -> f32 {
    if value <= percentiles[0][0] {
        return percentiles[0][1];
    }
    if value >= percentiles[percentiles.len() - 1][0] {
        return percentiles[percentiles.len() - 1][1];
    }

    for i in 0..percentiles.len() - 1 {
        let (x1, y1) = (percentiles[i][0], percentiles[i][1]);
        let (x2, y2) = (percentiles[i + 1][0], percentiles[i + 1][1]);

        if value >= x1 && value <= x2 {
            let t = (value - x1) / (x2 - x1);
            return y1 + t * (y2 - y1);
        }
    }

    0.0
}

fn normalize_churn(raw_score: f32) -> f32 {
    interpolate_cdf(CHURN_PERCENTILES, raw_score)
}

fn normalize_complexity(raw_score: f32) -> f32 {
    interpolate_cdf(COMPLEXITY_PERCENTILES, raw_score)
}

fn normalize_duplication(raw_score: f32) -> f32 {
    raw_score.clamp(0.0, 1.0)
}

fn normalize_coupling(raw_score: f32) -> f32 {
    interpolate_cdf(COUPLING_PERCENTILES, raw_score)
}

fn normalize_ownership(contributors: f32) -> f32 {
    interpolate_cdf(OWNERSHIP_PERCENTILES, contributors)
}

/// Sigmoid transformation for probability calibration.
fn sigmoid(raw_score: f32) -> f32 {
    1.0 / (1.0 + (-10.0 * (raw_score - 0.5)).exp())
}

/// Calculate percentile from sorted values.
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f32).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "defect");
        assert!(analyzer.requires_git());
    }

    #[test]
    fn test_default_weights() {
        let weights = Weights::default();
        let total = weights.churn
            + weights.complexity
            + weights.duplication
            + weights.coupling
            + weights.ownership;
        assert!((total - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_risk_level_from_probability() {
        assert_eq!(RiskLevel::from_probability(0.8), RiskLevel::High);
        assert_eq!(RiskLevel::from_probability(0.7), RiskLevel::High);
        assert_eq!(RiskLevel::from_probability(0.5), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_probability(0.3), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_probability(0.2), RiskLevel::Low);
        assert_eq!(RiskLevel::from_probability(0.0), RiskLevel::Low);
    }

    #[test]
    fn test_normalize_churn() {
        assert!((normalize_churn(0.0) - 0.0).abs() < 0.001);
        assert!((normalize_churn(1.0) - 1.0).abs() < 0.001);
        assert!(normalize_churn(0.5) > 0.5); // Non-linear
    }

    #[test]
    fn test_normalize_complexity() {
        assert!((normalize_complexity(1.0) - 0.1).abs() < 0.001);
        assert!(normalize_complexity(10.0) > 0.7);
        assert!((normalize_complexity(50.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_duplication() {
        assert_eq!(normalize_duplication(0.5), 0.5);
        assert_eq!(normalize_duplication(-0.1), 0.0);
        assert_eq!(normalize_duplication(1.5), 1.0);
    }

    #[test]
    fn test_normalize_ownership() {
        assert!(normalize_ownership(1.0) < 0.2); // Single owner = low risk
        assert!(normalize_ownership(5.0) > 0.5); // 5 contributors = elevated
        assert!(normalize_ownership(20.0) >= 0.99); // Many contributors = high risk
    }

    #[test]
    fn test_sigmoid() {
        assert!((sigmoid(0.5) - 0.5).abs() < 0.001);
        assert!(sigmoid(0.0) < 0.01);
        assert!(sigmoid(1.0) > 0.99);
    }

    #[test]
    fn test_percentile() {
        let values = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        assert!((percentile(&values, 50.0) - 0.3).abs() < 0.001);
        assert!((percentile(&values, 0.0) - 0.1).abs() < 0.001);
        assert!((percentile(&values, 100.0) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_percentile_empty() {
        let values: Vec<f32> = vec![];
        assert!((percentile(&values, 50.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_interpolate_cdf() {
        // Test exact matches
        assert!((interpolate_cdf(CHURN_PERCENTILES, 0.0) - 0.0).abs() < 0.001);
        assert!((interpolate_cdf(CHURN_PERCENTILES, 1.0) - 1.0).abs() < 0.001);

        // Test interpolation
        let mid = interpolate_cdf(CHURN_PERCENTILES, 0.15);
        assert!(mid > 0.05 && mid < 0.15);
    }

    #[test]
    fn test_calculate_probability() {
        let analyzer = Analyzer::new();

        // Low risk file
        let low_risk = FileMetrics {
            churn_score: 0.1,
            complexity: 2.0,
            duplicate_ratio: 0.0,
            ownership_diffusion: 1.0,
            ..Default::default()
        };
        let prob = analyzer.calculate_probability(&low_risk);
        assert!(prob < 0.3);

        // High risk file
        let high_risk = FileMetrics {
            churn_score: 0.9,
            complexity: 30.0,
            duplicate_ratio: 0.5,
            ownership_diffusion: 10.0,
            ..Default::default()
        };
        let prob = analyzer.calculate_probability(&high_risk);
        assert!(prob > 0.7);
    }

    #[test]
    fn test_calculate_confidence() {
        let analyzer = Analyzer::new();

        // Full confidence
        let full = FileMetrics {
            lines_of_code: 100,
            churn_score: 0.5,
            afferent_coupling: 5.0,
            ..Default::default()
        };
        let conf = analyzer.calculate_confidence(&full);
        assert!(conf > 0.9);

        // Low confidence (small file, no history)
        let low = FileMetrics {
            lines_of_code: 5,
            churn_score: 0.0,
            ..Default::default()
        };
        let conf = analyzer.calculate_confidence(&low);
        assert!(conf < 0.5);
    }

    #[test]
    fn test_recommendations_high_churn() {
        let analyzer = Analyzer::new();
        let metrics = FileMetrics {
            churn_score: 0.8,
            ..Default::default()
        };
        let recs = analyzer.generate_recommendations(&metrics, 0.5);
        assert!(recs.iter().any(|r| r.contains("churn")));
    }

    #[test]
    fn test_recommendations_high_complexity() {
        let analyzer = Analyzer::new();
        let metrics = FileMetrics {
            complexity: 25.0,
            ..Default::default()
        };
        let recs = analyzer.generate_recommendations(&metrics, 0.5);
        assert!(recs.iter().any(|r| r.contains("complexity")));
    }

    #[test]
    fn test_recommendations_critical() {
        let analyzer = Analyzer::new();
        let metrics = FileMetrics::default();
        let recs = analyzer.generate_recommendations(&metrics, 0.85);
        assert!(recs.iter().any(|r| r.contains("CRITICAL")));
    }

    #[test]
    fn test_complexity_estimate_reasonable() {
        // Test that LOC-based complexity estimate produces reasonable values.
        // Using LOC/30 should give ~3.3 for 100 lines (not 2.0 from LOC/50).
        // This is still an estimate - actual cyclomatic complexity requires
        // integration with complexity::Analyzer.

        // 100 lines should estimate ~3.3 complexity
        let estimate_100 = (100.0_f32 / 30.0).min(50.0);
        assert!(
            (3.0..=4.0).contains(&estimate_100),
            "100 LOC should estimate 3-4 complexity, got {}",
            estimate_100
        );

        // 300 lines should estimate ~10 complexity
        let estimate_300 = (300.0_f32 / 30.0).min(50.0);
        assert!(
            (9.0..=11.0).contains(&estimate_300),
            "300 LOC should estimate ~10 complexity, got {}",
            estimate_300
        );

        // Very large files should cap at 50
        let estimate_large = (2000.0_f32 / 30.0).min(50.0);
        assert_eq!(estimate_large, 50.0, "Large files should cap at 50");
    }

    #[test]
    fn test_confidence_penalty_for_estimates() {
        let analyzer = Analyzer::new();

        // File with actual complexity data (hypothetical future state)
        let with_real_data = FileMetrics {
            lines_of_code: 100,
            churn_score: 0.5,
            afferent_coupling: 5.0,
            efferent_coupling: 3.0,
            duplicate_ratio: 0.1,
            uses_estimated_complexity: false,
            ..Default::default()
        };

        // File with estimated complexity (current state)
        let with_estimates = FileMetrics {
            lines_of_code: 100,
            churn_score: 0.5,
            afferent_coupling: 0.0, // No coupling data
            efferent_coupling: 0.0,
            duplicate_ratio: 0.0, // No clone data
            uses_estimated_complexity: true,
            ..Default::default()
        };

        let conf_real = analyzer.calculate_confidence(&with_real_data);
        let conf_estimated = analyzer.calculate_confidence(&with_estimates);

        // Estimated metrics should have lower confidence
        assert!(
            conf_estimated < conf_real,
            "Estimated metrics should have lower confidence: {} vs {}",
            conf_estimated,
            conf_real
        );
    }
}
