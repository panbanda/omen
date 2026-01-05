//! Repository map (PageRank-ranked symbols) analyzer.
//!
//! Generates a PageRank-ranked index of repository symbols (functions, classes, etc.)
//! optimized for LLM context. Higher-ranked symbols are more "central" in the codebase
//! based on call relationships.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::{extract_functions, Parser};

/// Repomap analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// PageRank damping factor (default: 0.85).
    pub damping: f64,
    /// PageRank max iterations (default: 100).
    pub max_iterations: usize,
    /// PageRank convergence tolerance (default: 1e-6).
    pub tolerance: f64,
    /// Maximum symbols to return (0 = all).
    pub max_symbols: usize,
    /// Skip test files.
    pub skip_test_files: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
            max_symbols: 0,
            skip_test_files: true,
        }
    }
}

/// Repomap analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    pub fn with_damping(mut self, damping: f64) -> Self {
        self.config.damping = damping;
        self
    }

    pub fn with_max_symbols(mut self, max: usize) -> Self {
        self.config.max_symbols = max;
        self
    }

    pub fn with_skip_test_files(mut self, skip: bool) -> Self {
        self.config.skip_test_files = skip;
        self
    }

    /// Analyze a repository and generate a PageRank-ranked symbol map.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let parser = Parser::new();

        // Collect all symbols with their calls
        let mut symbols: Vec<SymbolInfo> = Vec::new();
        let mut symbol_names: HashMap<String, usize> = HashMap::new();

        for entry in WalkBuilder::new(repo_path).hidden(false).build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Skip test files if configured
            if self.config.skip_test_files && is_test_file(path) {
                continue;
            }

            // Detect language
            let lang = match Language::detect(path) {
                Some(l) => l,
                None => continue,
            };

            // Parse file
            let parse_result = match parser.parse_file(path) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Extract functions/symbols
            let functions = extract_functions(&parse_result);
            let source = &parse_result.source;

            for func in functions {
                let qualified_name = format!(
                    "{}:{}",
                    path.strip_prefix(repo_path)
                        .unwrap_or(path)
                        .to_string_lossy(),
                    func.name
                );

                // Get index for this symbol
                let idx = symbols.len();
                symbol_names.insert(qualified_name.clone(), idx);

                // Extract function calls from body
                let calls = if let Some(body_node) = func.body {
                    extract_calls_from_body(&body_node, source, lang)
                } else {
                    Vec::new()
                };

                symbols.push(SymbolInfo {
                    name: func.name.clone(),
                    qualified_name,
                    kind: SymbolKind::Function,
                    file: path
                        .strip_prefix(repo_path)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string(),
                    line: func.start_line,
                    signature: func.signature.clone(),
                    calls,
                    is_exported: func.is_exported,
                });
            }
        }

        // Build call graph
        let mut graph: DiGraph<usize, ()> = DiGraph::new();
        let mut node_indices: HashMap<usize, NodeIndex> = HashMap::new();

        // Create nodes
        for (idx, _) in symbols.iter().enumerate() {
            let node_idx = graph.add_node(idx);
            node_indices.insert(idx, node_idx);
        }

        // Create edges based on calls
        for (caller_idx, symbol) in symbols.iter().enumerate() {
            let caller_node = node_indices[&caller_idx];

            for call in &symbol.calls {
                // Try to resolve the call to a symbol
                // First, try exact qualified name match
                if let Some(&callee_idx) = symbol_names.get(call) {
                    let callee_node = node_indices[&callee_idx];
                    graph.add_edge(caller_node, callee_node, ());
                    continue;
                }

                // Try matching by function name only (within same file first)
                let same_file_key = format!("{}:{}", symbol.file, call);
                if let Some(&callee_idx) = symbol_names.get(&same_file_key) {
                    let callee_node = node_indices[&callee_idx];
                    graph.add_edge(caller_node, callee_node, ());
                    continue;
                }

                // Try any function with this name
                for (name, &callee_idx) in &symbol_names {
                    if name.ends_with(&format!(":{}", call)) {
                        let callee_node = node_indices[&callee_idx];
                        graph.add_edge(caller_node, callee_node, ());
                        break;
                    }
                }
            }
        }

        // Calculate PageRank
        let pagerank = self.calculate_pagerank(&graph);

        // Build output symbols with metrics
        let mut output_symbols: Vec<SymbolEntry> = symbols
            .iter()
            .enumerate()
            .map(|(idx, sym)| {
                let node_idx = node_indices[&idx];
                let pr = pagerank.get(&node_idx).copied().unwrap_or(0.0);
                let in_degree = graph.edges_directed(node_idx, Direction::Incoming).count();
                let out_degree = graph.edges_directed(node_idx, Direction::Outgoing).count();

                SymbolEntry {
                    name: sym.name.clone(),
                    kind: sym.kind,
                    file: sym.file.clone(),
                    line: sym.line,
                    signature: sym.signature.clone(),
                    pagerank: pr,
                    in_degree,
                    out_degree,
                }
            })
            .collect();

        // Sort by PageRank descending
        output_symbols.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit if configured
        if self.config.max_symbols > 0 && output_symbols.len() > self.config.max_symbols {
            output_symbols.truncate(self.config.max_symbols);
        }

        let summary = calculate_summary(&output_symbols);

        Ok(Analysis {
            generated_at: Utc::now().to_rfc3339(),
            symbols: output_symbols,
            summary,
        })
    }

    /// Calculate PageRank for all nodes in the graph.
    fn calculate_pagerank(&self, graph: &DiGraph<usize, ()>) -> HashMap<NodeIndex, f64> {
        let n = graph.node_count();
        if n == 0 {
            return HashMap::new();
        }

        let d = self.config.damping;
        let mut rank: HashMap<NodeIndex, f64> = graph
            .node_indices()
            .map(|idx| (idx, 1.0 / n as f64))
            .collect();

        for _ in 0..self.config.max_iterations {
            let mut new_rank: HashMap<NodeIndex, f64> = HashMap::new();
            let mut diff = 0.0;

            for node in graph.node_indices() {
                let incoming: f64 = graph
                    .edges_directed(node, Direction::Incoming)
                    .map(|e| {
                        let source = e.source();
                        let out_deg = graph.edges_directed(source, Direction::Outgoing).count();
                        if out_deg > 0 {
                            rank[&source] / out_deg as f64
                        } else {
                            0.0
                        }
                    })
                    .sum();

                let new_score = (1.0 - d) / n as f64 + d * incoming;
                diff += (new_score - rank[&node]).abs();
                new_rank.insert(node, new_score);
            }

            rank = new_rank;

            if diff < self.config.tolerance {
                break;
            }
        }

        rank
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "repomap"
    }

    fn description(&self) -> &'static str {
        "Generate PageRank-ranked symbol index for LLM context"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        self.analyze_repo(ctx.root)
    }
}

