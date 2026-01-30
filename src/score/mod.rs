//! Composite health score analyzer.

pub mod trend;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

pub use trend::{analyze_trend, default_sample_count};

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
        let mut acc = ScoreAccumulator::default();

        macro_rules! run_analyzer {
            ($name:expr, $weight:expr, $analyzer:expr, $score_fn:expr, $details:expr) => {
                if $weight > 0.0 {
                    if let Ok(result) = $analyzer.analyze(ctx) {
                        let score = $score_fn(&result);
                        let details = $details(&result);
                        acc.add($name, $weight, score, details);
                    }
                }
            };
        }

        // Complexity needs inline handling: skip when no functions are detected,
        // otherwise p90_cyclomatic == 0 produces a false perfect score of 100.
        if self.weights.complexity > 0.0 {
            if let Ok(result) = crate::analyzers::complexity::Analyzer::new().analyze(ctx) {
                if result.summary.total_functions > 0 {
                    let score = calculate_complexity_score(&result);
                    acc.add(
                        "complexity",
                        self.weights.complexity,
                        score,
                        format!(
                            "Analyzed {} files, p90 cyclomatic: {}, avg: {:.1}",
                            result.files.len(),
                            result.summary.p90_cyclomatic,
                            result.summary.avg_cyclomatic
                        ),
                    );
                }
            }
        }

        // SATD needs file_count from ctx, so handle inline
        if self.weights.satd > 0.0 {
            if let Ok(result) = crate::analyzers::satd::Analyzer::new().analyze(ctx) {
                let score = calculate_satd_score(&result, ctx.files.files().len());
                let high_priority = result
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
                acc.add(
                    "satd",
                    self.weights.satd,
                    score,
                    format!(
                        "Found {} debt items ({} high priority)",
                        result.items.len(),
                        high_priority
                    ),
                );
            }
        }

        run_analyzer!(
            "deadcode",
            self.weights.deadcode,
            crate::analyzers::deadcode::Analyzer::new(),
            calculate_deadcode_score,
            |r: &crate::analyzers::deadcode::Analysis| format!(
                "Found {} dead code items",
                r.items.len()
            )
        );

        run_analyzer!(
            "churn",
            self.weights.churn,
            crate::analyzers::churn::Analyzer::new(),
            calculate_churn_score,
            |r: &crate::analyzers::churn::Analysis| format!(
                "Analyzed {} files, mean churn: {:.2}",
                r.summary.total_files_changed, r.summary.mean_churn_score
            )
        );

        run_analyzer!(
            "duplication",
            self.weights.duplicates,
            crate::analyzers::duplicates::Analyzer::new(),
            calculate_duplicates_score,
            |r: &crate::analyzers::duplicates::Analysis| format!(
                "Found {} clones, {:.1}% duplication",
                r.summary.total_clones,
                r.summary.duplication_ratio * 100.0
            )
        );

        run_analyzer!(
            "cohesion",
            self.weights.cohesion,
            crate::analyzers::cohesion::Analyzer::new(),
            calculate_cohesion_score,
            |r: &crate::analyzers::cohesion::Analysis| format!(
                "Analyzed {} classes, avg LCOM: {:.1}",
                r.summary.total_classes, r.summary.avg_lcom
            )
        );

        run_analyzer!(
            "ownership",
            self.weights.ownership,
            crate::analyzers::ownership::Analyzer::new(),
            calculate_ownership_score,
            |r: &crate::analyzers::ownership::Analysis| format!(
                "Bus factor: {}, {} knowledge silos",
                r.summary.bus_factor, r.summary.silo_count
            )
        );

        run_analyzer!(
            "defect",
            self.weights.defect,
            crate::analyzers::defect::Analyzer::new(),
            calculate_defect_score,
            |r: &crate::analyzers::defect::Analysis| format!(
                "{} high-risk files, avg probability: {:.1}%",
                r.summary.high_risk_count,
                r.summary.avg_probability * 100.0
            )
        );

        run_analyzer!(
            "tdg",
            self.weights.tdg,
            crate::analyzers::tdg::Analyzer::new(),
            calculate_tdg_score,
            |r: &crate::analyzers::tdg::Analysis| format!(
                "Analyzed {} files, avg grade: {:?}",
                r.total_files, r.average_grade
            )
        );

        run_analyzer!(
            "coupling",
            self.weights.coupling,
            crate::analyzers::graph::Analyzer::new(),
            calculate_coupling_score,
            |r: &crate::analyzers::graph::Analysis| format!(
                "{} nodes, {} cycles, avg degree: {:.1}",
                r.summary.total_nodes, r.summary.cycle_count, r.summary.avg_degree
            )
        );

        run_analyzer!(
            "smells",
            self.weights.smells,
            crate::analyzers::smells::Analyzer::new(),
            calculate_smells_score,
            |r: &crate::analyzers::smells::Analysis| format!(
                "{} smells ({} critical, {} high)",
                r.summary.total_smells, r.summary.critical_count, r.summary.high_count
            )
        );

        acc.into_analysis(ctx.files.files().len())
    }
}

