//! Temporal coupling analysis.

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Temporal coupling analyzer.
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
        "temporal"
    }

    fn description(&self) -> &'static str {
        "Find files that change together (hidden dependencies)"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement temporal coupling analysis
        Ok(Analysis {
            couplings: Vec::new(),
            summary: AnalysisSummary::default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub couplings: Vec<TemporalCoupling>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalCoupling {
    pub file_a: String,
    pub file_b: String,
    pub co_change_count: u32,
    pub coupling_strength: f64,
    pub has_import_relationship: bool,
    pub coupling_type: CouplingType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CouplingType {
    Explicit,
    Hidden,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_couplings: usize,
    pub hidden_couplings: usize,
    pub avg_coupling_strength: f64,
}
