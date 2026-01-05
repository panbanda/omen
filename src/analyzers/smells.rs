//! Architectural smells analyzer.

use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Smells analyzer.
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
        "smells"
    }

    fn description(&self) -> &'static str {
        "Detect architecture anti-patterns"
    }

    fn analyze(&self, _ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // TODO: Implement smells detection
        Ok(Analysis {
            smells: Vec::new(),
            by_type: std::collections::HashMap::new(),
            summary: AnalysisSummary::default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub smells: Vec<Smell>,
    pub by_type: std::collections::HashMap<String, usize>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Smell {
    pub smell_type: SmellType,
    pub severity: Severity,
    pub files: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SmellType {
    CyclicDependency,
    GodClass,
    UnstableDependency,
    FeatureEnvy,
    Hub,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_smells: usize,
    pub high_severity: usize,
    pub medium_severity: usize,
}
