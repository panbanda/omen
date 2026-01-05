//! Dead code detection analyzer.

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Dead code analyzer.
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
        "deadcode"
    }

    fn description(&self) -> &'static str {
        "Find unreachable/unused functions and variables"
    }

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement dead code detection
        Ok(Analysis {
            items: Vec::new(),
            summary: AnalysisSummary::default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub items: Vec<DeadCodeItem>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeItem {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_items: usize,
    pub by_kind: std::collections::HashMap<String, usize>,
}