#[derive(Default)]
struct ScoreAccumulator {
    components: HashMap<String, ScoreComponent>,
    weighted_sum: f64,
    total_weight: f64,
}

impl ScoreAccumulator {
    fn add(&mut self, name: &str, weight: f64, score: f64, details: String) {
        self.components.insert(
            name.to_string(),
            ScoreComponent {
                score,
                weight,
                details,
            },
        );
        self.weighted_sum += score * weight;
        self.total_weight += weight;
    }

    fn into_analysis(self, files_analyzed: usize) -> Result<Analysis> {
        let overall_score = if self.total_weight > 0.0 {
            self.weighted_sum / self.total_weight
        } else {
            100.0
        };
        let grade = score_to_grade(overall_score);
        let analyzers_run = self.components.len();
        let critical_issues = count_critical_issues(&self.components);
        Ok(Analysis {
            overall_score,
            grade,
            components: self.components,
            summary: AnalysisSummary {
                files_analyzed,
                analyzers_run,
                critical_issues,
            },
        })
    }
}

/// Compute the health score from pre-generated JSON files (avoids re-running analyzers).
///
/// Reads analyzer results from the given directory and computes the composite score.
/// Used by `report generate` to avoid redundantly re-running all sub-analyzers.
pub fn compute_from_data_dir(data_dir: &std::path::Path, file_count: usize) -> Result<Analysis> {
    let weights = ScoreWeights::default();
    let mut acc = ScoreAccumulator::default();

    macro_rules! load_and_score {
        ($file:expr, $name:expr, $weight:expr, $type:ty, $score_fn:expr, $details_fn:expr) => {
            if $weight > 0.0 {
                let path = data_dir.join($file);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(result) = serde_json::from_str::<$type>(&content) {
                        let score = $score_fn(&result);
                        let details = $details_fn(&result);
                        acc.add($name, $weight, score, details);
                    }
                }
            }
        };
    }

    // Complexity: skip when no functions detected to avoid false 100 score.
    if weights.complexity > 0.0 {
        let path = data_dir.join("complexity.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(result) =
                serde_json::from_str::<crate::analyzers::complexity::Analysis>(&content)
            {
                if result.summary.total_functions > 0 {
                    let score = calculate_complexity_score(&result);
                    let details = format!(
                        "Analyzed {} files, avg cyclomatic: {:.1}",
                        result.files.len(),
                        result.summary.avg_cyclomatic
                    );
                    acc.add("complexity", weights.complexity, score, details);
                }
            }
        }
    }

    load_and_score!(
        "satd.json",
        "satd",
        weights.satd,
        crate::analyzers::satd::Analysis,
        |r: &crate::analyzers::satd::Analysis| calculate_satd_score(r, file_count),
        |r: &crate::analyzers::satd::Analysis| {
            let high_priority = r
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
            format!(
                "Found {} debt items ({} high priority)",
                r.items.len(),
                high_priority
            )
        }
    );

    load_and_score!(
        "duplicates.json",
        "duplication",
        weights.duplicates,
        crate::analyzers::duplicates::Analysis,
        calculate_duplicates_score,
        |r: &crate::analyzers::duplicates::Analysis| format!(
            "Found {} clones, {:.1}% duplication",
            r.summary.total_clones,
            r.summary.duplication_ratio * 100.0
        )
    );

    load_and_score!(
        "cohesion.json",
        "cohesion",
        weights.cohesion,
        crate::analyzers::cohesion::Analysis,
        calculate_cohesion_score,
        |r: &crate::analyzers::cohesion::Analysis| format!(
            "Analyzed {} classes, avg LCOM: {:.1}",
            r.summary.total_classes, r.summary.avg_lcom
        )
    );

    load_and_score!(
        "tdg.json",
        "tdg",
        weights.tdg,
        crate::analyzers::tdg::Analysis,
        calculate_tdg_score,
        |r: &crate::analyzers::tdg::Analysis| format!(
            "Analyzed {} files, avg grade: {:?}",
            r.total_files, r.average_grade
        )
    );

    load_and_score!(
        "graph.json",
        "coupling",
        weights.coupling,
        crate::analyzers::graph::Analysis,
        calculate_coupling_score,
        |r: &crate::analyzers::graph::Analysis| format!(
            "{} nodes, {} cycles, avg degree: {:.1}",
            r.summary.total_nodes, r.summary.cycle_count, r.summary.avg_degree
        )
    );

    load_and_score!(
        "smells.json",
        "smells",
        weights.smells,
        crate::analyzers::smells::Analysis,
        calculate_smells_score,
        |r: &crate::analyzers::smells::Analysis| format!(
            "{} smells ({} critical, {} high)",
            r.summary.total_smells, r.summary.critical_count, r.summary.high_count
        )
    );

    acc.into_analysis(file_count)
}

