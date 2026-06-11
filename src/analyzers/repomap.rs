//! Repository map (PageRank-ranked symbols) analyzer.
//!
//! Generates a PageRank-ranked index of repository symbols (functions, classes, etc.)
//! optimized for LLM context. Higher-ranked symbols are more "central" in the codebase
//! based on call relationships.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::Utc;
use ignore::WalkBuilder;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rayon::prelude::*;
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

/// Internal structure to hold symbol info during collection.
pub struct SymbolInfo {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub signature: String,
    pub calls: Vec<String>,
    pub is_exported: bool,
}

/// Index of the call graph for a repository — the parsed/collected phase.
/// Callers can use `resolve`, `callers`, and `callees` without re-parsing.
pub struct CallGraphIndex {
    pub symbols: Vec<SymbolInfo>,
    pub graph: DiGraph<usize, ()>,
    pub by_qualified: HashMap<String, usize>,
    pub by_name: HashMap<String, Vec<usize>>,
    pub(crate) node_indices: HashMap<usize, NodeIndex>,
}

impl CallGraphIndex {
    /// Resolve a symbol query to zero or more symbol indices.
    ///
    /// Resolution tiers:
    /// 1. Exact qualified-name match (`file.rs:name`) → returns that one index.
    /// 2. If `name` contains `:` → treat as `file:name` (same-file match).
    /// 3. Bare-name lookup → all matches from `by_name`, sorted lex by qualified_name.
    pub fn resolve(&self, name: &str) -> Vec<usize> {
        // Tier 1: exact qualified name
        if let Some(&idx) = self.by_qualified.get(name) {
            return vec![idx];
        }
        // Tier 2: name contains ':' — treat as file:name already tried above
        if name.contains(':') {
            // already tried qualified, no match
            return vec![];
        }
        // Tier 3: bare name → all candidates (already sorted lex by qualified_name)
        self.by_name.get(name).cloned().unwrap_or_default()
    }

    /// BFS returning callers of `roots` up to `depth` levels.
    ///
    /// Returns one `Vec<usize>` per BFS level (level 0 = direct callers).
    /// Each level is sorted by symbol index. Cycles are safe (visited set).
    pub fn callers(&self, roots: &[usize], depth: usize) -> Vec<Vec<usize>> {
        self.bfs_traverse(roots, depth, Direction::Incoming)
    }

    /// BFS returning callees of `roots` up to `depth` levels.
    ///
    /// Returns one `Vec<usize>` per BFS level (level 0 = direct callees).
    /// Each level is sorted by symbol index. Cycles are safe (visited set).
    pub fn callees(&self, roots: &[usize], depth: usize) -> Vec<Vec<usize>> {
        self.bfs_traverse(roots, depth, Direction::Outgoing)
    }

    fn bfs_traverse(&self, roots: &[usize], depth: usize, dir: Direction) -> Vec<Vec<usize>> {
        if depth == 0 {
            return vec![];
        }

        let mut visited: HashSet<NodeIndex> = HashSet::new();
        // Pre-insert root node indices into visited so we don't revisit them
        for &root_idx in roots {
            if let Some(&ni) = self.node_indices.get(&root_idx) {
                visited.insert(ni);
            }
        }

        // Queue entries: (NodeIndex, depth_remaining)
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        for &root_idx in roots {
            if let Some(&ni) = self.node_indices.get(&root_idx) {
                queue.push_back((ni, depth));
            }
        }

        // levels indexed 0..depth
        let mut levels: Vec<Vec<usize>> = vec![vec![]; depth];

        while let Some((node, remaining)) = queue.pop_front() {
            let level_idx = depth - remaining;
            let neighbors: Vec<NodeIndex> = match dir {
                Direction::Incoming => self
                    .graph
                    .edges_directed(node, Direction::Incoming)
                    .map(|e| e.source())
                    .collect(),
                Direction::Outgoing => self
                    .graph
                    .edges_directed(node, Direction::Outgoing)
                    .map(|e| e.target())
                    .collect(),
            };

            for neighbor in neighbors {
                if visited.contains(&neighbor) {
                    continue;
                }
                visited.insert(neighbor);
                let Some(&sym_idx) = self.graph.node_weight(neighbor) else {
                    continue;
                };
                if level_idx < depth {
                    levels[level_idx].push(sym_idx);
                }
                if remaining > 1 {
                    queue.push_back((neighbor, remaining - 1));
                }
            }
        }

        // Sort each level by symbol index for determinism
        for level in &mut levels {
            level.sort_unstable();
        }

        levels
    }
}

