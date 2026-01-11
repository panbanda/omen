//! Composite health score analyzer.

pub mod trend;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

pub use trend::analyze_trend;

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

        // Churn score (requires git)
        if self.weights.churn > 0.0 {
            let churn_analyzer = crate::analyzers::churn::Analyzer::new();
            if let Ok(result) = churn_analyzer.analyze(ctx) {
                let score = calculate_churn_score(&result);
                components.insert(
                    "churn".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.churn,
                        details: format!(
                            "Analyzed {} files, mean churn: {:.2}",
                            result.summary.total_files_changed, result.summary.mean_churn_score
                        ),
                    },
                );
                weighted_sum += score * self.weights.churn;
                total_weight += self.weights.churn;
            }
        }

        // Duplicates score (named "duplication" for template compatibility)
        if self.weights.duplicates > 0.0 {
            let dup_analyzer = crate::analyzers::duplicates::Analyzer::new();
            if let Ok(result) = dup_analyzer.analyze(ctx) {
                let score = calculate_duplicates_score(&result);
                components.insert(
                    "duplication".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.duplicates,
                        details: format!(
                            "Found {} clones, {:.1}% duplication",
                            result.summary.total_clones,
                            result.summary.duplication_ratio * 100.0
                        ),
                    },
                );
                weighted_sum += score * self.weights.duplicates;
                total_weight += self.weights.duplicates;
            }
        }

        // Cohesion score
        if self.weights.cohesion > 0.0 {
            let cohesion_analyzer = crate::analyzers::cohesion::Analyzer::new();
            if let Ok(result) = cohesion_analyzer.analyze(ctx) {
                let score = calculate_cohesion_score(&result);
                components.insert(
                    "cohesion".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.cohesion,
                        details: format!(
                            "Analyzed {} classes, avg LCOM: {:.1}",
                            result.summary.total_classes, result.summary.avg_lcom
                        ),
                    },
                );
                weighted_sum += score * self.weights.cohesion;
                total_weight += self.weights.cohesion;
            }
        }

        // Ownership score (requires git)
        if self.weights.ownership > 0.0 {
            let ownership_analyzer = crate::analyzers::ownership::Analyzer::new();
            if let Ok(result) = ownership_analyzer.analyze(ctx) {
                let score = calculate_ownership_score(&result);
                components.insert(
                    "ownership".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.ownership,
                        details: format!(
                            "Bus factor: {}, {} knowledge silos",
                            result.summary.bus_factor, result.summary.silo_count
                        ),
                    },
                );
                weighted_sum += score * self.weights.ownership;
                total_weight += self.weights.ownership;
            }
        }

        // Defect score (requires git)
        if self.weights.defect > 0.0 {
            let defect_analyzer = crate::analyzers::defect::Analyzer::new();
            if let Ok(result) = defect_analyzer.analyze(ctx) {
                let score = calculate_defect_score(&result);
                components.insert(
                    "defect".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.defect,
                        details: format!(
                            "{} high-risk files, avg probability: {:.1}%",
                            result.summary.high_risk_count,
                            result.summary.avg_probability * 100.0
                        ),
                    },
                );
                weighted_sum += score * self.weights.defect;
                total_weight += self.weights.defect;
            }
        }

        // TDG (Technical Debt Gradient) score
        if self.weights.tdg > 0.0 {
            let tdg_analyzer = crate::analyzers::tdg::Analyzer::new();
            if let Ok(result) = tdg_analyzer.analyze(ctx) {
                let score = calculate_tdg_score(&result);
                components.insert(
                    "tdg".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.tdg,
                        details: format!(
                            "Analyzed {} files, avg grade: {:?}",
                            result.total_files, result.average_grade
                        ),
                    },
                );
                weighted_sum += score * self.weights.tdg;
                total_weight += self.weights.tdg;
            }
        }

        // Coupling score (from graph analyzer)
        if self.weights.coupling > 0.0 {
            let graph_analyzer = crate::analyzers::graph::Analyzer::new();
            if let Ok(result) = graph_analyzer.analyze(ctx) {
                let score = calculate_coupling_score(&result);
                components.insert(
                    "coupling".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.coupling,
                        details: format!(
                            "{} nodes, {} cycles, avg degree: {:.1}",
                            result.summary.total_nodes,
                            result.summary.cycle_count,
                            result.summary.avg_degree
                        ),
                    },
                );
                weighted_sum += score * self.weights.coupling;
                total_weight += self.weights.coupling;
            }
        }

        // Smells score
        if self.weights.smells > 0.0 {
            let smells_analyzer = crate::analyzers::smells::Analyzer::new();
            if let Ok(result) = smells_analyzer.analyze(ctx) {
                let score = calculate_smells_score(&result);
                components.insert(
                    "smells".to_string(),
                    ScoreComponent {
                        score,
                        weight: self.weights.smells,
                        details: format!(
                            "{} smells ({} critical, {} high)",
                            result.summary.total_smells,
                            result.summary.critical_count,
                            result.summary.high_count
                        ),
                    },
                );
                weighted_sum += score * self.weights.smells;
                total_weight += self.weights.smells;
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

fn calculate_churn_score(result: &crate::analyzers::churn::Analysis) -> f64 {
    // Lower mean churn score = higher health score
    // Churn scores typically range 0-1+
    let mean = result.summary.mean_churn_score;
    if mean <= 0.1 {
        100.0
    } else if mean <= 0.3 {
        80.0 + (0.3 - mean) * 100.0
    } else if mean <= 0.5 {
        60.0 + (0.5 - mean) * 100.0
    } else if mean <= 0.8 {
        40.0 + (0.8 - mean) * 66.67
    } else {
        (40.0 - (mean - 0.8) * 50.0).max(0.0)
    }
}

fn calculate_duplicates_score(result: &crate::analyzers::duplicates::Analysis) -> f64 {
    // Lower duplication ratio = higher score
    // duplication_ratio: 0 = no duplication, 0.2 = 20% duplicated
    let ratio = result.summary.duplication_ratio;
    if ratio <= 0.0 {
        100.0
    } else if ratio <= 0.05 {
        90.0 + (0.05 - ratio) * 200.0
    } else if ratio <= 0.10 {
        70.0 + (0.10 - ratio) * 400.0
    } else if ratio <= 0.20 {
        50.0 + (0.20 - ratio) * 200.0
    } else {
        (50.0 - (ratio - 0.20) * 100.0).max(0.0)
    }
}

fn calculate_cohesion_score(result: &crate::analyzers::cohesion::Analysis) -> f64 {
    // Lower LCOM (Lack of Cohesion of Methods) = better cohesion = higher score
    // LCOM typically 0-10+
    let lcom = result.summary.avg_lcom;
    if lcom <= 1.0 {
        100.0
    } else if lcom <= 2.0 {
        80.0 + (2.0 - lcom) * 20.0
    } else if lcom <= 5.0 {
        50.0 + (5.0 - lcom) * 10.0
    } else {
        (50.0 - (lcom - 5.0) * 5.0).max(0.0)
    }
}

fn calculate_ownership_score(result: &crate::analyzers::ownership::Analysis) -> f64 {
    // Higher bus factor and lower silo count = better ownership = higher score
    let bus_factor = result.summary.bus_factor;
    let silo_ratio = if result.summary.total_files > 0 {
        result.summary.silo_count as f64 / result.summary.total_files as f64
    } else {
        0.0
    };

    // Bus factor contribution (0-50 points)
    let bus_score = match bus_factor {
        0..=1 => 20.0,
        2..=3 => 35.0,
        4..=5 => 45.0,
        _ => 50.0,
    };

    // Silo ratio contribution (0-50 points, fewer silos = higher score)
    let silo_score = if silo_ratio <= 0.1 {
        50.0
    } else if silo_ratio <= 0.3 {
        30.0 + (0.3 - silo_ratio) * 100.0
    } else if silo_ratio <= 0.5 {
        10.0 + (0.5 - silo_ratio) * 100.0
    } else {
        (10.0 - (silo_ratio - 0.5) * 20.0).max(0.0)
    };

    bus_score + silo_score
}

fn calculate_defect_score(result: &crate::analyzers::defect::Analysis) -> f64 {
    // Lower average probability = fewer predicted defects = higher score
    let avg_prob = result.summary.avg_probability as f64;
    if avg_prob <= 0.1 {
        100.0
    } else if avg_prob <= 0.3 {
        80.0 + (0.3 - avg_prob) * 100.0
    } else if avg_prob <= 0.5 {
        60.0 + (0.5 - avg_prob) * 100.0
    } else if avg_prob <= 0.7 {
        40.0 + (0.7 - avg_prob) * 100.0
    } else {
        (40.0 - (avg_prob - 0.7) * 100.0).max(0.0)
    }
}

fn calculate_tdg_score(result: &crate::analyzers::tdg::Analysis) -> f64 {
    // TDG average_score is typically 0-100, higher is better
    // Convert to health score: higher TDG score = higher health
    let avg = result.average_score as f64;
    avg.clamp(0.0, 100.0)
}

fn calculate_coupling_score(result: &crate::analyzers::graph::Analysis) -> f64 {
    // Score based on cycles and average degree
    // Fewer cycles and lower avg degree = better coupling = higher score
    let cycle_count = result.summary.cycle_count;
    let avg_degree = result.summary.avg_degree;

    // Cycle penalty: 0 cycles = 50 points, each cycle reduces by 5 (min 0)
    let cycle_score = (50.0 - cycle_count as f64 * 5.0).max(0.0);

    // Degree penalty: avg degree 0-2 = 50 points, 2-5 = 30-50, 5+ = <30
    let degree_score = if avg_degree <= 2.0 {
        50.0
    } else if avg_degree <= 5.0 {
        30.0 + (5.0 - avg_degree) * 6.67
    } else if avg_degree <= 10.0 {
        10.0 + (10.0 - avg_degree) * 4.0
    } else {
        (10.0 - (avg_degree - 10.0)).max(0.0)
    };

    cycle_score + degree_score
}

fn calculate_smells_score(result: &crate::analyzers::smells::Analysis) -> f64 {
    // Score based on smell count and severity
    // Fewer smells and lower severity = higher score
    let total = result.summary.total_smells;
    let critical = result.summary.critical_count;
    let high = result.summary.high_count;

    if total == 0 {
        return 100.0;
    }

    // Base score: 100 - (smells * penalty)
    // Critical smells have 10x penalty, high smells have 5x penalty
    let weighted_count =
        critical as f64 * 10.0 + high as f64 * 5.0 + (total - critical - high) as f64;
    let penalty = weighted_count * 2.0;

    (100.0 - penalty).max(0.0)
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
    pub tdg: f64,
    pub coupling: f64,
    pub smells: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        // Weights based on 3.x report component importance:
        // Complexity 25%, Duplication 20%, Cohesion 15%, TDG 15%,
        // Known Debt 10%, Coupling 10%, Smells 5%
        Self {
            complexity: 1.0, // 25% - highest priority
            duplicates: 0.8, // 20%
            cohesion: 0.6,   // 15%
            tdg: 0.6,        // 15%
            satd: 0.4,       // 10%
            coupling: 0.4,   // 10%
            smells: 0.2,     // 5%
            deadcode: 0.0,   // Not in 3.x display
            churn: 0.0,      // Not in 3.x display
            defect: 0.0,     // Not in 3.x display
            ownership: 0.0,  // Not in 3.x display
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
            tdg: 0.5,
            coupling: 0.5,
            smells: 0.5,
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
        // Weights based on 3.x report: Complexity 25%, Duplication 20%, Cohesion 15%,
        // TDG 15%, Known Debt 10%, Coupling 10%, Smells 5%
        assert_eq!(weights.complexity, 1.0);
        assert_eq!(weights.duplicates, 0.8);
        assert_eq!(weights.cohesion, 0.6);
        assert_eq!(weights.tdg, 0.6);
        assert_eq!(weights.satd, 0.4);
        assert_eq!(weights.coupling, 0.4);
        assert_eq!(weights.smells, 0.2);
        // Disabled by default (not shown in 3.x report)
        assert_eq!(weights.deadcode, 0.0);
        assert_eq!(weights.churn, 0.0);
        assert_eq!(weights.defect, 0.0);
        assert_eq!(weights.ownership, 0.0);
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
        assert!(json.contains("\"satd\":0.4"));
        assert!(json.contains("\"tdg\":0.6"));
        assert!(json.contains("\"coupling\":0.4"));
        assert!(json.contains("\"smells\":0.2"));
    }

    #[test]
    fn test_calculate_churn_score_low() {
        use chrono::Utc;
        let result = crate::analyzers::churn::Analysis {
            generated_at: Utc::now(),
            period_days: 30,
            repository_root: ".".to_string(),
            files: vec![],
            summary: crate::analyzers::churn::Summary {
                total_file_changes: 0,
                total_files_changed: 0,
                total_additions: 0,
                total_deletions: 0,
                avg_commits_per_file: 0.0,
                max_churn_score: 0.0,
                mean_churn_score: 0.05,
                variance_churn_score: 0.0,
                stddev_churn_score: 0.0,
                p50_churn_score: 0.0,
                p95_churn_score: 0.0,
                hotspot_files: vec![],
                stable_files: vec![],
                author_contributions: std::collections::HashMap::new(),
            },
        };
        let score = calculate_churn_score(&result);
        assert_eq!(score, 100.0);
    }

    #[test]
    fn test_calculate_churn_score_medium() {
        use chrono::Utc;
        let result = crate::analyzers::churn::Analysis {
            generated_at: Utc::now(),
            period_days: 30,
            repository_root: ".".to_string(),
            files: vec![],
            summary: crate::analyzers::churn::Summary {
                total_file_changes: 100,
                total_files_changed: 50,
                total_additions: 500,
                total_deletions: 200,
                avg_commits_per_file: 2.0,
                max_churn_score: 0.8,
                mean_churn_score: 0.4,
                variance_churn_score: 0.1,
                stddev_churn_score: 0.32,
                p50_churn_score: 0.3,
                p95_churn_score: 0.7,
                hotspot_files: vec![],
                stable_files: vec![],
                author_contributions: std::collections::HashMap::new(),
            },
        };
        let score = calculate_churn_score(&result);
        assert!((60.0..=80.0).contains(&score));
    }

    #[test]
    fn test_calculate_churn_score_high() {
        use chrono::Utc;
        let result = crate::analyzers::churn::Analysis {
            generated_at: Utc::now(),
            period_days: 30,
            repository_root: ".".to_string(),
            files: vec![],
            summary: crate::analyzers::churn::Summary {
                total_file_changes: 1000,
                total_files_changed: 500,
                total_additions: 5000,
                total_deletions: 3000,
                avg_commits_per_file: 10.0,
                max_churn_score: 1.5,
                mean_churn_score: 0.9,
                variance_churn_score: 0.2,
                stddev_churn_score: 0.45,
                p50_churn_score: 0.8,
                p95_churn_score: 1.2,
                hotspot_files: vec![],
                stable_files: vec![],
                author_contributions: std::collections::HashMap::new(),
            },
        };
        let score = calculate_churn_score(&result);
        assert!(score < 40.0);
    }

    #[test]
    fn test_calculate_duplicates_score_none() {
        let result = crate::analyzers::duplicates::Analysis {
            clones: vec![],
            groups: vec![],
            summary: crate::analyzers::duplicates::AnalysisSummary {
                total_clones: 0,
                total_groups: 0,
                type1_count: 0,
                type2_count: 0,
                type3_count: 0,
                duplicated_lines: 0,
                total_lines: 1000,
                duplication_ratio: 0.0,
                file_occurrences: std::collections::HashMap::new(),
                avg_similarity: 0.0,
                p50_similarity: 0.0,
                p95_similarity: 0.0,
                hotspots: vec![],
            },
            total_files_scanned: 10,
            min_lines: 6,
            threshold: 0.8,
        };
        assert_eq!(calculate_duplicates_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_duplicates_score_low() {
        let result = crate::analyzers::duplicates::Analysis {
            clones: vec![],
            groups: vec![],
            summary: crate::analyzers::duplicates::AnalysisSummary {
                total_clones: 5,
                total_groups: 2,
                type1_count: 3,
                type2_count: 2,
                type3_count: 0,
                duplicated_lines: 30,
                total_lines: 1000,
                duplication_ratio: 0.03,
                file_occurrences: std::collections::HashMap::new(),
                avg_similarity: 0.85,
                p50_similarity: 0.84,
                p95_similarity: 0.95,
                hotspots: vec![],
            },
            total_files_scanned: 10,
            min_lines: 6,
            threshold: 0.8,
        };
        let score = calculate_duplicates_score(&result);
        assert!((90.0..=100.0).contains(&score));
    }

    #[test]
    fn test_calculate_duplicates_score_high() {
        let result = crate::analyzers::duplicates::Analysis {
            clones: vec![],
            groups: vec![],
            summary: crate::analyzers::duplicates::AnalysisSummary {
                total_clones: 100,
                total_groups: 20,
                type1_count: 50,
                type2_count: 30,
                type3_count: 20,
                duplicated_lines: 300,
                total_lines: 1000,
                duplication_ratio: 0.30,
                file_occurrences: std::collections::HashMap::new(),
                avg_similarity: 0.90,
                p50_similarity: 0.88,
                p95_similarity: 0.98,
                hotspots: vec![],
            },
            total_files_scanned: 10,
            min_lines: 6,
            threshold: 0.8,
        };
        let score = calculate_duplicates_score(&result);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_cohesion_score_high() {
        let result = crate::analyzers::cohesion::Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            classes: vec![],
            summary: crate::analyzers::cohesion::Summary {
                total_classes: 10,
                total_files: 5,
                avg_wmc: 5.0,
                avg_cbo: 3.0,
                avg_rfc: 8.0,
                avg_lcom: 0.5,
                max_wmc: 10,
                max_cbo: 6,
                max_rfc: 15,
                max_lcom: 2,
                max_dit: 3,
                low_cohesion_count: 1,
                violation_count: 0,
            },
        };
        assert_eq!(calculate_cohesion_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_cohesion_score_low() {
        let result = crate::analyzers::cohesion::Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            classes: vec![],
            summary: crate::analyzers::cohesion::Summary {
                total_classes: 10,
                total_files: 5,
                avg_wmc: 20.0,
                avg_cbo: 10.0,
                avg_rfc: 30.0,
                avg_lcom: 8.0,
                max_wmc: 50,
                max_cbo: 20,
                max_rfc: 60,
                max_lcom: 20,
                max_dit: 10,
                low_cohesion_count: 8,
                violation_count: 15,
            },
        };
        let score = calculate_cohesion_score(&result);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_ownership_score_good() {
        let result = crate::analyzers::ownership::Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            files: vec![],
            summary: crate::analyzers::ownership::Summary {
                total_files: 100,
                bus_factor: 6,
                silo_count: 5,
                high_risk_count: 2,
                avg_contributors: 4.0,
                max_concentration: 0.5,
                top_contributors: vec![],
            },
        };
        let score = calculate_ownership_score(&result);
        assert!(score >= 80.0);
    }

    #[test]
    fn test_calculate_ownership_score_poor() {
        let result = crate::analyzers::ownership::Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            files: vec![],
            summary: crate::analyzers::ownership::Summary {
                total_files: 100,
                bus_factor: 1,
                silo_count: 60,
                high_risk_count: 40,
                avg_contributors: 1.2,
                max_concentration: 0.95,
                top_contributors: vec![],
            },
        };
        let score = calculate_ownership_score(&result);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_defect_score_low_risk() {
        let result = crate::analyzers::defect::Analysis {
            files: vec![],
            summary: crate::analyzers::defect::Summary {
                total_files: 100,
                high_risk_count: 0,
                medium_risk_count: 5,
                low_risk_count: 95,
                avg_probability: 0.05,
                p50_probability: 0.03,
                p95_probability: 0.15,
            },
            weights: crate::analyzers::defect::Weights::default(),
        };
        let score = calculate_defect_score(&result);
        assert_eq!(score, 100.0);
    }

    #[test]
    fn test_calculate_defect_score_high_risk() {
        let result = crate::analyzers::defect::Analysis {
            files: vec![],
            summary: crate::analyzers::defect::Summary {
                total_files: 100,
                high_risk_count: 40,
                medium_risk_count: 30,
                low_risk_count: 30,
                avg_probability: 0.75,
                p50_probability: 0.70,
                p95_probability: 0.95,
            },
            weights: crate::analyzers::defect::Weights::default(),
        };
        let score = calculate_defect_score(&result);
        assert!(score < 40.0);
    }
}