/// Internal structure to hold symbol info during collection.
struct SymbolInfo {
    name: String,
    #[allow(dead_code)]
    qualified_name: String,
    kind: SymbolKind,
    file: String,
    line: u32,
    signature: String,
    calls: Vec<String>,
    #[allow(dead_code)]
    is_exported: bool,
}

/// Repomap analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: String,
    pub symbols: Vec<SymbolEntry>,
    pub summary: Summary,
}

/// A symbol entry in the repo map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub signature: String,
    pub pagerank: f64,
    pub in_degree: usize,
    pub out_degree: usize,
}

/// Symbol kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Enum,
    Constant,
}

/// Summary statistics for the repo map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_symbols: usize,
    pub total_files: usize,
    pub avg_pagerank: f64,
    pub max_pagerank: f64,
    pub avg_connections: f64,
}

/// Calculate summary statistics from symbols.
fn calculate_summary(symbols: &[SymbolEntry]) -> Summary {
    if symbols.is_empty() {
        return Summary {
            total_symbols: 0,
            total_files: 0,
            avg_pagerank: 0.0,
            max_pagerank: 0.0,
            avg_connections: 0.0,
        };
    }

    let mut files = std::collections::HashSet::new();
    let mut total_pr = 0.0;
    let mut max_pr = 0.0_f64;
    let mut total_connections = 0;

    for sym in symbols {
        files.insert(&sym.file);
        total_pr += sym.pagerank;
        max_pr = max_pr.max(sym.pagerank);
        total_connections += sym.in_degree + sym.out_degree;
    }

    Summary {
        total_symbols: symbols.len(),
        total_files: files.len(),
        avg_pagerank: total_pr / symbols.len() as f64,
        max_pagerank: max_pr,
        avg_connections: total_connections as f64 / symbols.len() as f64,
    }
}

