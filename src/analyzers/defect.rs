//! Defect prediction analyzer using PMAT-weighted metrics.
//!
//! Predicts defect probability based on:
//! - Churn: How frequently the file changes
//! - Complexity: Cyclomatic/cognitive complexity
//! - Duplication: Code clone ratio
//! - Coupling: Afferent coupling (dependencies)
//! - Ownership: Contributor diffusion (Bird et al. 2011)

use std::collections::HashMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

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

    /// Get file metrics from git for defect prediction.
    fn get_file_metrics(
        &self,
        git_path: &std::path::Path,
        file_path: &str,
    ) -> FileMetrics {
        let mut metrics = FileMetrics {
            file_path: file_path.to_string(),
            ..Default::default()
        };

        // Get churn metrics from git log
        if let Ok(output) = Command::new("git")
            .args([
                "log",
                "--oneline",
                &format!("--since={} days ago", self.config.churn_days),
                "--follow",
                "--",
                file_path,
            ])
            .current_dir(git_path)
            .output()
        {
            if output.status.success() {
                let commit_count = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .count();
                // Normalize churn score (max ~20 commits/month = high churn)
                metrics.churn_score = (commit_count as f32 / 20.0).min(1.0);
            }
        }

        // Get contributor count for ownership diffusion
        if let Ok(output) = Command::new("git")
            .args([
                "shortlog",
                "-sn",
                "--all",
                "--",
                file_path,
            ])
            .current_dir(git_path)
            .output()
        {
            if output.status.success() {
                let contributor_count = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .count();
                metrics.ownership_diffusion = contributor_count as f32;
            }
        }

        // Get file size as a proxy for complexity (simple heuristic)
        if let Ok(content) = std::fs::read_to_string(git_path.join(file_path)) {
            let lines = content.lines().count();
            metrics.lines_of_code = lines;
            // Estimate complexity from lines (very rough)
            metrics.complexity = (lines as f32 / 50.0).min(30.0);
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

        // Reduce confidence if coupling metrics are missing
        if metrics.afferent_coupling == 0.0 && metrics.efferent_coupling == 0.0 {
            confidence *= 0.9;
        }

        // Reduce confidence for very new files (no churn history)
        if metrics.churn_score == 0.0 {
            confidence *= 0.85;
        }

        confidence.clamp(0.0, 1.0)
    }

    /// Generate recommendations based on metrics.
    fn generate_recommendations(&self, metrics: &FileMetrics, prob: f32) -> Vec<String> {
        let mut recs = Vec::new();

        if metrics.churn_score > 0.7 {
            recs.push("High churn detected. Consider stabilizing with better test coverage.".to_string());
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
            recs.push("High contributor diffusion. Consider establishing clearer ownership.".to_string());
        } else if metrics.ownership_diffusion >= 5.0 {
            recs.push("Moderate contributor diffusion. Document ownership responsibilities.".to_string());
        }

        if prob > 0.8 {
            recs.push("CRITICAL: Very high defect probability. Prioritize review and testing.".to_string());
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
        let git_path = ctx.git_path.ok_or_else(|| {
            crate::core::Error::git("Defect analyzer requires a git repository")
        })?;

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

            let metrics = self.get_file_metrics(git_path, &file_str);
            let prob = self.calculate_probability(&metrics);
            let confidence = self.calculate_confidence(&metrics);
            let risk = RiskLevel::from_probability(prob);

            let w = &self.config.weights;
            let contributing_factors = HashMap::from([
                ("churn".to_string(), normalize_churn(metrics.churn_score) * w.churn),
                ("complexity".to_string(), normalize_complexity(metrics.complexity) * w.complexity),
                ("duplication".to_string(), normalize_duplication(metrics.duplicate_ratio) * w.duplication),
                ("coupling".to_string(), normalize_coupling(metrics.afferent_coupling) * w.coupling),
                ("ownership".to_string(), normalize_ownership(metrics.ownership_diffusion) * w.ownership),
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
    [0.0, 0.0], [0.1, 0.05], [0.2, 0.15], [0.3, 0.30],
    [0.4, 0.50], [0.5, 0.70], [0.6, 0.85], [0.7, 0.93],
    [0.8, 0.97], [1.0, 1.0],
];

const COMPLEXITY_PERCENTILES: &[[f32; 2]] = &[
    [1.0, 0.1], [2.0, 0.2], [3.0, 0.3], [5.0, 0.5],
    [7.0, 0.7], [10.0, 0.8], [15.0, 0.9], [20.0, 0.95],
    [30.0, 0.98], [50.0, 1.0],
];

const COUPLING_PERCENTILES: &[[f32; 2]] = &[
    [0.0, 0.1], [1.0, 0.3], [2.0, 0.5], [3.0, 0.7],
    [5.0, 0.8], [8.0, 0.9], [12.0, 0.95], [20.0, 1.0],
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
        let total = weights.churn + weights.complexity + weights.duplication
            + weights.coupling + weights.ownership;
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
}