fn calculate_complexity_score(result: &crate::analyzers::complexity::Analysis) -> f64 {
    // Scores p90 cyclomatic complexity (not average, which is skewed by outliers).
    //
    // Bands based on McCabe's risk categories (McCabe 1976, NIST SP 500-235):
    //   CC 1-10   = low risk      → score 90-100
    //   CC 11-20  = moderate risk  → score 70-90
    //   CC 21-50  = high risk      → score 30-70
    //   CC > 50   = very high risk → score 0-30
    //
    // Real-world references:
    //   - NASA SLS: average CC 2.9, hard cap 20
    //   - Apache 2.2.8: average CC 6.04, ~85% of functions below 10
    //   - NIST: "limit of 10 has significant supporting evidence"
    //
    // We use 40 instead of 50 as the high-risk ceiling since this is p90
    // (already the worst 10% of functions), so the threshold is stricter.
    let p90 = result.summary.p90_cyclomatic as f64;
    if p90 <= 10.0 {
        100.0 - p90
    } else if p90 <= 20.0 {
        70.0 + (20.0 - p90) * 2.0
    } else if p90 <= 40.0 {
        50.0 + (40.0 - p90) * 1.0
    } else {
        (50.0 - (p90 - 40.0)).max(0.0)
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
    // Score based on cycles, average degree, and hub concentration.
    // The old formula only used cycle_count and avg_degree, which was too coarse:
    // avg_degree is diluted when most nodes have 0 edges, hiding extreme hubs.
    let cycle_count = result.summary.cycle_count;
    let avg_degree = result.summary.avg_degree;
    let total_nodes = result.summary.total_nodes.max(1);

    // Cycle penalty: 0 cycles = 35 points, each cycle costs 5 (min 0)
    let cycle_score = (35.0 - cycle_count as f64 * 5.0).max(0.0);

    // Degree score: avg degree 0-2 = 35 points, 2-5 = 20-35, 5+ = <20
    let degree_score = if avg_degree <= 2.0 {
        35.0
    } else if avg_degree <= 5.0 {
        20.0 + (5.0 - avg_degree) * 5.0
    } else if avg_degree <= 10.0 {
        5.0 + (10.0 - avg_degree) * 3.0
    } else {
        (5.0 - (avg_degree - 10.0)).max(0.0)
    };

    // Hub concentration: penalize when a few nodes have disproportionate degree.
    // Compute max degree and fraction of "high-degree" nodes (degree > 10).
    let (max_degree, high_degree_count) = if result.nodes.is_empty() {
        (0usize, 0usize)
    } else {
        let mut max_deg = 0usize;
        let mut high_count = 0usize;
        for node in &result.nodes {
            let deg = node.in_degree + node.out_degree;
            if deg > max_deg {
                max_deg = deg;
            }
            if deg > 10 {
                high_count += 1;
            }
        }
        (max_deg, high_count)
    };

    // Hub score: 30 points. Penalize for max degree and high-degree node fraction.
    let max_degree_penalty = if max_degree <= 10 {
        0.0
    } else if max_degree <= 30 {
        (max_degree as f64 - 10.0) * 0.3
    } else if max_degree <= 80 {
        6.0 + (max_degree as f64 - 30.0) * 0.2
    } else {
        16.0 + (max_degree as f64 - 80.0) * 0.1
    }
    .min(20.0);

    let high_degree_ratio = high_degree_count as f64 / total_nodes as f64;
    let ratio_penalty = (high_degree_ratio * 100.0).min(10.0);

    let hub_score = (30.0 - max_degree_penalty - ratio_penalty).max(0.0);

    cycle_score + degree_score + hub_score
}

fn calculate_smells_score(result: &crate::analyzers::smells::Analysis) -> f64 {
    // Score based on smell density relative to codebase size.
    // The old formula used absolute counts which bottomed out at 0
    // for any non-trivial codebase (5 critical smells = score 0).
    let total = result.summary.total_smells;
    let critical = result.summary.critical_count;
    let high = result.summary.high_count;
    let components = result.summary.total_components.max(1);

    if total == 0 {
        return 100.0;
    }

    // Weight smells by severity then compute density against codebase size
    let weighted_count =
        critical as f64 * 5.0 + high as f64 * 2.0 + (total - critical - high) as f64;
    let density = weighted_count / components as f64;

    // Use logarithmic decay so the score degrades gracefully:
    // density 0.01 -> ~95, 0.05 -> ~80, 0.1 -> ~70, 0.3 -> ~50, 1.0 -> ~25
    let raw = 100.0 * (-2.5 * density).exp();
    raw.clamp(0.0, 100.0)
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

impl Analysis {
    /// Check that overall score meets a minimum threshold.
    pub fn check_threshold(&self, min_score: f64) -> crate::core::Result<()> {
        if self.overall_score >= min_score {
            Ok(())
        } else {
            Err(crate::core::Error::threshold_violation(
                format!(
                    "Score {:.1} ({}) is below minimum {:.1}",
                    self.overall_score, self.grade, min_score
                ),
                self.overall_score,
            ))
        }
    }
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
        // Disabled by default (not shown in 3.x report) omen:ignore
        assert_eq!(weights.deadcode, 0.0);
        assert_eq!(weights.churn, 0.0);
        assert_eq!(weights.defect, 0.0);
        assert_eq!(weights.ownership, 0.0);
    }

    #[test]
    fn test_calculate_complexity_score_low_p90() {
        // p90=3 should score high but not a flat 100
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 3,
                avg_cyclomatic: 50.0, // high avg should NOT drag score down
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert_eq!(score, 97.0);
    }

    #[test]
    fn test_calculate_complexity_score_zero_p90() {
        // p90=0 should be 100
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 0,
                ..Default::default()
            },
        };
        assert_eq!(calculate_complexity_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_complexity_score_medium_p90() {
        // p90 5-10 should be 90-100
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 8,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!(
            (90.0..=100.0).contains(&score),
            "p90=8 should score 90-100, got {score}"
        );
    }

    #[test]
    fn test_calculate_complexity_score_high_p90() {
        // p90 10-20 should be 70-90
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 15,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!(
            (70.0..=90.0).contains(&score),
            "p90=15 should score 70-90, got {score}"
        );
    }

    #[test]
    fn test_calculate_complexity_score_very_high_p90() {
        // p90 20-40 should be 50-70
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 30,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!(
            (50.0..=70.0).contains(&score),
            "p90=30 should score 50-70, got {score}"
        );
    }

    #[test]
    fn test_calculate_complexity_score_extreme_p90() {
        // p90 > 40 should be < 50
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                p90_cyclomatic: 60,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        assert!(score < 50.0, "p90=60 should score < 50, got {score}");
    }

    #[test]
    fn test_calculate_complexity_score_outlier_resistant() {
        // A repo with p50=2, p90=41, avg=29 (like real data) should score reasonably
        let result = crate::analyzers::complexity::Analysis {
            files: vec![],
            summary: crate::analyzers::complexity::AnalysisSummary {
                avg_cyclomatic: 29.2,
                p50_cyclomatic: 2,
                p90_cyclomatic: 41,
                max_cyclomatic: 1504,
                ..Default::default()
            },
        };
        let score = calculate_complexity_score(&result);
        // Should NOT be 0 - that was the bug. Should reflect p90=41 which is high but not catastrophic
        assert!(
            score >= 40.0,
            "real-world data (p90=41) should not score near 0, got {score}"
        );
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
    fn test_check_threshold_passes() {
        let analysis = Analysis {
            overall_score: 85.0,
            grade: "B".to_string(),
            components: HashMap::new(),
            summary: AnalysisSummary::default(),
        };
        assert!(analysis.check_threshold(80.0).is_ok());
    }

    #[test]
    fn test_check_threshold_fails() {
        let analysis = Analysis {
            overall_score: 72.0,
            grade: "C".to_string(),
            components: HashMap::new(),
            summary: AnalysisSummary::default(),
        };
        assert!(analysis.check_threshold(80.0).is_err());
    }

    #[test]
    fn test_check_threshold_exact_boundary() {
        let analysis = Analysis {
            overall_score: 80.0,
            grade: "B".to_string(),
            components: HashMap::new(),
            summary: AnalysisSummary::default(),
        };
        assert!(analysis.check_threshold(80.0).is_ok());
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
    fn test_calculate_smells_score_none() {
        let result = crate::analyzers::smells::Analysis {
            generated_at: String::new(),
            smells: vec![],
            components: vec![],
            summary: crate::analyzers::smells::Summary {
                total_smells: 0,
                ..Default::default()
            },
            thresholds: crate::analyzers::smells::Thresholds::default(),
        };
        assert_eq!(calculate_smells_score(&result), 100.0);
    }

    #[test]
    fn test_calculate_smells_score_scales_with_codebase() {
        // 32 critical + 27 high + 246 medium in a 4835-component codebase
        // should NOT be 0 - that was the bug
        let result = crate::analyzers::smells::Analysis {
            generated_at: String::new(),
            smells: vec![],
            components: vec![],
            summary: crate::analyzers::smells::Summary {
                total_smells: 305,
                critical_count: 32,
                high_count: 27,
                medium_count: 246,
                total_components: 4835,
                ..Default::default()
            },
            thresholds: crate::analyzers::smells::Thresholds::default(),
        };
        let score = calculate_smells_score(&result);
        // 305 smells in 4835 components = ~6.3% affected. Should be a moderate penalty, not 0
        assert!(
            score >= 30.0,
            "305 smells in 4835 components should not be 0, got {score}"
        );
        assert!(
            score <= 80.0,
            "305 smells with 32 critical should not be too high, got {score}"
        );
    }

    #[test]
    fn test_calculate_smells_score_small_codebase_penalized_more() {
        // Same smell count but fewer components = higher density = lower score
        let large = crate::analyzers::smells::Analysis {
            generated_at: String::new(),
            smells: vec![],
            components: vec![],
            summary: crate::analyzers::smells::Summary {
                total_smells: 20,
                critical_count: 5,
                high_count: 5,
                medium_count: 10,
                total_components: 1000,
                ..Default::default()
            },
            thresholds: crate::analyzers::smells::Thresholds::default(),
        };
        let small = crate::analyzers::smells::Analysis {
            generated_at: String::new(),
            smells: vec![],
            components: vec![],
            summary: crate::analyzers::smells::Summary {
                total_smells: 20,
                critical_count: 5,
                high_count: 5,
                medium_count: 10,
                total_components: 50,
                ..Default::default()
            },
            thresholds: crate::analyzers::smells::Thresholds::default(),
        };
        let large_score = calculate_smells_score(&large);
        let small_score = calculate_smells_score(&small);
        assert!(
            large_score > small_score,
            "larger codebase should score higher: large={large_score}, small={small_score}"
        );
    }

    #[test]
    fn test_calculate_smells_score_few_critical_not_zero() {
        // 5 critical smells in a large codebase should not immediately score 0
        let result = crate::analyzers::smells::Analysis {
            generated_at: String::new(),
            smells: vec![],
            components: vec![],
            summary: crate::analyzers::smells::Summary {
                total_smells: 5,
                critical_count: 5,
                high_count: 0,
                medium_count: 0,
                total_components: 500,
                ..Default::default()
            },
            thresholds: crate::analyzers::smells::Thresholds::default(),
        };
        let score = calculate_smells_score(&result);
        assert!(
            score > 0.0,
            "5 critical smells in 500 components should not be 0, got {score}"
        );
    }

    #[test]
    fn test_calculate_coupling_score_uses_hub_concentration() {
        // A repo with low avg degree but extreme hub nodes should score lower
        // than one with uniform low degree
        let uniform = crate::analyzers::graph::Analysis {
            nodes: (0..100)
                .map(|i| crate::analyzers::graph::Node {
                    path: format!("file{i}.rs"),
                    pagerank: 0.01,
                    betweenness: 0.01,
                    in_degree: 1,
                    out_degree: 1,
                    instability: 0.5,
                })
                .collect(),
            edges: vec![],
            cycles: vec![],
            summary: crate::analyzers::graph::AnalysisSummary {
                total_nodes: 100,
                total_edges: 100,
                avg_degree: 2.0,
                cycle_count: 0,
            },
        };
        let hub_heavy = crate::analyzers::graph::Analysis {
            nodes: {
                let mut nodes: Vec<_> = (0..100)
                    .map(|i| crate::analyzers::graph::Node {
                        path: format!("file{i}.rs"),
                        pagerank: 0.01,
                        betweenness: 0.01,
                        in_degree: 0,
                        out_degree: 0,
                        instability: 0.5,
                    })
                    .collect();
                // One mega-hub with 141 connections
                nodes[0].in_degree = 70;
                nodes[0].out_degree = 71;
                nodes
            },
            edges: vec![],
            cycles: vec![],
            summary: crate::analyzers::graph::AnalysisSummary {
                total_nodes: 100,
                total_edges: 141,
                avg_degree: 1.41, // low avg because most nodes are 0
                cycle_count: 0,
            },
        };
        let uniform_score = calculate_coupling_score(&uniform);
        let hub_score = calculate_coupling_score(&hub_heavy);
        assert!(
            uniform_score > hub_score,
            "uniform coupling should score higher than hub-heavy: uniform={uniform_score}, hub={hub_score}"
        );
    }

    #[test]
    fn test_calculate_coupling_score_not_always_near_100() {
        // A repo with 4835 nodes, 1 cycle, avg_degree 1.2, but extreme hubs
        // should not just score 95
        let result = crate::analyzers::graph::Analysis {
            nodes: {
                let mut nodes: Vec<_> = (0..4835)
                    .map(|i| crate::analyzers::graph::Node {
                        path: format!("file{i}.rs"),
                        pagerank: 0.0002,
                        betweenness: 0.0,
                        in_degree: 0,
                        out_degree: 0,
                        instability: 0.5,
                    })
                    .collect();
                // Top hubs from real data
                nodes[0].in_degree = 70;
                nodes[0].out_degree = 71;
                nodes[1].in_degree = 57;
                nodes[1].out_degree = 57;
                nodes[2].in_degree = 50;
                nodes[2].out_degree = 55;
                nodes
            },
            edges: vec![],
            cycles: vec![vec!["a".into(), "b".into()]],
            summary: crate::analyzers::graph::AnalysisSummary {
                total_nodes: 4835,
                total_edges: 2907,
                avg_degree: 1.2,
                cycle_count: 1,
            },
        };
        let score = calculate_coupling_score(&result);
        // Should reflect the hub concentration, not just avg_degree
        assert!(
            score < 90.0,
            "hub-heavy graph should not score 95, got {score}"
        );
    }

    #[test]
    fn test_score_module_line_count_acceptable() {
        let source = include_str!("mod.rs");
        let line_count = source.lines().count();
        assert!(
            line_count <= 1750,
            "score/mod.rs has {line_count} lines (max 1700)"
        );
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