/// Extract function calls from a function body.
fn extract_calls_from_body(
    body: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Vec<String> {
    let mut calls = Vec::new();
    let mut cursor = body.walk();
    let call_node_kinds = get_call_node_kinds(lang);

    collect_calls(&mut cursor, source, &call_node_kinds, &mut calls);

    calls
}

/// Recursively collect call expressions.
fn collect_calls(
    cursor: &mut tree_sitter::TreeCursor<'_>,
    source: &[u8],
    call_kinds: &[&str],
    calls: &mut Vec<String>,
) {
    let node = cursor.node();

    if call_kinds.contains(&node.kind()) {
        if let Some(name) = extract_call_name(&node, source) {
            if !calls.contains(&name) {
                calls.push(name);
            }
        }
    }

    if cursor.goto_first_child() {
        loop {
            collect_calls(cursor, source, call_kinds, calls);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Get node kinds that represent function calls for a language.
fn get_call_node_kinds(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Go => vec!["call_expression"],
        Language::Rust => vec!["call_expression", "method_call_expression"],
        Language::Python => vec!["call"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            vec!["call_expression", "new_expression"]
        }
        Language::Java => vec!["method_invocation", "object_creation_expression"],
        Language::CSharp => vec!["invocation_expression", "object_creation_expression"],
        Language::Cpp | Language::C => vec!["call_expression"],
        Language::Ruby => vec!["call", "method_call"],
        Language::Php => vec!["function_call_expression", "method_call_expression"],
        Language::Bash => vec!["command"],
    }
}

/// Extract the function name from a call expression.
fn extract_call_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    // Try to find the function/identifier node
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "simple_identifier"
                || kind == "field_identifier"
                || kind == "property_identifier"
            {
                let text = child.utf8_text(source).ok()?;
                return Some(text.to_string());
            }
            // For method calls like obj.method(), get the method name
            if kind == "selector_expression" || kind == "member_expression" {
                // Get the rightmost identifier
                if let Some(right) = child.child_by_field_name("field")
                    .or_else(|| child.child_by_field_name("property"))
                    .or_else(|| child.child(child.child_count().saturating_sub(1)))
                {
                    let text = right.utf8_text(source).ok()?;
                    return Some(text.to_string());
                }
            }
            // For simple function calls, use the function child
            if kind == "function" {
                let text = child.utf8_text(source).ok()?;
                // Extract just the function name, not the full path
                return Some(text.split('.').last()?.to_string());
            }
        }
    }

    None
}

