//! Complexity analyzer - cyclomatic and cognitive complexity.
//!
//! # Overview
//!
//! This analyzer computes two types of complexity metrics:
//!
//! - **Cyclomatic Complexity**: Counts the number of linearly independent paths through code.
//!   Based on McCabe's 1976 paper.
//!
//! - **Cognitive Complexity**: Measures how hard code is to understand, with penalties
//!   for nesting. Based on SonarSource's methodology.
//!
//! # Example
//!
//! ```no_run
//! use omen::analyzers::complexity::Analyzer;
//! use omen::core::{AnalysisContext, Analyzer as AnalyzerTrait, FileSet};
//! use omen::config::Config;
//!
//! let config = Config::default();
//! let files = FileSet::from_path(".", &config).unwrap();
//! let ctx = AnalysisContext::new(&files, &config, None);
//!
//! let analyzer = Analyzer::new();
//! let result = analyzer.analyze(&ctx).unwrap();
//! println!("Analyzed {} functions", result.summary.total_functions);
//! ```

use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::queries::{
    get_decision_node_types, get_flat_node_types, get_nesting_node_types,
};
use crate::parser::{self, ParseResult, Parser};

/// Complexity analyzer.
pub struct Analyzer {
    parser: Parser,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new complexity analyzer.
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    /// Analyze complexity for a single file.
    pub fn analyze_file(&self, path: &std::path::Path) -> Result<FileResult> {
        let result = self.parser.parse_file(path)?;
        Ok(analyze_parse_result(&result))
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "complexity"
    }

    fn description(&self) -> &'static str {
        "Calculate cyclomatic and cognitive complexity per function"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        let results: Vec<FileResult> = ctx
            .files
            .iter()
            .par_bridge()
            .filter_map(|path| {
                ctx.report_progress(0, ctx.files.len());
                self.analyze_file(path).ok()
            })
            .collect();

        let summary = build_summary(&results);
        let analysis = Analysis {
            files: results,
            summary,
        };

        tracing::info!(
            "Complexity analysis completed in {:?}: {} files, {} functions",
            start.elapsed(),
            analysis.summary.total_files,
            analysis.summary.total_functions
        );

        Ok(analysis)
    }
}

/// Full complexity analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// Per-file results.
    pub files: Vec<FileResult>,
    /// Aggregate summary.
    pub summary: AnalysisSummary,
}

/// Per-file complexity result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResult {
    /// File path.
    pub path: String,
    /// Detected language.
    pub language: String,
    /// Per-function results.
    pub functions: Vec<FunctionResult>,
    /// Total cyclomatic complexity.
    pub total_cyclomatic: u32,
    /// Total cognitive complexity.
    pub total_cognitive: u32,
    /// Average cyclomatic complexity.
    pub avg_cyclomatic: f64,
    /// Average cognitive complexity.
    pub avg_cognitive: f64,
}

/// Per-function complexity result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResult {
    /// Function name.
    pub name: String,
    /// File path.
    pub file: String,
    /// Start line (1-indexed).
    pub start_line: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// Complexity metrics.
    pub metrics: Metrics,
}

/// Complexity metrics for a function.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    /// Cyclomatic complexity.
    pub cyclomatic: u32,
    /// Cognitive complexity.
    pub cognitive: u32,
    /// Maximum nesting depth.
    pub max_nesting: u32,
    /// Number of lines.
    pub lines: u32,
}

/// Analysis summary statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    /// Total files analyzed.
    pub total_files: usize,
    /// Total functions analyzed.
    pub total_functions: usize,
    /// Average cyclomatic complexity.
    pub avg_cyclomatic: f64,
    /// Average cognitive complexity.
    pub avg_cognitive: f64,
    /// Maximum cyclomatic complexity.
    pub max_cyclomatic: u32,
    /// Maximum cognitive complexity.
    pub max_cognitive: u32,
    /// P50 cyclomatic complexity.
    pub p50_cyclomatic: u32,
    /// P90 cyclomatic complexity.
    pub p90_cyclomatic: u32,
    /// P95 cyclomatic complexity.
    pub p95_cyclomatic: u32,
    /// P50 cognitive complexity.
    pub p50_cognitive: u32,
    /// P90 cognitive complexity.
    pub p90_cognitive: u32,
    /// P95 cognitive complexity.
    pub p95_cognitive: u32,
}

