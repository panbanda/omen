//! Technical Debt Gradient (TDG) analyzer.

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// TDG analyzer.
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
        "tdg"
    }

    fn description(&self) -> &'static str {
        "Calculate Technical Debt Gradient scores (0-100 per file)"
    }

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement TDG analysis
        Ok(Analysis {
            files: Vec::new(),
            avg_score: 0.0,
            grade_distribution: GradeDistribution::default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub files: Vec<FileTdg>,
    pub avg_score: f64,
    pub grade_distribution: GradeDistribution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTdg {
    pub file: String,
    pub score: f64,
    pub grade: char,
    pub components: TdgComponents,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TdgComponents {
    pub structural: f64,
    pub semantic: f64,
    pub duplication: f64,
    pub coupling: f64,
    pub hotspot: f64,
    pub temporal: f64,
    pub consistency: f64,
    pub entropy: f64,
    pub documentation: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GradeDistribution {
    pub a: usize,
    pub b: usize,
    pub c: usize,
    pub d: usize,
    pub f: usize,
}