/// Build a `CallGraphIndex` from a list of absolute file paths.
///
/// This contains the parse/symbol-collection/graph-build phases (no PageRank).
pub fn build_index(repo_path: &Path, files: &[PathBuf]) -> Result<CallGraphIndex> {
    // Phase 1: Parallel parsing - extract symbols from all files
    let file_symbols: Vec<Vec<SymbolInfo>> = files
        .par_iter()
        .filter_map(|path| {
            let lang = Language::detect(path)?;
            let parser = Parser::new();
            let parse_result = parser.parse_file(path).ok()?;
            let functions = extract_functions(&parse_result);
            let source = &parse_result.source;

            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let symbols: Vec<SymbolInfo> = functions
                .into_iter()
                .map(|func| {
                    let qualified_name = format!("{}:{}", rel_path, func.name);
                    let calls = if let Some((start, end)) = func.body_byte_range {
                        let root = parse_result.tree.root_node();
                        if let Some(body_node) = root.descendant_for_byte_range(start, end) {
                            extract_calls_from_body(&body_node, source, lang)
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };

                    SymbolInfo {
                        name: func.name.clone(),
                        qualified_name,
                        kind: SymbolKind::Function,
                        file: rel_path.clone(),
                        line: func.start_line,
                        end_line: func.end_line,
                        signature: func.signature.clone(),
                        calls,
                        is_exported: func.is_exported,
                    }
                })
                .collect();

            Some(symbols)
        })
        .collect();

    // Flatten
    let symbols: Vec<SymbolInfo> = file_symbols.into_iter().flatten().collect();

    // Build lookup indices
    let mut by_qualified: HashMap<String, usize> = HashMap::with_capacity(symbols.len());
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, sym) in symbols.iter().enumerate() {
        by_qualified.insert(sym.qualified_name.clone(), idx);
        by_name.entry(sym.name.clone()).or_default().push(idx);
    }

    // Pre-sort name lookups for deterministic resolution (lex by qualified_name)
    for indices in by_name.values_mut() {
        indices.sort_by(|a, b| symbols[*a].qualified_name.cmp(&symbols[*b].qualified_name));
    }

    // Build call graph
    let mut graph: DiGraph<usize, ()> = DiGraph::new();
    let mut node_indices: HashMap<usize, NodeIndex> = HashMap::with_capacity(symbols.len());

    for idx in 0..symbols.len() {
        let node_idx = graph.add_node(idx);
        node_indices.insert(idx, node_idx);
    }

    // Create edges based on calls
    for (caller_idx, symbol) in symbols.iter().enumerate() {
        let caller_node = node_indices[&caller_idx];

        for call in &symbol.calls {
            // 1. Try exact qualified name match
            if let Some(&callee_idx) = by_qualified.get(call) {
                let callee_node = node_indices[&callee_idx];
                graph.add_edge(caller_node, callee_node, ());
                continue;
            }

            // 2. Try same-file match
            let same_file_key = format!("{}:{}", symbol.file, call);
            if let Some(&callee_idx) = by_qualified.get(&same_file_key) {
                let callee_node = node_indices[&callee_idx];
                graph.add_edge(caller_node, callee_node, ());
                continue;
            }

            // 3. Use name index for O(1) lookup (already sorted for determinism)
            if let Some(indices) = by_name.get(call) {
                if let Some(&callee_idx) = indices.first() {
                    let callee_node = node_indices[&callee_idx];
                    graph.add_edge(caller_node, callee_node, ());
                }
            }
        }
    }

    Ok(CallGraphIndex {
        symbols,
        graph,
        by_qualified,
        by_name,
        node_indices,
    })
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
        // Phase 1: Collect all file paths (fast)
        let files: Vec<_> = WalkBuilder::new(repo_path)
            .hidden(false)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .filter(|e| !self.config.skip_test_files || !is_test_file(e.path()))
            .filter(|e| Language::detect(e.path()).is_some())
            .map(|e| e.into_path())
            .collect();

        self.analyze_files(repo_path, &files)
    }

    /// Analyze using a pre-filtered file set from an AnalysisContext.
    pub fn analyze_with_files(
        &self,
        repo_path: &Path,
        file_set: &crate::core::FileSet,
    ) -> Result<Analysis> {
        let files: Vec<_> = file_set
            .iter()
            .filter(|path| {
                if self.config.skip_test_files && is_test_file(path) {
                    return false;
                }
                Language::detect(path).is_some()
            })
            .map(|p| repo_path.join(p))
            .collect();

        self.analyze_files(repo_path, &files)
    }

    /// Core analysis logic operating on a list of absolute file paths.
    fn analyze_files(&self, repo_path: &Path, files: &[PathBuf]) -> Result<Analysis> {
        let index = build_index(repo_path, files)?;

        // Phase 5: Calculate PageRank
        let pagerank = self.calculate_pagerank(&index.graph);

        // Phase 6: Build output symbols with metrics
        let mut output_symbols: Vec<SymbolEntry> = index
            .symbols
            .iter()
            .enumerate()
            .map(|(idx, sym)| {
                let node_idx = index.node_indices[&idx];
                let pr = pagerank.get(&node_idx).copied().unwrap_or(0.0);
                let in_degree = index
                    .graph
                    .edges_directed(node_idx, Direction::Incoming)
                    .count();
                let out_degree = index
                    .graph
                    .edges_directed(node_idx, Direction::Outgoing)
                    .count();

                SymbolEntry {
                    name: sym.name.clone(),
                    qualified_name: sym.qualified_name.clone(),
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
    ///
    /// Note: Dangling nodes (no outgoing edges) are effectively treated as
    /// having self-loops rather than redistributing rank uniformly.
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
        self.analyze_with_files(ctx.root, ctx.files)
    }
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
    pub qualified_name: String,
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
    for i in 0..node.child_count() as u32 {
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
            // PHP: function_call_expression has a `name` child (kind == "name")
            if kind == "name" {
                let text = child.utf8_text(source).ok()?;
                return Some(text.to_string());
            }
            // For method calls like obj.method(), get the method name
            if kind == "selector_expression" || kind == "member_expression" {
                // Get the rightmost identifier
                if let Some(right) = child
                    .child_by_field_name("field")
                    .or_else(|| child.child_by_field_name("property"))
                    .or_else(|| child.child(child.child_count().saturating_sub(1) as u32))
                {
                    let text = right.utf8_text(source).ok()?;
                    return Some(text.to_string());
                }
            }
            // C# member_access_expression: `new B().b()` → invocation_expression →
            // member_access_expression → (object_creation_expression . identifier)
            // Extract the rightmost identifier (the method name).
            if kind == "member_access_expression" {
                // Walk children to find the last identifier
                let mut last_ident: Option<String> = None;
                for j in 0..child.child_count() as u32 {
                    if let Some(grandchild) = child.child(j) {
                        if grandchild.kind() == "identifier" {
                            if let Ok(text) = grandchild.utf8_text(source) {
                                last_ident = Some(text.to_string());
                            }
                        }
                    }
                }
                if let Some(name) = last_ident {
                    return Some(name);
                }
            }
            // For simple function calls, use the function child
            if kind == "function" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let text = name_node.utf8_text(source).ok()?;
                    return Some(text.to_string());
                }
            }
            // Bash: command node has a command_name child which has a word child
            if kind == "command_name" {
                // Get the first child of command_name (typically a `word`)
                if let Some(word) = child.child(0) {
                    let text = word.utf8_text(source).ok()?;
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

fn is_test_file(path: &Path) -> bool {
    // Normalise to forward slashes so Windows paths (using `\`) are handled.
    let path_str = path.to_string_lossy().replace('\\', "/");
    path_str.contains("/test")
        || path_str.contains("/tests/")
        || path_str.contains("_test.")
        || path_str.contains("test_")
        || path_str.ends_with("_test.go")
        || path_str.ends_with(".test.ts")
        || path_str.ends_with(".spec.ts")
        || path_str.ends_with(".test.js")
        || path_str.ends_with(".spec.js")
        || path_str.ends_with(".test.tsx")
        || path_str.ends_with(".spec.tsx")
        || path_str.ends_with(".test.jsx")
        || path_str.ends_with(".spec.jsx")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_rust_fixture() -> TempDir {
        let dir = TempDir::new().unwrap();

        // a.rs: fn a() calls b()
        fs::write(
            dir.path().join("a.rs"),
            r#"
fn a() {
    b();
}
"#,
        )
        .unwrap();

        // b.rs: fn b() calls c()
        fs::write(
            dir.path().join("b.rs"),
            r#"
fn b() {
    c();
}
"#,
        )
        .unwrap();

        // c.rs: fn c() — leaf
        fs::write(
            dir.path().join("c.rs"),
            r#"
fn c() {
    // leaf
}
"#,
        )
        .unwrap();

        dir
    }

    #[test]
    fn test_characterization_repomap_output() {
        // Characterization test: pins current repomap output for a→b→c chain.
        let dir = create_rust_fixture();
        let path = dir.path();

        let analyzer = Analyzer::new().with_skip_test_files(false);
        let result = analyzer.analyze_repo(path).unwrap();

        // Should have 3 symbols: a, b, c
        assert_eq!(result.symbols.len(), 3);

        // Collect names for checking
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"), "symbols should include a");
        assert!(names.contains(&"b"), "symbols should include b");
        assert!(names.contains(&"c"), "symbols should include c");

        // By PageRank ordering, c (receives calls from b) should be >= b (receives from a) >= a
        // c is called by b which is called by a → c has highest in_degree
        // Verify files are reported correctly
        let by_name: HashMap<&str, &SymbolEntry> = result
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s))
            .collect();

        let sym_a = by_name["a"];
        let sym_b = by_name["b"];
        let sym_c = by_name["c"];

        // out_degree: a→b (1), b→c (1), c→nothing (0)
        assert_eq!(sym_a.out_degree, 1, "a should call b");
        assert_eq!(sym_b.out_degree, 1, "b should call c");
        assert_eq!(sym_c.out_degree, 0, "c calls nothing");

        // in_degree: a←nothing (0), b←a (1), c←b (1)
        assert_eq!(sym_a.in_degree, 0, "nobody calls a");
        assert_eq!(sym_b.in_degree, 1, "a calls b");
        assert_eq!(sym_c.in_degree, 1, "b calls c");

        // PageRank ordering: c should rank highest (receives rank from b which gets from a)
        // At minimum: c.pagerank >= a.pagerank
        assert!(
            sym_c.pagerank >= sym_a.pagerank,
            "c should rank >= a by pagerank"
        );

        // Verify qualified_name is present
        assert!(sym_a.qualified_name.contains("a.rs:a"));
        assert!(sym_b.qualified_name.contains("b.rs:b"));
        assert!(sym_c.qualified_name.contains("c.rs:c"));
    }

    #[test]
    fn test_build_index_single_rust_file() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            r#"
fn foo() {
    bar();
}
fn bar() {}
"#,
        )
        .unwrap();

        let files = vec![dir.path().join("main.rs")];
        let index = build_index(dir.path(), &files).unwrap();

        assert_eq!(index.symbols.len(), 2);
        let names: Vec<&str> = index.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
    }

    #[test]
    fn test_resolve_exact_qualified() {
        let dir = create_rust_fixture();
        let files: Vec<PathBuf> = vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect();
        let index = build_index(dir.path(), &files).unwrap();

        // Exact qualified name
        let result = index.resolve("a.rs:a");
        assert_eq!(result.len(), 1);
        assert_eq!(index.symbols[result[0]].name, "a");
    }

    #[test]
    fn test_resolve_bare_name_multiple_sorted() {
        let dir = TempDir::new().unwrap();
        // Two files with same function name
        fs::write(dir.path().join("z_mod.rs"), r#"fn helper() {}"#).unwrap();
        fs::write(dir.path().join("a_mod.rs"), r#"fn helper() {}"#).unwrap();

        let files = vec![dir.path().join("z_mod.rs"), dir.path().join("a_mod.rs")];
        let index = build_index(dir.path(), &files).unwrap();

        let result = index.resolve("helper");
        assert_eq!(result.len(), 2);
        // Should be sorted lex by qualified_name → a_mod.rs:helper first
        assert!(index.symbols[result[0]].qualified_name < index.symbols[result[1]].qualified_name);
        assert!(index.symbols[result[0]].qualified_name.contains("a_mod"));
    }

    #[test]
    fn test_resolve_unknown_empty() {
        let dir = create_rust_fixture();
        let files: Vec<PathBuf> = vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect();
        let index = build_index(dir.path(), &files).unwrap();

        let result = index.resolve("nonexistent_function_xyz");
        assert!(result.is_empty());
    }

    #[test]
    fn test_callers_depth_1() {
        // a→b→c. callers of c depth 1 = [[b]]
        let dir = create_rust_fixture();
        let files: Vec<PathBuf> = vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect();
        let index = build_index(dir.path(), &files).unwrap();

        let c_idxs = index.resolve("c");
        assert!(!c_idxs.is_empty());
        let levels = index.callers(&c_idxs, 1);
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].len(), 1);
        assert_eq!(index.symbols[levels[0][0]].name, "b");
    }

    #[test]
    fn test_callers_depth_2() {
        // a→b→c. callers of c depth 2 = [[b],[a]]
        let dir = create_rust_fixture();
        let files: Vec<PathBuf> = vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect();
        let index = build_index(dir.path(), &files).unwrap();

        let c_idxs = index.resolve("c");
        let levels = index.callers(&c_idxs, 2);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].len(), 1);
        assert_eq!(index.symbols[levels[0][0]].name, "b");
        assert_eq!(levels[1].len(), 1);
        assert_eq!(index.symbols[levels[1][0]].name, "a");
    }

    #[test]
    fn test_callers_cycle_safe() {
        // a→b→a (cycle) - should terminate
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), r#"fn a() { b(); }"#).unwrap();
        fs::write(dir.path().join("b.rs"), r#"fn b() { a(); }"#).unwrap();

        let files = vec![dir.path().join("a.rs"), dir.path().join("b.rs")];
        let index = build_index(dir.path(), &files).unwrap();

        let a_idxs = index.resolve("a");
        // Should not hang or panic
        let levels = index.callers(&a_idxs, 5);
        assert!(!levels.is_empty());
    }

    #[test]
    fn test_callees_depth_2() {
        // a→b→c. callees of a depth 2 = [[b],[c]]
        let dir = create_rust_fixture();
        let files: Vec<PathBuf> = vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect();
        let index = build_index(dir.path(), &files).unwrap();

        let a_idxs = index.resolve("a");
        let levels = index.callees(&a_idxs, 2);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].len(), 1);
        assert_eq!(index.symbols[levels[0][0]].name, "b");
        assert_eq!(levels[1].len(), 1);
        assert_eq!(index.symbols[levels[1][0]].name, "c");
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
                qualified_name: "test.rs:test".to_string(),
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
        assert_eq!(parsed.symbols[0].qualified_name, "test.rs:test");
    }

    #[test]
    fn test_pagerank_cycle() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // A -> B -> C -> A (circular dependency)
        let a = graph.add_node(0);
        let b = graph.add_node(1);
        let c = graph.add_node(2);

        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());
        graph.add_edge(c, a, ());

        let pagerank = analyzer.calculate_pagerank(&graph);
        assert_eq!(pagerank.len(), 3);

        // All nodes should have equal rank in a symmetric cycle
        let ranks: Vec<f64> = vec![pagerank[&a], pagerank[&b], pagerank[&c]];
        let max_diff = ranks
            .iter()
            .map(|r| (r - ranks[0]).abs())
            .fold(0.0f64, f64::max);
        assert!(
            max_diff < 0.01,
            "Nodes in a symmetric cycle should have roughly equal rank, diff={}",
            max_diff
        );
    }

    #[test]
    fn test_pagerank_self_loop() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // A -> A (self-reference), B -> A
        let a = graph.add_node(0);
        let b = graph.add_node(1);

        graph.add_edge(a, a, ());
        graph.add_edge(b, a, ());

        let pagerank = analyzer.calculate_pagerank(&graph);
        assert_eq!(pagerank.len(), 2);
        // A should have higher rank than B (A receives from both self and B)
        assert!(pagerank[&a] > pagerank[&b]);
    }

    #[test]
    fn test_pagerank_disconnected_components() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // Component 1: A -> B
        let a = graph.add_node(0);
        let b = graph.add_node(1);
        graph.add_edge(a, b, ());

        // Component 2: C -> D (disconnected from component 1)
        let c = graph.add_node(2);
        let d = graph.add_node(3);
        graph.add_edge(c, d, ());

        let pagerank = analyzer.calculate_pagerank(&graph);
        assert_eq!(pagerank.len(), 4);

        // Symmetric structure: A and C should have similar ranks, B and D similar
        assert!((pagerank[&a] - pagerank[&c]).abs() < 0.01);
        assert!((pagerank[&b] - pagerank[&d]).abs() < 0.01);
    }

    #[test]
    fn test_calculate_summary_single_file_symbols() {
        let symbols = vec![
            SymbolEntry {
                name: "a".to_string(),
                qualified_name: "file1.rs:a".to_string(),
                kind: SymbolKind::Function,
                file: "file1.rs".to_string(),
                line: 1,
                signature: String::new(),
                pagerank: 0.5,
                in_degree: 0,
                out_degree: 0,
            },
            SymbolEntry {
                name: "b".to_string(),
                qualified_name: "file1.rs:b".to_string(),
                kind: SymbolKind::Function,
                file: "file1.rs".to_string(),
                line: 10,
                signature: String::new(),
                pagerank: 0.3,
                in_degree: 1,
                out_degree: 2,
            },
        ];

        let summary = calculate_summary(&symbols);
        assert_eq!(summary.total_symbols, 2);
        // Both in same file
        assert_eq!(summary.total_files, 1);
    }

    #[test]
    fn test_max_symbols_truncation() {
        let analyzer = Analyzer::new().with_max_symbols(2);
        // Verify config is set
        assert_eq!(analyzer.config.max_symbols, 2);
    }

    #[test]
    fn test_deterministic_call_resolution() {
        // When multiple functions match a call name, the resolution should be
        // deterministic (lexicographically sorted by qualified name)
        let mut symbol_names: HashMap<String, usize> = HashMap::new();
        symbol_names.insert("z_module.rs:helper".to_string(), 0);
        symbol_names.insert("a_module.rs:helper".to_string(), 1);
        symbol_names.insert("m_module.rs:helper".to_string(), 2);

        let call = "helper";
        let suffix = format!(":{}", call);

        // Collect and sort candidates
        let mut candidates: Vec<_> = symbol_names
            .iter()
            .filter(|(name, _)| name.ends_with(&suffix))
            .collect();
        candidates.sort_by(|a, b| a.0.cmp(b.0));

        // Should always resolve to a_module.rs:helper (lexicographically first)
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].0, "a_module.rs:helper");
        assert_eq!(*candidates[0].1, 1);

        // Run multiple times to verify determinism
        for _ in 0..10 {
            let mut candidates2: Vec<_> = symbol_names
                .iter()
                .filter(|(name, _)| name.ends_with(&suffix))
                .collect();
            candidates2.sort_by(|a, b| a.0.cmp(b.0));
            assert_eq!(candidates2[0].0, "a_module.rs:helper");
        }
    }

    // ===== is_test_file tests =====

    #[test]
    fn test_is_test_file_known_patterns() {
        // Patterns that SHOULD be identified as test files.
        // Note: the `/test` prefix check requires a leading `/`, so use paths
        // with a directory component to trigger that branch.
        let positives = [
            "src/foo_test.go",     // _test. match
            "src/tests/bar.rs",    // /test match
            "src/foo.test.ts",     // .test.ts match
            "src/foo.spec.ts",     // .spec.ts match
            "src/foo.test.js",     // .test.js match
            "src/foo.spec.js",     // .spec.js match
            "src/foo.test.tsx",    // .test.tsx match (new)
            "src/foo.spec.tsx",    // .spec.tsx match (new)
            "src/foo.test.jsx",    // .test.jsx match (new)
            "src/foo.spec.jsx",    // .spec.jsx match (new)
            "src/test_helpers.rs", // test_ match
        ];
        for p in &positives {
            assert!(is_test_file(Path::new(p)), "expected {p} to be a test file");
        }
    }

    #[test]
    fn test_is_test_file_non_test_paths() {
        let negatives = [
            "src/main.rs",
            "src/foo.ts",
            "src/foo.tsx",
            "src/foo.jsx",
            "src/foo.js",
        ];
        for p in &negatives {
            assert!(
                !is_test_file(Path::new(p)),
                "expected {p} NOT to be a test file"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_is_test_file_windows_backslash() {
        // On Windows paths may use backslash separators.
        assert!(is_test_file(Path::new("src\\tests\\foo.rs")));
        assert!(is_test_file(Path::new("src\\foo.test.tsx")));
    }
}