/// Analyze a parsed file and extract complexity metrics.
fn analyze_parse_result(result: &ParseResult) -> FileResult {
    let functions = parser::extract_functions(result);
    let mut file_result = FileResult {
        path: result.path.to_string_lossy().to_string(),
        language: result.language.to_string(),
        functions: Vec::with_capacity(functions.len()),
        total_cyclomatic: 0,
        total_cognitive: 0,
        avg_cyclomatic: 0.0,
        avg_cognitive: 0.0,
    };

    for func in functions {
        let metrics = analyze_function_complexity(&func, result);
        file_result.total_cyclomatic += metrics.cyclomatic;
        file_result.total_cognitive += metrics.cognitive;

        file_result.functions.push(FunctionResult {
            name: func.name,
            file: result.path.to_string_lossy().to_string(),
            start_line: func.start_line,
            end_line: func.end_line,
            metrics,
        });
    }

    if !file_result.functions.is_empty() {
        let count = file_result.functions.len() as f64;
        file_result.avg_cyclomatic = file_result.total_cyclomatic as f64 / count;
        file_result.avg_cognitive = file_result.total_cognitive as f64 / count;
    }

    file_result
}

/// Analyze complexity for a single function.
fn analyze_function_complexity(func: &parser::FunctionNode, result: &ParseResult) -> Metrics {
    let root = result.root_node();

    // Find the function node in the tree
    let func_node = find_function_at_line(&root, func.start_line);

    let (cyclomatic, cognitive, max_nesting) = if let Some(node) = func_node {
        let body = node.child_by_field_name("body").unwrap_or(node);
        (
            1 + count_decision_points(&body, &result.source, result.language),
            calculate_cognitive_complexity(&body, &result.source, result.language, 0),
            calculate_max_nesting(&body, &result.source, 0),
        )
    } else {
        (1, 0, 0)
    };

    Metrics {
        cyclomatic,
        cognitive,
        max_nesting,
        lines: func.end_line.saturating_sub(func.start_line) + 1,
    }
}

/// Find a function node at a specific line.
fn find_function_at_line<'a>(
    root: &tree_sitter::Node<'a>,
    target_line: u32,
) -> Option<tree_sitter::Node<'a>> {
    let line = target_line.saturating_sub(1); // Convert to 0-indexed

    fn search(node: tree_sitter::Node<'_>, line: u32) -> Option<tree_sitter::Node<'_>> {
        let start = node.start_position().row as u32;
        let end = node.end_position().row as u32;

        if start <= line && line <= end {
            // Check if this is a function node
            let kind = node.kind();
            if kind.contains("function") || kind.contains("method") || kind == "impl_item" {
                return Some(node);
            }

            // Search children
            for child in node.children(&mut node.walk()) {
                if let Some(found) = search(child, line) {
                    return Some(found);
                }
            }
        }

        None
    }

    search(*root, line)
}

