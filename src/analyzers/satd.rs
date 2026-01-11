//! SATD (Self-Admitted Technical Debt) analyzer.
//!
//! Finds TODO, FIXME, HACK, and other debt markers in comments.

use std::time::Instant;

use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result, SourceFile};
use crate::parser::queries::satd;

/// SATD analyzer.
pub struct Analyzer {
    /// Compiled regex patterns for each category.
    patterns: Vec<(String, Regex, f64)>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new SATD analyzer.
    pub fn new() -> Self {
        let patterns = satd::all_categories()
            .iter()
            .map(|(category, markers, weight)| {
                let pattern = format!(r"(?i)\b({})\b", markers.join("|"));
                let regex = Regex::new(&pattern).expect("Invalid SATD pattern");
                (category.to_string(), regex, *weight)
            })
            .collect();

        Self { patterns }
    }

    /// Analyze a single file for SATD.
    pub fn analyze_file(&self, file: &SourceFile) -> Vec<SatdItem> {
        let content = file.content_str();
        let mut items = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            // Check if line is a comment
            if !is_comment_line(line) {
                continue;
            }

            for (category, regex, weight) in &self.patterns {
                if let Some(mat) = regex.find(line) {
                    items.push(SatdItem {
                        file: file.path.to_string_lossy().to_string(),
                        line: line_num as u32 + 1,
                        category: category.clone(),
                        severity: severity_from_weight(*weight),
                        marker: mat.as_str().to_uppercase(),
                        text: line.trim().chars().take(200).collect(),
                        weight: *weight,
                    });
                    break; // One category per line
                }
            }
        }

        items
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "satd"
    }

    fn description(&self) -> &'static str {
        "Find self-admitted technical debt (TODO/FIXME/HACK comments)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        // Single pass: collect SATD items and LOC simultaneously to avoid double file loading
        let (items, total_loc): (Vec<SatdItem>, usize) = ctx
            .files
            .iter()
            .par_bridge()
            .filter_map(|path| SourceFile::load(path).ok())
            .map(|file| {
                let loc = file.lines_of_code();
                let file_items = self.analyze_file(&file);
                (file_items, loc)
            })
            .reduce(
                || (Vec::new(), 0),
                |(mut items1, loc1), (items2, loc2)| {
                    items1.extend(items2);
                    (items1, loc1 + loc2)
                },
            );

        // Group by category
        let mut by_category = std::collections::HashMap::new();
        for item in &items {
            *by_category.entry(item.category.clone()).or_insert(0usize) += 1;
        }

        // Calculate weighted density
        let total_weight: f64 = items.iter().map(|i| i.weight).sum();

        let density = if total_loc > 0 {
            total_weight / (total_loc as f64 / 1000.0)
        } else {
            0.0
        };

        let total_items: usize = by_category.values().sum();
        let analysis = Analysis {
            items,
            by_category,
            density,
            summary: AnalysisSummary {
                total_items,
                weighted_count: total_weight,
                density,
            },
        };

        tracing::info!(
            "SATD analysis completed in {:?}: {} items, density {:.2}",
            start.elapsed(),
            analysis.summary.total_items,
            analysis.summary.density
        );

        Ok(analysis)
    }
}

/// Full SATD analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// All SATD items found.
    pub items: Vec<SatdItem>,
    /// Count by category.
    pub by_category: std::collections::HashMap<String, usize>,
    /// Weighted density per 1K LOC.
    pub density: f64,
    /// Summary statistics.
    pub summary: AnalysisSummary,
}

/// A single SATD item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatdItem {
    /// File path.
    pub file: String,
    /// Line number (1-indexed).
    pub line: u32,
    /// Category (design, defect, requirement, etc.).
    pub category: String,
    /// Severity level.
    pub severity: Severity,
    /// Matched marker (TODO, FIXME, etc.).
    pub marker: String,
    /// Comment text (truncated).
    pub text: String,
    /// Severity weight.
    #[serde(skip)]
    pub weight: f64,
}

/// SATD severity level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

/// Analysis summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    /// Total SATD items.
    pub total_items: usize,
    /// Weighted count.
    pub weighted_count: f64,
    /// Density per 1K LOC.
    pub density: f64,
}

/// Check if a line is a comment.
fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("'''")
        || trimmed.starts_with("\"\"\"")
        || trimmed.starts_with("--")
        || trimmed.starts_with(';')
}

/// Convert weight to severity level.
fn severity_from_weight(weight: f64) -> Severity {
    if weight >= 4.0 {
        Severity::Critical
    } else if weight >= 2.0 {
        Severity::High
    } else if weight >= 1.0 {
        Severity::Medium
    } else {
        Severity::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language;

    #[test]
    fn test_satd_detection() {
        let analyzer = Analyzer::new();
        let content = b"// TODO: implement this\n// FIXME: broken\nfn main() {}\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, "TODO");
        assert_eq!(items[1].marker, "FIXME");
    }

    #[test]
    fn test_satd_categories() {
        let analyzer = Analyzer::new();
        let content = b"// HACK: workaround\n// SECURITY: vulnerable\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].category, "design");
        assert_eq!(items[1].category, "security");
    }

    #[test]
    fn test_severity_from_weight() {
        assert_eq!(severity_from_weight(4.0), Severity::Critical);
        assert_eq!(severity_from_weight(2.0), Severity::High);
        assert_eq!(severity_from_weight(1.0), Severity::Medium);
        assert_eq!(severity_from_weight(0.25), Severity::Low);
    }
}
