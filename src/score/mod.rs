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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_new() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "score");
    }

    #[test]
    fn test_analyzer_default() {
        let analyzer = Analyzer::default();
        assert_eq!(analyzer.weights.complexity, 1.0);
    }

    #[test]
    fn test_analyzer_with_weights() {
        let weights = ScoreWeights {
            complexity: 0.5,
            satd: 0.5,
            deadcode: 0.5,
            churn: 0.5,
            duplicates: 0.5,
            defect: 0.5,
            ownership: 0.5,
            cohesion: 0.5,
        };
        let analyzer = Analyzer::with_weights(weights);
        assert_eq!(analyzer.weights.complexity, 0.5);
    }

    #[test]
    fn test_analyzer_name() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "score");
    }

    #[test]
    fn test_analyzer_description() {
        let analyzer = Analyzer::new();
        assert!(analyzer.description().contains("health score"));
    }

    #[test]
    fn test_score_weights_default() {
        let weights = ScoreWeights::default();
        assert_eq!(weights.complexity, 1.0);
        assert_eq!(weights.satd, 0.8);
        assert_eq!(weights.deadcode, 0.6);
        assert_eq!(weights.churn, 0.7);
        assert_eq!(weights.duplicates, 0.8);
        assert_eq!(weights.defect, 0.9);
        assert_eq!(weights.ownership, 0.5);
        assert_eq!(weights.cohesion, 0.6);
    }

    #[test]
    fn test_calculate_complexity_score_low() {
        // avg <= 5.0 should be 100
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 3.0,
                ..Default::default()
            },
        };
        assert_eq!(calculate_complexity_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_complexity_score_medium() {
        // avg 5-10 should be 90-100
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 7.5,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!((90.0..=100.0).contains(&score));
    }

    #[test]
    fn test_calculate_complexity_score_high() {
        // avg 10-20 should be 70-90
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 15.0,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!((70.0..=90.0).contains(&score));
    }

    #[test]
    fn test_calculate_complexity_score_very_high() {
        // avg 20-30 should be 50-70
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 25.0,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!((50.0..=70.0).contains(&score));
    }

    #[test]
    fn test_calculate_complexity_score_extreme() {
        // avg > 30 should be < 50
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 50.0,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_satd_score_none() {
        let result = crate::analyzers::satd::Analysis {
            items: vec![],
            by_category: std::collections::HashMap::new(),
            density: 0.0,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 0,
                weighted_count: 0.0,
                density: 0.0,
            },
        };
        assert_eq!(calculate_satd_score(&result, 10), 100.0);
    }

    #[test]
    fn test_calculate_satd_score_zero_files() {
        let result = crate::analyzers::satd::Analysis {
            items: vec![],
            by_category: std::collections::HashMap::new(),
            density: 0.0,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 0,
                weighted_count: 0.0,
                density: 0.0,
            },
        };
        assert_eq!(calculate_satd_score(&result, 0), 100.0);
    }

    #[test]
    fn test_calculate_satd_score_low_density() {
        // 1 item in 100 files = 0.01 density, score should be high (> 90)
        let result = crate::analyzers::satd::Analysis {
            items: vec![crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: 1,
                marker: "TODO".to_string(),
                text: "test".to_string(),
                category: "design".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 1.0,
            }],
            by_category: std::collections::HashMap::new(),
            density: 0.01,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 1,
                weighted_count: 1.0,
                density: 0.01,
            },
        };
        let score = calculate_satd_score(&result, 100);
        assert!(score > 90.0);
    }

    #[test]
    fn test_calculate_satd_score_high_density() {
        // Many items per file
        let items: Vec<_> = (0..20)
            .map(|i| crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: i,
                marker: "TODO".to_string(),
                text: "test".to_string(),
                category: "design".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 1.0,
            })
            .collect();
        let result = crate::analyzers::satd::Analysis {
            items,
            by_category: std::collections::HashMap::new(),
            density: 4.0,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 20,
                weighted_count: 20.0,
                density: 4.0,
            },
        };
        let score = calculate_satd_score(&result, 5);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_deadcode_score_none() {
        let result = crate::analyzers::deadcode::Analysis {
            items: vec![],
            summary: crate::analyzers::deadcode::AnalysisSummary {
                total_items: 0,
                by_kind: std::collections::HashMap::new(),
                total_definitions: 0,
                reachable_count: 0,
            },
        };
        assert_eq!(calculate_deadcode_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_deadcode_score_few() {
        let items: Vec<_> = (0..3)
            .map(|_| crate::analyzers::deadcode::DeadCodeItem {
                file: "test.rs".to_string(),
                line: 1,
                end_line: 5,
                name: "test".to_string(),
                kind: "function".to_string(),
                visibility: "private".to_string(),
                confidence: 0.9,
                reason: "not called".to_string(),
            })
            .collect();
        let result = crate::analyzers::deadcode::Analysis {
            items,
            summary: crate::analyzers::deadcode::AnalysisSummary {
                total_items: 3,
                by_kind: std::collections::HashMap::new(),
                total_definitions: 10,
                reachable_count: 7,
            },
        };
        let score = calculate_deadcode_score(&result);
        assert!((80.0..=90.0).contains(&score));
    }

    #[test]
    fn test_calculate_deadcode_score_many() {
        let items: Vec<_> = (0..50)
            .map(|_| crate::analyzers::deadcode::DeadCodeItem {
                file: "test.rs".to_string(),
                line: 1,
                end_line: 5,
                name: "test".to_string(),
                kind: "function".to_string(),
                visibility: "private".to_string(),
                confidence: 0.9,
                reason: "not called".to_string(),
            })
            .collect();
        let result = crate::analyzers::deadcode::Analysis {
            items,
            summary: crate::analyzers::deadcode::AnalysisSummary {
                total_items: 50,
                by_kind: std::collections::HashMap::new(),
                total_definitions: 100,
                reachable_count: 50,
            },
        };
        let score = calculate_deadcode_score(&result);
        assert!(score < 50.0);
    }

    #[test]
    fn test_score_to_grade_a() {
        assert_eq!(score_to_grade(95.0), "A");
        assert_eq!(score_to_grade(90.0), "A");
    }

    #[test]
    fn test_score_to_grade_b() {
        assert_eq!(score_to_grade(85.0), "B");
        assert_eq!(score_to_grade(80.0), "B");
    }

    #[test]
    fn test_score_to_grade_c() {
        assert_eq!(score_to_grade(75.0), "C");
        assert_eq!(score_to_grade(70.0), "C");
    }

    #[test]
    fn test_score_to_grade_d() {
        assert_eq!(score_to_grade(65.0), "D");
        assert_eq!(score_to_grade(60.0), "D");
    }

    #[test]
    fn test_score_to_grade_f() {
        assert_eq!(score_to_grade(55.0), "F");
        assert_eq!(score_to_grade(0.0), "F");
    }

    #[test]
    fn test_count_critical_issues_none() {
        let components = HashMap::new();
        assert_eq!(count_critical_issues(&components), 0);
    }

    #[test]
    fn test_count_critical_issues_some() {
        let mut components = HashMap::new();
        components.insert(
            "good".to_string(),
            ScoreComponent {
                score: 80.0,
                weight: 1.0,
                details: "ok".to_string(),
            },
        );
        components.insert(
            "bad".to_string(),
            ScoreComponent {
                score: 30.0,
                weight: 1.0,
                details: "critical".to_string(),
            },
        );
        assert_eq!(count_critical_issues(&components), 1);
    }

    #[test]
    fn test_analysis_summary_default() {
        let summary = AnalysisSummary::default();
        assert_eq!(summary.files_analyzed, 0);
        assert_eq!(summary.analyzers_run, 0);
        assert_eq!(summary.critical_issues, 0);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            overall_score: 85.0,
            grade: "B".to_string(),
            components: HashMap::new(),
            summary: AnalysisSummary {
                files_analyzed: 10,
                analyzers_run: 3,
                critical_issues: 0,
            },
        };
        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("\"overall_score\":85.0"));
        assert!(json.contains("\"grade\":\"B\""));
    }

    #[test]
    fn test_score_component_serialization() {
        let component = ScoreComponent {
            score: 90.0,
            weight: 1.0,
            details: "test details".to_string(),
        };
        let json = serde_json::to_string(&component).unwrap();
        assert!(json.contains("\"score\":90.0"));
        assert!(json.contains("\"weight\":1.0"));
        assert!(json.contains("test details"));
    }

    #[test]
    fn test_score_weights_serialization() {
        let weights = ScoreWeights::default();
        let json = serde_json::to_string(&weights).unwrap();
        assert!(json.contains("\"complexity\":1.0"));
        assert!(json.contains("\"satd\":0.8"));
    }
}
