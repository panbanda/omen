//! Complexity analyzer - cyclomatic and cognitive complexity.
//!
//! # Overview
//!
//! This analyzer computes two types of complexity metrics:
//!
//! - **Cyclomatic Complexity**: Counts the number of linearly independent paths through code.
//!   Based on McCabe (1976) "A Complexity Measure", IEEE TSE SE-2(4).
//!
//! - **Cognitive Complexity**: Measures how hard code is to understand, with penalties
//!   for nesting. Based on SonarSource's methodology.
//!   Reference: https://www.sonarsource.com/docs/CognitiveComplexity.pdf
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

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result, SourceFile};
use crate::parser::queries::{
    get_decision_node_types, get_flat_node_types, get_nesting_node_types,
};
use crate::parser::{self, ParseResult, Parser};

use std::path::Path;

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

    /// Analyze complexity for file content (without reading from filesystem).
    pub fn analyze_content(&self, path: &Path, content: Vec<u8>) -> Result<FileResult> {
        // Skip files that are too large
        if content.len() > Self::MAX_FILE_SIZE as usize {
            return Err(crate::core::Error::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "File too large: {} bytes (max {})",
                    content.len(),
                    Self::MAX_FILE_SIZE
                ),
            });
        }

        let language =
            Language::detect(path).ok_or_else(|| crate::core::Error::UnsupportedLanguage {
                path: path.to_path_buf(),
            })?;

        let source_file = SourceFile::from_content(path, language, content);
        let result = self.parser.parse_source(&source_file)?;
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
                let result = if ctx.content_source.is_some() {
                    // Read via content source (e.g., git tree)
                    ctx.read_file(path)
                        .ok()
                        .and_then(|content| self.analyze_content(path, content).ok())
                } else {
                    // Read from filesystem using absolute path
                    let full_path = ctx.root.join(path);
                    self.analyze_file(&full_path).ok()
                };

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

/// Analyze complexity for a single function from its parse result.
pub fn analyze_function_complexity(func: &parser::FunctionNode, result: &ParseResult) -> Metrics {
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
/// Uses iterative cursor traversal for performance.
fn find_function_at_line<'a>(
    root: &tree_sitter::Node<'a>,
    target_line: u32,
) -> Option<tree_sitter::Node<'a>> {
    let line = target_line.saturating_sub(1); // Convert to 0-indexed
    let mut cursor = root.walk();

    loop {
        let node = cursor.node();
        let start = node.start_position().row as u32;
        let end = node.end_position().row as u32;

        // Only descend if line is within this node's range
        if start <= line && line <= end {
            let kind = node.kind();
            if kind.contains("function") || kind.contains("method") || kind == "impl_item" {
                return Some(node);
            }

            // Try to go deeper
            if cursor.goto_first_child() {
                continue;
            }
        }

        // Move to next sibling or go up
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

/// Count decision points for cyclomatic complexity.
/// Uses iterative cursor traversal for performance.
fn count_decision_points(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> u32 {
    let decision_types = get_decision_node_types(lang);
    let mut count = 0;
    let mut cursor = node.walk();

    loop {
        let current = cursor.node();
        let kind = current.kind();

        // Count decision points
        if decision_types.contains(&kind) {
            count += 1;
        }

        // Count logical operators as additional decision points
        if kind == "binary_expression" || kind == "logical_expression" {
            if let Some(op) = get_operator(&current, source) {
                if op == "&&" || op == "||" || op == "and" || op == "or" {
                    count += 1;
                }
            }
        }

        // Traverse tree
        if cursor.goto_first_child() {
            continue;
        }

        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return count;
            }
        }
    }
}