/// Count decision points for cyclomatic complexity.
fn count_decision_points(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> u32 {
    let decision_types: std::collections::HashSet<_> =
        get_decision_node_types(lang).iter().copied().collect();
    let mut count = 0;

    fn visit(
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        decision_types: &std::collections::HashSet<&str>,
        count: &mut u32,
    ) {
        let kind = node.kind();

        if decision_types.contains(kind) {
            *count += 1;
        }

        // Count logical operators as additional decision points
        if kind == "binary_expression" || kind == "logical_expression" {
            if let Some(op) = get_operator(node, source) {
                if op == "&&" || op == "||" || op == "and" || op == "or" {
                    *count += 1;
                }
            }
        }

        for child in node.children(&mut node.walk()) {
            visit(&child, source, decision_types, count);
        }
    }

    visit(node, source, &decision_types, &mut count);
    count
}

/// Calculate cognitive complexity with nesting penalties.
fn calculate_cognitive_complexity(
    node: &tree_sitter::Node<'_>,
    _source: &[u8],
    lang: Language,
    depth: u32,
) -> u32 {
    let nesting_types: std::collections::HashSet<_> =
        get_nesting_node_types(lang).iter().copied().collect();
    let flat_types: std::collections::HashSet<_> =
        get_flat_node_types(lang).iter().copied().collect();

    let mut complexity = 0;

    for child in node.children(&mut node.walk()) {
        let kind = child.kind();

        if nesting_types.contains(kind) {
            // Nesting construct: add base + depth penalty
            complexity += 1 + depth;
            complexity += calculate_cognitive_complexity(&child, _source, lang, depth + 1);
        } else if flat_types.contains(kind) {
            // Flat construct: add base + depth penalty without increasing depth
            complexity += 1 + depth;
            complexity += calculate_cognitive_complexity(&child, _source, lang, depth);
        } else {
            // Continue at same depth
            complexity += calculate_cognitive_complexity(&child, _source, lang, depth);
        }
    }

    complexity
}

/// Calculate maximum nesting depth.
fn calculate_max_nesting(node: &tree_sitter::Node<'_>, _source: &[u8], current_depth: u32) -> u32 {
    let nesting_kinds = [
        "if_statement",
        "if_expression",
        "if",
        "unless",
        "while_statement",
        "while_expression",
        "while",
        "until",
        "for_statement",
        "for_expression",
        "for",
        "switch_statement",
        "match_expression",
        "case",
        "try_statement",
        "begin",
    ];

    let mut max_depth = current_depth;

    for child in node.children(&mut node.walk()) {
        let kind = child.kind();
        let child_depth = if nesting_kinds.contains(&kind) {
            calculate_max_nesting(&child, _source, current_depth + 1)
        } else {
            calculate_max_nesting(&child, _source, current_depth)
        };

        if child_depth > max_depth {
            max_depth = child_depth;
        }
    }

    max_depth
}

/// Get the operator from a binary expression.
fn get_operator(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    for child in node.children(&mut node.walk()) {
        let kind = child.kind();
        if kind == "&&" || kind == "||" || kind == "and" || kind == "or" {
            return Some(kind.to_string());
        }
        if kind == "operator" {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
    }
    None
}

/// Build summary statistics from file results.
fn build_summary(results: &[FileResult]) -> AnalysisSummary {
    let mut summary = AnalysisSummary {
        total_files: results.len(),
        ..Default::default()
    };

    let mut all_cyclomatic = Vec::new();
    let mut all_cognitive = Vec::new();
    let mut total_cyclomatic: u64 = 0;
    let mut total_cognitive: u64 = 0;

    for file in results {
        summary.total_functions += file.functions.len();

        for func in &file.functions {
            all_cyclomatic.push(func.metrics.cyclomatic);
            all_cognitive.push(func.metrics.cognitive);
            total_cyclomatic += func.metrics.cyclomatic as u64;
            total_cognitive += func.metrics.cognitive as u64;

            if func.metrics.cyclomatic > summary.max_cyclomatic {
                summary.max_cyclomatic = func.metrics.cyclomatic;
            }
            if func.metrics.cognitive > summary.max_cognitive {
                summary.max_cognitive = func.metrics.cognitive;
            }
        }
    }

    if summary.total_functions > 0 {
        summary.avg_cyclomatic = total_cyclomatic as f64 / summary.total_functions as f64;
        summary.avg_cognitive = total_cognitive as f64 / summary.total_functions as f64;
    }

    // Calculate percentiles
    if !all_cyclomatic.is_empty() {
        all_cyclomatic.sort_unstable();
        all_cognitive.sort_unstable();

        summary.p50_cyclomatic = percentile(&all_cyclomatic, 50);
        summary.p90_cyclomatic = percentile(&all_cyclomatic, 90);
        summary.p95_cyclomatic = percentile(&all_cyclomatic, 95);
        summary.p50_cognitive = percentile(&all_cognitive, 50);
        summary.p90_cognitive = percentile(&all_cognitive, 90);
        summary.p95_cognitive = percentile(&all_cognitive, 95);
    }

    summary
}

/// Calculate percentile value from sorted slice.
fn percentile(sorted: &[u32], p: usize) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (p * sorted.len()) / 100;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_default() {
        let metrics = Metrics::default();
        assert_eq!(metrics.cyclomatic, 0);
        assert_eq!(metrics.cognitive, 0);
    }

    #[test]
    fn test_percentile() {
        let sorted = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        // p=50 -> idx=(50*10)/100=5 -> sorted[5]=6
        assert_eq!(percentile(&sorted, 50), 6);
        // p=90 -> idx=(90*10)/100=9 -> sorted[9]=10
        assert_eq!(percentile(&sorted, 90), 10);
    }

    #[test]
    fn test_percentile_empty() {
        let sorted: Vec<u32> = vec![];
        assert_eq!(percentile(&sorted, 50), 0);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "complexity");
    }
}
