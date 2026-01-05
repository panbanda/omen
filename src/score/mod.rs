//! Composite health score analyzer.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Score analyzer - calculates composite health score.
#[derive(Default)]
pub struct Analyzer {
    weights: ScoreWeights,
}

impl Analyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_weights(weights: ScoreWeights) -> Self {
        Self { weights }
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "score"
    }

    fn description(&self) -> &'static str {
        "Calculate composite health score from multiple analyzers"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let mut components = HashMap::new();
        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        // Complexity score
        if self.weights.complexity > 0.0 {
            let complexity_analyzer = crate::analyzers::complexity::Analyzer::new();
            if let Ok(result) = complexity_analyzer.analyze(ctx) {
                let score = calculate_complexity_score(&result);
                let file_count = result.files.len();
                components.insert(
                    "complexity".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.complexity,
                        details: format!(
                            "Analyzed {} files, avg cyclomatic: {:.1}",
                            file_count, result.summary.avg_cyclomatic
                        ),
                    },
                );
                weighted_sum += score * self.weights.complexity;
                total_weight += self.weights.complexity;
            }
        }

        // SATD score
        if self.weights.satd > 0.0 {
            let satd_analyzer = crate::analyzers::satd::Analyzer::new();
            if let Ok(result) = satd_analyzer.analyze(ctx) {
                let file_count = ctx.files.files().len();
                let score = calculate_satd_score(&result, file_count);
                let high_weight_count = result
                    .items
                    .iter()
                    .filter(|i| {
                        matches!(
                            i.severity,
                            crate::analyzers::satd::Severity::Critical
                                | crate::analyzers::satd::Severity::High
                        )
                    })
                    .count();
                components.insert(
                    "satd".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.satd,
                        details: format!(
                            "Found {} debt items ({} high priority)",
                            result.items.len(),
                            high_weight_count
                        ),
                    },
                );
                weighted_sum += score * self.weights.satd;
                total_weight += self.weights.satd;
            }
        }

        // Deadcode score
        if self.weights.deadcode > 0.0 {
            let deadcode_analyzer = crate::analyzers::deadcode::Analyzer::new();
            if let Ok(result) = deadcode_analyzer.analyze(ctx) {
                let score = calculate_deadcode_score(&result);
                components.insert(
                    "deadcode".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.deadcode,
                        details: format!("Found {} dead code items", result.items.len()),
                    },
                );
                weighted_sum += score * self.weights.deadcode;
                total_weight += self.weights.deadcode;
            }
        }

        // Calculate final score
        let overall_score = if total_weight > 0.0 {
            weighted_sum / total_weight
        } else {
            100.0
        };

        let grade = score_to_grade(overall_score);

        let analyzers_run = components.len();
        let critical_issues = count_critical_issues(&components);
        Ok(Analysis {
            overall_score,
            grade,
            components,
            summary: AnalysisSummary {
                files_analyzed: ctx.files.files().len(),
                analyzers_run,
                critical_issues,
            },
        })
    }
}

fn calculate_complexity_score(result: &crate::analyzers::complexity::Analysis) -> f64 {
    // Score based on average complexity
    // 0-5: 100, 5-10: 90-100, 10-20: 70-90, 20-30: 50-70, 30+: 0-50
    let avg = result.summary.avg_cyclomatic;
    if avg <= 5.0 {
        100.0
    } else if avg <= 10.0 {
        90.0 + (10.0 - avg) * 2.0
    } else if avg <= 20.0 {
        70.0 + (20.0 - avg) * 2.0
    } else if avg <= 30.0 {
        50.0 + (30.0 - avg) * 2.0
    } else {
        (50.0 - (avg - 30.0)).max(0.0)
    }
}

fn calculate_satd_score(result: &crate::analyzers::satd::Analysis, file_count: usize) -> f64 {
    // Score based on debt density
    if file_count == 0 {
        return 100.0;
    }
    let density = result.items.len() as f64 / file_count as f64;
    // 0 debt: 100, 0.1 per file: 90, 0.5 per file: 70, 1+ per file: 50 or less
    if density <= 0.0 {
        100.0
    } else if density <= 0.1 {
        90.0 + (0.1 - density) * 100.0
    } else if density <= 0.5 {
        70.0 + (0.5 - density) * 50.0
    } else if density <= 1.0 {
        50.0 + (1.0 - density) * 40.0
    } else {
        (50.0 - (density - 1.0) * 10.0).max(0.0)
    }
}

fn calculate_deadcode_score(result: &crate::analyzers::deadcode::Analysis) -> f64 {
    // Simple: fewer dead code items = higher score
    let count = result.items.len();
    if count == 0 {
        100.0
    } else if count <= 5 {
        90.0 - count as f64 * 2.0
    } else if count <= 20 {
        80.0 - (count - 5) as f64 * 2.0
    } else {
        (50.0 - (count - 20) as f64).max(0.0)
    }
}

fn score_to_grade(score: f64) -> String {
    if score >= 90.0 {
        "A".to_string()
    } else if score >= 80.0 {
        "B".to_string()
    } else if score >= 70.0 {
        "C".to_string()
    } else if score >= 60.0 {
        "D".to_string()
    } else {
        "F".to_string()
    }
}

fn count_critical_issues(components: &HashMap<String, ScoreComponent>) -> usize {
    components.values().filter(|c| c.score < 50.0).count()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub overall_score: f64,
    pub grade: String,
    pub components: HashMap<String, ScoreComponent>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreComponent {
    pub score: f64,
    pub weight: f64,
    pub details: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub files_analyzed: usize,
    pub analyzers_run: usize,
    pub critical_issues: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreWeights {
    pub complexity: f64,
    pub satd: f64,
    pub deadcode: f64,
    pub churn: f64,
    pub duplicates: f64,
    pub defect: f64,
    pub ownership: f64,
    pub cohesion: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            complexity: 1.0,
            satd: 0.8,
            deadcode: 0.6,
            churn: 0.7,
            duplicates: 0.8,
            defect: 0.9,
            ownership: 0.5,
            cohesion: 0.6,
        }
    }
}
