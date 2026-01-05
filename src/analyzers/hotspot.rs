//! Hotspot analysis (churn x complexity).

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Hotspot analyzer.
#[derive(Default)]
pub struct Analyzer;

impl Analyzer {
    pub fn new() -> Self {
        Self
    }
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

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement hotspot analysis
        Ok(Analysis {
            hotspots: Vec::new(),
            summary: AnalysisSummary::default(),
        })
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Moderate,
    Low,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_hotspots: usize,
    pub critical_count: usize,
    pub high_count: usize,
}
