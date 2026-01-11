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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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

    /// Maximum file size to analyze (1MB). Larger files are likely minified bundles.
    const MAX_FILE_SIZE: u64 = 1_000_000;

    /// Analyze complexity for a single file.
    pub fn analyze_file(&self, path: &std::path::Path) -> Result<FileResult> {
        // Skip files that are too large (likely minified bundles)
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() > Self::MAX_FILE_SIZE {
                return Err(crate::core::Error::Parse {
                    path: path.to_path_buf(),
                    message: format!(
                        "File too large: {} bytes (max {})",
                        metadata.len(),
                        Self::MAX_FILE_SIZE
                    ),
                });
            }
        }
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
        let total_files = ctx.files.len();
        let counter = Arc::new(AtomicUsize::new(0));

        let results: Vec<FileResult> = ctx
            .files
            .files()
            .par_iter()
            .filter_map(|path| {
                let result = self.analyze_file(path).ok();

                // Report progress
                let current = counter.fetch_add(1, Ordering::Relaxed) + 1;
                ctx.report_progress(current, total_files);

                result
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

/// A function that violated complexity thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Function name.
    pub name: String,
    /// File path.
    pub file: String,
    /// Line number.
    pub line: u32,
    /// Cyclomatic complexity.
    pub cyclomatic: u32,
    /// Cognitive complexity.
    pub cognitive: u32,
}

impl Analysis {
    /// Check if any functions exceed the given thresholds.
    ///
    /// Returns Ok(()) if all functions are within thresholds.
    /// Returns Err with a list of violations if any function exceeds either threshold.
    pub fn check_thresholds(
        &self,
        max_cyclomatic: u32,
        max_cognitive: u32,
    ) -> std::result::Result<(), Vec<Violation>> {
        let violations: Vec<Violation> = self
            .files
            .iter()
            .flat_map(|file| &file.functions)
            .filter(|func| {
                func.metrics.cyclomatic > max_cyclomatic || func.metrics.cognitive > max_cognitive
            })
            .map(|func| Violation {
                name: func.name.clone(),
                file: func.file.clone(),
                line: func.start_line,
                cyclomatic: func.metrics.cyclomatic,
                cognitive: func.metrics.cognitive,
            })
            .collect();

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
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
    // Get the static slice of decision types - no allocation needed
    let decision_types = get_decision_node_types(lang);
    let mut count = 0;

    fn visit(
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        decision_types: &[&str],
        count: &mut u32,
    ) {
        let kind = node.kind();

        // Use slice contains - O(n) but n is small (typically < 15 items)
        if decision_types.contains(&kind) {
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

    visit(node, source, decision_types, &mut count);
    count
}

/// Calculate cognitive complexity with nesting penalties.
fn calculate_cognitive_complexity(
    node: &tree_sitter::Node<'_>,
    _source: &[u8],
    lang: Language,
    depth: u32,
) -> u32 {
    // Get static slices once - no allocation needed
    let nesting_types = get_nesting_node_types(lang);
    let flat_types = get_flat_node_types(lang);

    fn visit(
        node: &tree_sitter::Node<'_>,
        nesting_types: &[&str],
        flat_types: &[&str],
        depth: u32,
    ) -> u32 {
        let mut complexity = 0;

        for child in node.children(&mut node.walk()) {
            let kind = child.kind();

            // Use slice contains - O(n) but n is small (typically < 10 items)
            if nesting_types.contains(&kind) {
                // Nesting construct: add base + depth penalty
                complexity += 1 + depth;
                complexity += visit(&child, nesting_types, flat_types, depth + 1);
            } else if flat_types.contains(&kind) {
                // Flat construct: add base + depth penalty without increasing depth
                complexity += 1 + depth;
                complexity += visit(&child, nesting_types, flat_types, depth);
            } else {
                // Continue at same depth
                complexity += visit(&child, nesting_types, flat_types, depth);
            }
        }

        complexity
    }

    visit(node, nesting_types, flat_types, depth)
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
/// Returns a borrowed string slice to avoid allocation.
fn get_operator<'a>(node: &tree_sitter::Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        if kind == "&&" || kind == "||" || kind == "and" || kind == "or" {
            return Some(kind);
        }
        if kind == "operator" {
            return child.utf8_text(source).ok();
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
    fn test_check_thresholds_passes_when_under() {
        let analysis = Analysis {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                language: "rust".to_string(),
                functions: vec![FunctionResult {
                    name: "simple_fn".to_string(),
                    file: "test.rs".to_string(),
                    start_line: 1,
                    end_line: 5,
                    metrics: Metrics {
                        cyclomatic: 5,
                        cognitive: 3,
                        max_nesting: 1,
                        lines: 5,
                    },
                }],
                total_cyclomatic: 5,
                total_cognitive: 3,
                avg_cyclomatic: 5.0,
                avg_cognitive: 3.0,
            }],
            summary: AnalysisSummary::default(),
        };

        let result = analysis.check_thresholds(15, 15);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_thresholds_fails_when_cyclomatic_exceeded() {
        let analysis = Analysis {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                language: "rust".to_string(),
                functions: vec![FunctionResult {
                    name: "complex_fn".to_string(),
                    file: "test.rs".to_string(),
                    start_line: 1,
                    end_line: 50,
                    metrics: Metrics {
                        cyclomatic: 20,
                        cognitive: 5,
                        max_nesting: 3,
                        lines: 50,
                    },
                }],
                total_cyclomatic: 20,
                total_cognitive: 5,
                avg_cyclomatic: 20.0,
                avg_cognitive: 5.0,
            }],
            summary: AnalysisSummary::default(),
        };

        let result = analysis.check_thresholds(15, 15);
        assert!(result.is_err());
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].name, "complex_fn");
        assert_eq!(violations[0].cyclomatic, 20);
    }

    #[test]
    fn test_check_thresholds_fails_when_cognitive_exceeded() {
        let analysis = Analysis {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                language: "rust".to_string(),
                functions: vec![FunctionResult {
                    name: "nested_fn".to_string(),
                    file: "test.rs".to_string(),
                    start_line: 1,
                    end_line: 30,
                    metrics: Metrics {
                        cyclomatic: 5,
                        cognitive: 25,
                        max_nesting: 5,
                        lines: 30,
                    },
                }],
                total_cyclomatic: 5,
                total_cognitive: 25,
                avg_cyclomatic: 5.0,
                avg_cognitive: 25.0,
            }],
            summary: AnalysisSummary::default(),
        };

        let result = analysis.check_thresholds(15, 15);
        assert!(result.is_err());
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].cognitive, 25);
    }

    #[test]
    fn test_check_thresholds_multiple_violations() {
        let analysis = Analysis {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                language: "rust".to_string(),
                functions: vec![
                    FunctionResult {
                        name: "ok_fn".to_string(),
                        file: "test.rs".to_string(),
                        start_line: 1,
                        end_line: 5,
                        metrics: Metrics {
                            cyclomatic: 3,
                            cognitive: 2,
                            max_nesting: 1,
                            lines: 5,
                        },
                    },
                    FunctionResult {
                        name: "bad_fn1".to_string(),
                        file: "test.rs".to_string(),
                        start_line: 10,
                        end_line: 50,
                        metrics: Metrics {
                            cyclomatic: 20,
                            cognitive: 18,
                            max_nesting: 4,
                            lines: 40,
                        },
                    },
                    FunctionResult {
                        name: "bad_fn2".to_string(),
                        file: "test.rs".to_string(),
                        start_line: 60,
                        end_line: 100,
                        metrics: Metrics {
                            cyclomatic: 10,
                            cognitive: 25,
                            max_nesting: 6,
                            lines: 40,
                        },
                    },
                ],
                total_cyclomatic: 33,
                total_cognitive: 45,
                avg_cyclomatic: 11.0,
                avg_cognitive: 15.0,
            }],
            summary: AnalysisSummary::default(),
        };

        let result = analysis.check_thresholds(15, 15);
        assert!(result.is_err());
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 2);
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
