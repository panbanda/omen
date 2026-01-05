//! Code ownership / bus factor analyzer.

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Ownership analyzer.
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
        "ownership"
    }

    fn description(&self) -> &'static str {
        "Analyze knowledge concentration and team risk"
    }

    fn requires_git(&self) -> bool {
        true
    }

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement ownership analysis
        Ok(Analysis {
            files: Vec::new(),
            summary: AnalysisSummary::default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub files: Vec<FileOwnership>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOwnership {
    pub file: String,
    pub primary_owner: String,
    pub ownership_ratio: f64,
    pub bus_factor: usize,
    pub risk_level: RiskLevel,
    pub contributors: Vec<Contributor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    pub name: String,
    pub lines: u32,
    pub percentage: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RiskLevel {
    High,
    Medium,
    Low,
    VeryLow,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_files: usize,
    pub high_risk_files: usize,
    pub avg_bus_factor: f64,
}