/// Check if a file is a test file.
fn is_test_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.ends_with("_test.go")
        || path_str.ends_with("_test.py")
        || path_str.ends_with(".test.ts")
        || path_str.ends_with(".test.js")
        || path_str.ends_with(".spec.ts")
        || path_str.ends_with(".spec.js")
        || path_str.contains("/test/")
        || path_str.contains("/tests/")
        || path_str.contains("/__tests__/")
        || path_str.starts_with("test/")
        || path_str.starts_with("tests/")
        || path_str.starts_with("__tests__/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.config.damping, 0.85);
        assert_eq!(analyzer.config.max_iterations, 100);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.damping, 0.85);
        assert_eq!(config.max_iterations, 100);
        assert!((config.tolerance - 1e-6).abs() < 1e-10);
        assert_eq!(config.max_symbols, 0);
        assert!(config.skip_test_files);
    }

    #[test]
    fn test_analyzer_with_damping() {
        let analyzer = Analyzer::new().with_damping(0.9);
        assert_eq!(analyzer.config.damping, 0.9);
    }

    #[test]
    fn test_analyzer_with_max_symbols() {
        let analyzer = Analyzer::new().with_max_symbols(100);
        assert_eq!(analyzer.config.max_symbols, 100);
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "repomap");
        assert!(analyzer.description().contains("PageRank"));
    }

    #[test]
    fn test_symbol_entry_fields() {
        let entry = SymbolEntry {
            name: "testFunc".to_string(),
            kind: SymbolKind::Function,
            file: "test.rs".to_string(),
            line: 10,
            signature: "fn testFunc()".to_string(),
            pagerank: 0.5,
            in_degree: 3,
            out_degree: 2,
        };

        assert_eq!(entry.name, "testFunc");
        assert_eq!(entry.kind, SymbolKind::Function);
        assert_eq!(entry.file, "test.rs");
        assert_eq!(entry.line, 10);
        assert_eq!(entry.in_degree, 3);
        assert_eq!(entry.out_degree, 2);
    }

    #[test]
    fn test_symbol_kind() {
        assert_eq!(SymbolKind::Function, SymbolKind::Function);
        assert_ne!(SymbolKind::Function, SymbolKind::Class);
    }

    #[test]
    fn test_calculate_summary_empty() {
        let summary = calculate_summary(&[]);
        assert_eq!(summary.total_symbols, 0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.avg_pagerank, 0.0);
        assert_eq!(summary.max_pagerank, 0.0);
        assert_eq!(summary.avg_connections, 0.0);
    }

    #[test]
    fn test_calculate_summary_with_symbols() {
        let symbols = vec![
            SymbolEntry {
                name: "a".to_string(),
                kind: SymbolKind::Function,
                file: "file1.rs".to_string(),
                line: 1,
                signature: String::new(),
                pagerank: 0.5,
                in_degree: 2,
                out_degree: 1,
            },
            SymbolEntry {
                name: "b".to_string(),
                kind: SymbolKind::Function,
                file: "file2.rs".to_string(),
                line: 1,
                signature: String::new(),
                pagerank: 0.3,
                in_degree: 1,
                out_degree: 2,
            },
        ];

        let summary = calculate_summary(&symbols);
        assert_eq!(summary.total_symbols, 2);
        assert_eq!(summary.total_files, 2);
        assert!((summary.avg_pagerank - 0.4).abs() < 0.001);
        assert!((summary.max_pagerank - 0.5).abs() < 0.001);
        assert!((summary.avg_connections - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file(Path::new("foo_test.go")));
        assert!(is_test_file(Path::new("bar_test.py")));
        assert!(is_test_file(Path::new("component.test.ts")));
        assert!(is_test_file(Path::new("component.spec.js")));
        assert!(is_test_file(Path::new("src/test/java/Foo.java")));
        assert!(is_test_file(Path::new("tests/unit.py")));
        assert!(is_test_file(Path::new("__tests__/foo.js")));

        assert!(!is_test_file(Path::new("main.go")));
        assert!(!is_test_file(Path::new("src/util.ts")));
    }

    #[test]
    fn test_get_call_node_kinds() {
        let go_kinds = get_call_node_kinds(Language::Go);
        assert!(go_kinds.contains(&"call_expression"));

        let rust_kinds = get_call_node_kinds(Language::Rust);
        assert!(rust_kinds.contains(&"call_expression"));
        assert!(rust_kinds.contains(&"method_call_expression"));

        let py_kinds = get_call_node_kinds(Language::Python);
        assert!(py_kinds.contains(&"call"));
    }

    #[test]
    fn test_pagerank_empty_graph() {
        let analyzer = Analyzer::new();
        let graph: DiGraph<usize, ()> = DiGraph::new();
        let pagerank = analyzer.calculate_pagerank(&graph);
        assert!(pagerank.is_empty());
    }

    #[test]
    fn test_pagerank_single_node() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();
        let node = graph.add_node(0);

        let pagerank = analyzer.calculate_pagerank(&graph);
        assert_eq!(pagerank.len(), 1);
        // Single node with no edges: rank = (1 - d) / n = 0.15 / 1 = 0.15
        // But starts at 1.0 and converges based on damping
        assert!(pagerank[&node] > 0.0);
        assert!(pagerank[&node] <= 1.0);
    }

    #[test]
    fn test_pagerank_chain() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // A -> B -> C
        let a = graph.add_node(0);
        let b = graph.add_node(1);
        let c = graph.add_node(2);

        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());

        let pagerank = analyzer.calculate_pagerank(&graph);
        assert_eq!(pagerank.len(), 3);

        // C should have highest rank (most incoming flow), A should have lowest
        assert!(pagerank[&c] > pagerank[&b]);
        assert!(pagerank[&b] > pagerank[&a]);
    }

    #[test]
    fn test_pagerank_star() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // Hub with multiple nodes pointing to it
        let hub = graph.add_node(0);
        let a = graph.add_node(1);
        let b = graph.add_node(2);
        let c = graph.add_node(3);

        graph.add_edge(a, hub, ());
        graph.add_edge(b, hub, ());
        graph.add_edge(c, hub, ());

        let pagerank = analyzer.calculate_pagerank(&graph);

        // Hub should have highest rank
        assert!(pagerank[&hub] > pagerank[&a]);
        assert!(pagerank[&hub] > pagerank[&b]);
        assert!(pagerank[&hub] > pagerank[&c]);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            symbols: vec![SymbolEntry {
                name: "test".to_string(),
                kind: SymbolKind::Function,
                file: "test.rs".to_string(),
                line: 1,
                signature: "fn test()".to_string(),
                pagerank: 0.5,
                in_degree: 1,
                out_degree: 2,
            }],
            summary: Summary {
                total_symbols: 1,
                total_files: 1,
                avg_pagerank: 0.5,
                max_pagerank: 0.5,
                avg_connections: 3.0,
            },
        };

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("\"test\""));
        assert!(json.contains("\"Function\""));

        let parsed: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.symbols.len(), 1);
        assert_eq!(parsed.symbols[0].name, "test");
    }
}