/// Calculate cognitive complexity with nesting penalties.
/// Uses iterative cursor traversal with depth tracking for performance.
///
/// Per SonarSource Cognitive Complexity specification:
/// - Nesting constructs (if, for, while, etc.) add +1 plus nesting depth
/// - Flat constructs (else, elif, break, continue) add +1 only (no nesting penalty)
/// - Logical operators (&&, ||, and, or) add +1 each (no nesting penalty)
fn calculate_cognitive_complexity(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
    initial_depth: u32,
) -> u32 {
    let nesting_types = get_nesting_node_types(lang);
    let flat_types = get_flat_node_types(lang);

    let mut complexity = 0;
    let mut cursor = node.walk();
    let start_depth = cursor.depth();

    // Track nesting depth separately from cursor depth
    // Use a small stack to track depth changes at each cursor level
    let mut depth_at_level: Vec<u32> = vec![initial_depth; 64]; // Pre-allocate for typical depths

    loop {
        let current = cursor.node();
        let kind = current.kind();
        let cursor_depth = cursor.depth();
        let level = (cursor_depth - start_depth) as usize;

        // Ensure we have space for this level
        if level >= depth_at_level.len() {
            depth_at_level.resize(level + 16, initial_depth);
        }

        let current_depth = depth_at_level[level];

        // Check if this is a complexity-adding construct
        if nesting_types.contains(&kind) {
            // Nesting constructs: +1 base plus nesting penalty
            complexity += 1 + current_depth;
            // Children will have increased depth
            if level + 1 < depth_at_level.len() {
                depth_at_level[level + 1] = current_depth + 1;
            }
        } else if flat_types.contains(&kind) {
            // Flat constructs: +1 only, NO nesting penalty per SonarSource spec
            complexity += 1;
            // Children stay at same depth
            if level + 1 < depth_at_level.len() {
                depth_at_level[level + 1] = current_depth;
            }
        } else if kind == "binary_expression"
            || kind == "logical_expression"
            || kind == "boolean_operator"
        {
            // Logical operators: +1 each for &&, ||, and, or (no nesting penalty)
            if let Some(op) = get_operator(&current, source) {
                if op == "&&" || op == "||" || op == "and" || op == "or" {
                    complexity += 1;
                }
            }
            // Children inherit current depth
            if level + 1 < depth_at_level.len() {
                depth_at_level[level + 1] = current_depth;
            }
        } else {
            // Non-complexity node, children inherit current depth
            if level + 1 < depth_at_level.len() {
                depth_at_level[level + 1] = current_depth;
            }
        }

        // Traverse tree
        if cursor.goto_first_child() {
            continue;
        }

        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.depth() < start_depth {
                return complexity;
            }
        }
    }
}

/// Calculate maximum nesting depth.
/// Uses iterative cursor traversal with depth tracking for performance.
fn calculate_max_nesting(node: &tree_sitter::Node<'_>, _source: &[u8], initial_depth: u32) -> u32 {
    const NESTING_KINDS: &[&str] = &[
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

    let mut max_depth = initial_depth;
    let mut cursor = node.walk();
    let start_depth = cursor.depth();

    // Track nesting depth at each cursor level
    let mut depth_at_level: Vec<u32> = vec![initial_depth; 64];

    loop {
        let current = cursor.node();
        let kind = current.kind();
        let cursor_depth = cursor.depth();
        let level = (cursor_depth - start_depth) as usize;

        if level >= depth_at_level.len() {
            depth_at_level.resize(level + 16, initial_depth);
        }

        let current_depth = depth_at_level[level];

        // Track max depth seen
        if current_depth > max_depth {
            max_depth = current_depth;
        }

        // Set depth for children
        let child_depth = if NESTING_KINDS.contains(&kind) {
            current_depth + 1
        } else {
            current_depth
        };

        if level + 1 < depth_at_level.len() {
            depth_at_level[level + 1] = child_depth;
        }

        // Traverse tree
        if cursor.goto_first_child() {
            continue;
        }

        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.depth() < start_depth {
                return max_depth;
            }
        }
    }
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

    // Language-specific complexity calculation tests

    fn parse_and_analyze(code: &[u8], lang: Language, filename: &str) -> FileResult {
        let parser = crate::parser::Parser::new();
        let result = parser
            .parse(code, lang, std::path::Path::new(filename))
            .expect("Parse failed");
        analyze_parse_result(&result)
    }

    #[test]
    fn test_complexity_rust_simple_function() {
        let code = b"fn simple() { let x = 1; }";
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        // Cyclomatic: 1 (baseline for a simple function)
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_rust_if_statement() {
        let code = b"fn with_if(x: i32) { if x > 0 { return; } }";
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // Cyclomatic: 1 (baseline) + 1 (if) = 2
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_rust_match_expression() {
        let code = br#"
fn with_match(x: i32) -> &'static str {
    match x {
        0 => "zero",
        1 => "one",
        _ => "other",
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // Match expression counts as one decision point (the match itself)
        // Individual arms contribute to cognitive complexity via nesting
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_rust_nested_if() {
        let code = br#"
fn nested(x: i32, y: i32) {
    if x > 0 {
        if y > 0 {
            println!("both positive");
        }
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // Nested if should have higher cognitive complexity
        assert!(result.functions[0].metrics.cognitive >= 2);
        assert!(result.functions[0].metrics.max_nesting >= 2);
    }

    #[test]
    fn test_complexity_go_simple_function() {
        let code = b"package main\n\nfunc simple() { x := 1 }";
        let result = parse_and_analyze(code, Language::Go, "test.go");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_go_if_else() {
        let code = br#"
package main

func withIfElse(x int) int {
    if x > 0 {
        return 1
    } else {
        return -1
    }
}
"#;
        let result = parse_and_analyze(code, Language::Go, "test.go");
        assert_eq!(result.functions.len(), 1);
        // if + else adds complexity
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_go_switch() {
        let code = br#"
package main

func withSwitch(x int) string {
    switch x {
    case 0:
        return "zero"
    case 1:
        return "one"
    default:
        return "other"
    }
}
"#;
        let result = parse_and_analyze(code, Language::Go, "test.go");
        assert_eq!(result.functions.len(), 1);
        // Switch statement counts as one decision point
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_go_for_loop() {
        let code = br#"
package main

func withLoop() {
    for i := 0; i < 10; i++ {
        println(i)
    }
}
"#;
        let result = parse_and_analyze(code, Language::Go, "test.go");
        assert_eq!(result.functions.len(), 1);
        // for loop adds complexity
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_python_simple_function() {
        let code = b"def simple():\n    x = 1";
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_python_if_elif_else() {
        let code = br#"
def classify(x):
    if x > 0:
        return "positive"
    elif x < 0:
        return "negative"
    else:
        return "zero"
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        // if + elif adds complexity
        assert!(result.functions[0].metrics.cyclomatic >= 3);
    }

    #[test]
    fn test_complexity_python_for_loop() {
        let code = br#"
def sum_list(items):
    total = 0
    for item in items:
        total += item
    return total
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_python_while_loop() {
        let code = br#"
def countdown(n):
    while n > 0:
        print(n)
        n -= 1
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_javascript_simple_function() {
        let code = b"function simple() { const x = 1; }";
        let result = parse_and_analyze(code, Language::JavaScript, "test.js");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_javascript_ternary() {
        let code = b"function tern(x) { return x > 0 ? 'pos' : 'neg'; }";
        let result = parse_and_analyze(code, Language::JavaScript, "test.js");
        assert_eq!(result.functions.len(), 1);
        // Ternary operator adds complexity
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_javascript_logical_and_or() {
        let code = b"function check(a, b) { if (a && b || !a) { return true; } return false; }";
        let result = parse_and_analyze(code, Language::JavaScript, "test.js");
        assert_eq!(result.functions.len(), 1);
        // Logical operators add to cognitive complexity
        assert!(result.functions[0].metrics.cognitive >= 1);
    }

    #[test]
    fn test_complexity_typescript_simple_function() {
        let code = b"function simple(): void { const x: number = 1; }";
        let result = parse_and_analyze(code, Language::TypeScript, "test.ts");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_typescript_try_catch() {
        let code = br#"
function safeParse(json: string): any {
    try {
        return JSON.parse(json);
    } catch (e) {
        return null;
    }
}
"#;
        let result = parse_and_analyze(code, Language::TypeScript, "test.ts");
        assert_eq!(result.functions.len(), 1);
        // try-catch adds complexity
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_ruby_simple_method() {
        let code = b"def simple\n  x = 1\nend";
        let result = parse_and_analyze(code, Language::Ruby, "test.rb");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_ruby_if_unless() {
        let code = br#"
def check(x)
  if x > 0
    "positive"
  else
    "non-positive"
  end
end
"#;
        let result = parse_and_analyze(code, Language::Ruby, "test.rb");
        assert_eq!(result.functions.len(), 1);
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_complexity_ruby_case_when() {
        let code = br#"
def classify(x)
  case x
  when 0 then "zero"
  when 1 then "one"
  else "other"
  end
end
"#;
        let result = parse_and_analyze(code, Language::Ruby, "test.rb");
        assert_eq!(result.functions.len(), 1);
        // case with multiple when clauses
        assert!(result.functions[0].metrics.cyclomatic >= 3);
    }

    #[test]
    fn test_complexity_java_simple_method() {
        let code = b"class Test { void simple() { int x = 1; } }";
        let result = parse_and_analyze(code, Language::Java, "Test.java");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "simple");
        assert_eq!(result.functions[0].metrics.cyclomatic, 1);
    }

    #[test]
    fn test_complexity_java_switch() {
        let code = br#"
class Test {
    String classify(int x) {
        switch (x) {
            case 0: return "zero";
            case 1: return "one";
            default: return "other";
        }
    }
}
"#;
        let result = parse_and_analyze(code, Language::Java, "Test.java");
        assert_eq!(result.functions.len(), 1);
        // Switch statement counts as one decision point
        assert!(result.functions[0].metrics.cyclomatic >= 2);
    }

    #[test]
    fn test_nesting_depth_multiple_levels() {
        let code = br#"
fn deeply_nested(x: i32, y: i32, z: i32) {
    if x > 0 {
        if y > 0 {
            if z > 0 {
                println!("all positive");
            }
        }
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // Should detect 3 levels of nesting
        assert!(result.functions[0].metrics.max_nesting >= 3);
    }

    #[test]
    fn test_multiple_functions_same_file() {
        let code = br#"
fn first() { let x = 1; }
fn second(x: i32) { if x > 0 { return; } }
fn third() { for i in 0..10 { println!("{}", i); } }
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 3);

        // Verify each function has correct name
        let names: Vec<_> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"first"));
        assert!(names.contains(&"second"));
        assert!(names.contains(&"third"));
    }

    // SonarSource Cognitive Complexity specification tests
    // These tests verify the fix for:
    // 1. else/elif adding +1 only (no nesting penalty)
    // 2. logical operators (&&/||) counting in cognitive complexity

    #[test]
    fn test_cognitive_else_no_nesting_penalty() {
        // Per SonarSource spec: else adds +1 only, no nesting penalty
        let code = br#"
fn with_else(x: i32) -> i32 {
    if x > 0 {
        1
    } else {
        -1
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // if = +1 (base) + 0 (depth) = 1
        // else = +1 (flat, no depth penalty)
        // Total = 2
        assert_eq!(
            result.functions[0].metrics.cognitive, 2,
            "if + else should equal 2 (else should not get nesting penalty)"
        );
    }

    #[test]
    fn test_cognitive_nested_if_else() {
        // Nested structure: outer if, inner if + else
        let code = br#"
fn nested_if_else(x: i32, y: i32) -> i32 {
    if x > 0 {
        if y > 0 {
            1
        } else {
            2
        }
    } else {
        0
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // outer if = +1 + 0 = 1
        // inner if = +1 + 1 = 2
        // inner else = +1 (no depth)
        // outer else = +1 (no depth)
        // Total = 5
        assert_eq!(
            result.functions[0].metrics.cognitive, 5,
            "nested if/else expected 5"
        );
    }

    #[test]
    fn test_cognitive_logical_operators_js() {
        // Per SonarSource spec: each && and || adds +1 (no nesting penalty)
        let code = b"function check(a, b, c) { if (a && b || c) { return true; } return false; }";
        let result = parse_and_analyze(code, Language::JavaScript, "test.js");
        assert_eq!(result.functions.len(), 1);
        // if = +1, && = +1, || = +1
        // Total = 3
        assert_eq!(
            result.functions[0].metrics.cognitive, 3,
            "if + && + || should equal 3"
        );
    }

    #[test]
    fn test_cognitive_elif_no_nesting_penalty_py() {
        // Per SonarSource spec: elif adds +1 only, no nesting penalty
        let code = br#"
def classify(x):
    if x > 0:
        return "positive"
    elif x < 0:
        return "negative"
    else:
        return "zero"
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        // if = +1, elif = +1 (flat), else = +1 (flat)
        // Total = 3
        assert_eq!(
            result.functions[0].metrics.cognitive, 3,
            "if + elif + else should equal 3"
        );
    }

    #[test]
    fn test_cognitive_complex_nesting() {
        // Test deeply nested structure with proper depth penalties
        let code = br#"
fn complex(a: bool, b: bool, c: bool) {
    if a {
        if b {
            if c {
                println!("all true");
            }
        }
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // if a (depth 0) = +1 + 0 = 1
        // if b (depth 1) = +1 + 1 = 2
        // if c (depth 2) = +1 + 2 = 3
        // Total = 6
        assert_eq!(
            result.functions[0].metrics.cognitive, 6,
            "3 nested ifs should equal 6 (1+2+3)"
        );
    }

    #[test]
    fn test_cognitive_catch_no_nesting_penalty() {
        // Per SonarSource spec: catch adds +1 only, no nesting penalty
        // (similar to else - it's already inside a nesting structure)
        let code = br#"
function safeParse(json) {
    try {
        if (json) {
            return JSON.parse(json);
        }
    } catch (e) {
        return null;
    }
}
"#;
        let result = parse_and_analyze(code, Language::JavaScript, "test.js");
        assert_eq!(result.functions.len(), 1);
        // try = +1 (nesting construct)
        // if (inside try, depth 1) = +1 + 1 = 2
        // catch = +1 (flat, no depth penalty per SonarSource spec)
        // Total = 4
        assert_eq!(
            result.functions[0].metrics.cognitive, 4,
            "try + nested if + catch should equal 4 (catch should not get nesting penalty)"
        );
    }

    #[test]
    fn test_cognitive_python_except_no_nesting_penalty() {
        // Python's except is equivalent to catch - should be flat
        let code = br#"
def safe_parse(data):
    try:
        if data:
            return json.loads(data)
    except ValueError:
        return None
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        // try = +1
        // if (depth 1) = +1 + 1 = 2
        // except = +1 (flat)
        // Total = 4
        assert_eq!(
            result.functions[0].metrics.cognitive, 4,
            "try + nested if + except should equal 4 (except should not get nesting penalty)"
        );
    }

    #[test]
    fn test_complexity_python_conditional_expression() {
        let code = br#"
def pick(x):
    return 1 if x > 0 else 0
"#;
        let result = parse_and_analyze(code, Language::Python, "test.py");
        assert_eq!(result.functions.len(), 1);
        // baseline 1 + conditional_expression 1 = 2
        assert_eq!(
            result.functions[0].metrics.cyclomatic, 2,
            "Python ternary (conditional_expression) must count as a decision point"
        );
    }

    #[test]
    fn test_complexity_rust_while_let() {
        // while let parses as while_expression with a let_condition child
        // in tree-sitter-rust 0.23+, so it is already counted.
        let code = br#"
fn drain(v: &mut Vec<i32>) {
    while let Some(x) = v.pop() {
        println!("{}", x);
    }
}
"#;
        let result = parse_and_analyze(code, Language::Rust, "test.rs");
        assert_eq!(result.functions.len(), 1);
        // baseline 1 + while_expression 1 = 2
        assert_eq!(
            result.functions[0].metrics.cyclomatic, 2,
            "Rust while-let must count as a decision point (via while_expression)"
        );
    }

    #[test]
    fn test_complexity_go_case_clauses() {
        let code = br#"
package main

func classify(x int) string {
    switch x {
    case 0:
        return "zero"
    case 1:
        return "one"
    default:
        return "other"
    }
}
"#;
        let result = parse_and_analyze(code, Language::Go, "test.go");
        assert_eq!(result.functions.len(), 1);
        // baseline 1 + expression_switch_statement 1 + expression_case * 2 = 4
        // (default_case is not counted)
        assert_eq!(
            result.functions[0].metrics.cyclomatic, 4,
            "Go switch with 2 case clauses: 1 base + 1 switch + 2 cases = 4"
        );
    }

    #[test]
    fn test_complexity_typescript_switch_case() {
        let code = br#"
function classify(x: number): string {
    switch (x) {
        case 0: return "zero";
        case 1: return "one";
        default: return "other";
    }
}
"#;
        let result = parse_and_analyze(code, Language::TypeScript, "test.ts");
        assert_eq!(result.functions.len(), 1);
        // baseline 1 + switch_statement 1 + switch_case * 2 = 4
        // (default_clause is not a switch_case, so not counted)
        assert!(
            result.functions[0].metrics.cyclomatic >= 4,
            "TypeScript switch with 2 case clauses should count each case as a decision point"
        );
    }
}
