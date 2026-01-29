//! Dependency graph analyzer.
//!
//! Builds a directed graph of file dependencies, calculates graph metrics,
//! detects cycles, and outputs Mermaid diagrams.
//!
//! # Key Metrics
//!
//! - **PageRank**: Importance based on incoming edges
//!   Reference: Page, Brin, Motwani, Winograd (1999) "The PageRank Citation Ranking"
//!   Damping factor 0.85 is the canonical value.
//!
//! - **Betweenness Centrality**: How often a node appears on shortest paths
//!   Reference: Brandes, U. (2001) "A Faster Algorithm for Betweenness Centrality"
//!
//! - **Instability**: out_degree / (in_degree + out_degree)
//!   Reference: Martin, R.C. (2003) "Agile Software Development"
//!   Measures tendency to change (1.0 = maximally unstable, 0.0 = maximally stable)
//!
//! - **Cycle Detection**: Uses Tarjan's SCC algorithm
//!   Reference: Tarjan, R. (1972) "Depth-first search and linear graph algorithms"
//!
//! # Known Limitation
//!
//! PageRank implementation does not redistribute dangling node mass uniformly,
//! which may slightly affect scores in sparse graphs.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::{extract_imports, Parser};

/// Graph analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// PageRank damping factor (default: 0.85).
    pub damping: f64,
    /// PageRank max iterations (default: 100).
    pub max_iterations: usize,
    /// PageRank convergence tolerance (default: 1e-6).
    pub tolerance: f64,
    /// Resolve relative imports to absolute paths.
    pub resolve_imports: bool,
    /// Include external dependencies.
    pub include_external: bool,
}

/// Pre-built index for O(1) file path lookups during import resolution.
#[allow(dead_code)]
struct FilePathIndex {
    /// Full relative path -> relative path (identity mapping for exact matches).
    by_full_path: HashMap<String, String>,
    /// File stem (without extension) -> list of relative paths.
    by_stem: HashMap<String, Vec<String>>,
    /// File name (with extension) -> list of relative paths.
    by_name: HashMap<String, Vec<String>>,
    /// Normalized path segments for partial matching.
    by_segments: HashMap<String, Vec<String>>,
}

impl FilePathIndex {
    fn new(files: &[std::path::PathBuf], root: &Path) -> Self {
        let mut by_full_path = HashMap::with_capacity(files.len());
        let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        let mut by_segments: HashMap<String, Vec<String>> = HashMap::new();

        for file in files {
            let rel = file.strip_prefix(root).unwrap_or(file);
            let rel_str = rel.to_string_lossy().to_string();

            // Full path lookup
            by_full_path.insert(rel_str.clone(), rel_str.clone());

            // File stem lookup (e.g., "mod" from "mod.rs")
            if let Some(stem) = rel.file_stem() {
                let stem_str = stem.to_string_lossy().to_string();
                by_stem.entry(stem_str).or_default().push(rel_str.clone());
            }

            // File name lookup (e.g., "mod.rs")
            if let Some(name) = rel.file_name() {
                let name_str = name.to_string_lossy().to_string();
                by_name.entry(name_str).or_default().push(rel_str.clone());
            }

            // Segment-based lookup for partial path matching
            // e.g., "src/utils/helpers" can match "utils/helpers.rs"
            for component in rel.iter() {
                let comp_str = component.to_string_lossy().to_string();
                if !comp_str.is_empty() && comp_str != "." {
                    by_segments
                        .entry(comp_str)
                        .or_default()
                        .push(rel_str.clone());
                }
            }
        }

        Self {
            by_full_path,
            by_stem,
            by_name,
            by_segments,
        }
    }

    /// Try to find a file matching the import path.
    fn find_match(&self, import_path: &str, from_file: &Path) -> Option<String> {
        // 1. Handle relative imports (./foo, ../foo)
        if import_path.starts_with("./") || import_path.starts_with("../") {
            if let Some(parent) = from_file.parent() {
                let resolved = parent.join(import_path);
                let normalized = normalize_path(&resolved);

                // Try exact match with common extensions
                for ext in &[
                    "", ".rs", ".go", ".py", ".ts", ".tsx", ".js", ".jsx", ".java",
                ] {
                    let with_ext = if ext.is_empty() {
                        normalized.clone()
                    } else {
                        format!(
                            "{}.{}",
                            normalized.trim_end_matches(&format!(".{}", &ext[1..])),
                            &ext[1..]
                        )
                    };

                    if self.by_full_path.contains_key(&with_ext) {
                        return Some(with_ext);
                    }

                    // Try index files (e.g., ./utils -> ./utils/index.ts)
                    let index_path = format!("{}/index{}", normalized, ext);
                    if self.by_full_path.contains_key(&index_path) {
                        return Some(index_path);
                    }
                }
            }
        }

        // 2. Try exact stem match
        if let Some(matches) = self.by_stem.get(import_path) {
            if matches.len() == 1 {
                return Some(matches[0].clone());
            }
            // Multiple matches - prefer shortest path (most specific)
            if !matches.is_empty() {
                let mut sorted = matches.clone();
                sorted.sort_by_key(|s| s.len());
                return Some(sorted[0].clone());
            }
        }

        // 3. Try snake_case conversion for Ruby CamelCase constants
        //    e.g., "OrderSearcher" -> "order_searcher", "ActiveModel::Validations" -> "active_model/validations"
        if import_path.contains("::") {
            // Scoped constant: split on ::, convert each segment, join with /
            // e.g., ActiveModel::Validations -> active_model/validations
            let snake_path: String = import_path
                .split("::")
                .map(camel_to_snake)
                .collect::<Vec<_>>()
                .join("/");
            // Match on path segment boundaries (must be preceded by / or start of string)
            let with_slash = format!("/{}", snake_path);
            if let Some(matched) = self
                .by_full_path
                .keys()
                .find(|k| k.starts_with(&snake_path) || k.contains(&with_slash))
            {
                return Some(matched.clone());
            }
            // Also try the last segment as a stem match
            if let Some(last) = snake_path.rsplit('/').next() {
                if let Some(matches) = self.by_stem.get(last) {
                    for candidate in matches {
                        if candidate.contains(&snake_path) {
                            return Some(candidate.clone());
                        }
                    }
                }
            }
        } else {
            let snake = camel_to_snake(import_path);
            if snake != import_path {
                if let Some(matches) = self.by_stem.get(&snake) {
                    if matches.len() == 1 {
                        return Some(matches[0].clone());
                    }
                    if !matches.is_empty() {
                        let mut sorted = matches.clone();
                        sorted.sort_by_key(|s| s.len());
                        return Some(sorted[0].clone());
                    }
                }
            }
        }

        // 4. Try segment-based matching for module paths like "utils/helpers"
        let segments: Vec<&str> = import_path.split('/').filter(|s| !s.is_empty()).collect();
        if let Some(last_segment) = segments.last() {
            if let Some(candidates) = self.by_stem.get(*last_segment) {
                // Find candidates that match the full path pattern
                for candidate in candidates {
                    if candidate.contains(import_path) {
                        return Some(candidate.clone());
                    }
                }
                // Fall back to first match with the stem
                if !candidates.is_empty() {
                    return Some(candidates[0].clone());
                }
            }
        }

        None
    }
}

/// Convert CamelCase to snake_case for Ruby constant-to-filename resolution.
/// Handles consecutive uppercase (e.g., HTTPClient -> http_client).
fn camel_to_snake(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
                // Insert underscore before: uppercase after lowercase, or
                // start of new word in consecutive uppercase (e.g., the P in HTTPParser)
                if prev.is_lowercase() || (prev.is_uppercase() && next_is_lower) {
                    result.push('_');
                }
            }
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Normalize a path by removing . and resolving ..
fn normalize_path(path: &Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(c) => {
                components.push(c.to_string_lossy().to_string());
            }
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            _ => {}
        }
    }
    components.join("/")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
            resolve_imports: true,
            include_external: false,
        }
    }
}

/// Graph analyzer.
pub struct Analyzer {
    #[allow(dead_code)]
    parser: Parser,
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
            parser: Parser::new(),
            config: Config::default(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            parser: Parser::new(),
            config,
        }
    }

    /// Analyze a directory and build dependency graph.
    pub fn analyze_project(&self, root: &Path) -> Result<Analysis> {
        use crate::config::Config as AppConfig;
        use crate::core::FileSet;

        let config = AppConfig::default();
        let file_set = FileSet::from_path(root, &config)?;
        let ctx = AnalysisContext::new(&file_set, &config, Some(root));
        self.analyze_files(&ctx)
    }

    /// Analyze a set of files and build dependency graph.
    /// Uses ctx.read_file() to support both filesystem and git tree sources.
    pub fn analyze_files(&self, ctx: &AnalysisContext<'_>) -> Result<Analysis> {
        let files: Vec<_> = ctx.files.iter().collect();

        // Build file path index for O(1) lookups during import resolution
        let file_index = FilePathIndex::new(
            &files.iter().map(|p| (*p).clone()).collect::<Vec<_>>(),
            ctx.root,
        );

        // Parallel parsing: extract imports from all files concurrently
        let file_imports: Vec<(String, Vec<String>)> = files
            .par_iter()
            .filter_map(|file| {
                let rel_path = file.strip_prefix(ctx.root).unwrap_or(file);
                let path_str = rel_path.to_string_lossy().to_string();

                // Read file via context (supports both filesystem and git tree)
                let content = ctx.read_file(file).ok()?;
                let lang = Language::detect(file)?;

                // Parse and extract imports using thread-local parser
                let parser = Parser::new();
                let result = parser.parse(&content, lang, file).ok()?;
                let imports = extract_imports(&result);

                // Resolve imports using the pre-built index
                let resolved: Vec<String> = if self.config.resolve_imports {
                    imports
                        .iter()
                        .filter_map(|imp| {
                            file_index.find_match(&imp.path, rel_path).or_else(|| {
                                if self.config.include_external {
                                    Some(imp.path.clone())
                                } else {
                                    None
                                }
                            })
                        })
                        .collect()
                } else {
                    imports.iter().map(|imp| imp.path.clone()).collect()
                };

                Some((path_str, resolved))
            })
            .collect();

        // Build graph (sequential, but fast since parsing is done)
        let mut graph: DiGraph<String, ()> = DiGraph::with_capacity(files.len(), files.len() * 4);
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::with_capacity(files.len());

        // First pass: create all nodes
        for (path_str, _) in &file_imports {
            if !node_indices.contains_key(path_str) {
                let idx = graph.add_node(path_str.clone());
                node_indices.insert(path_str.clone(), idx);
            }
        }

        // Second pass: create edges
        for (from_path, imports) in &file_imports {
            let from_idx = node_indices[from_path];

            for import in imports {
                // Add target node if not exists (external dependency)
                let to_idx = if let Some(&idx) = node_indices.get(import) {
                    idx
                } else if self.config.include_external {
                    let idx = graph.add_node(import.clone());
                    node_indices.insert(import.clone(), idx);
                    idx
                } else {
                    continue;
                };

                // Add edge (avoid self-loops)
                if from_idx != to_idx && !graph.contains_edge(from_idx, to_idx) {
                    graph.add_edge(from_idx, to_idx, ());
                }
            }
        }

        // Calculate metrics
        let pagerank = self.calculate_pagerank(&graph);
        let betweenness = self.calculate_betweenness(&graph);
        let cycles = self.detect_cycles(&graph);

        // Build nodes with metrics
        let mut nodes: Vec<Node> = Vec::new();
        for (path, &idx) in &node_indices {
            let in_deg = graph.edges_directed(idx, Direction::Incoming).count();
            let out_deg = graph.edges_directed(idx, Direction::Outgoing).count();
            let total_deg = in_deg + out_deg;
            let instability = if total_deg > 0 {
                out_deg as f64 / total_deg as f64
            } else {
                0.0
            };

            nodes.push(Node {
                path: path.clone(),
                pagerank: *pagerank.get(&idx).unwrap_or(&0.0),
                betweenness: *betweenness.get(&idx).unwrap_or(&0.0),
                in_degree: in_deg,
                out_degree: out_deg,
                instability,
            });
        }

        // Sort by PageRank descending
        nodes.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Build edges list
        let edges: Vec<Edge> = graph
            .edge_references()
            .map(|e| {
                let from = &graph[e.source()];
                let to = &graph[e.target()];
                Edge {
                    from: from.clone(),
                    to: to.clone(),
                }
            })
            .collect();

        // Calculate summary
        let total_nodes = nodes.len();
        let total_edges = edges.len();
        let avg_degree = if total_nodes > 0 {
            (2.0 * total_edges as f64) / total_nodes as f64
        } else {
            0.0
        };

        Ok(Analysis {
            nodes,
            edges,
            cycles,
            summary: AnalysisSummary {
                total_nodes,
                total_edges,
                avg_degree,
                cycle_count: 0, // Will be set from cycles.len()
            },
        })
    }

    /// Calculate PageRank scores using power iteration.
    fn calculate_pagerank(&self, graph: &DiGraph<String, ()>) -> HashMap<NodeIndex, f64> {
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
            let mut new_rank: HashMap<NodeIndex, f64> = HashMap::with_capacity(n);
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

    /// Calculate betweenness centrality using Brandes' algorithm with parallel BFS.
    fn calculate_betweenness(&self, graph: &DiGraph<String, ()>) -> HashMap<NodeIndex, f64> {
        let n = graph.node_count();
        if n <= 2 {
            return graph.node_indices().map(|idx| (idx, 0.0)).collect();
        }

        // Use all nodes as sources (no sampling - per project requirements)
        let sources: Vec<NodeIndex> = graph.node_indices().collect();

        // Parallel betweenness calculation
        let partial_betweenness: Vec<HashMap<NodeIndex, f64>> = sources
            .par_iter()
            .map(|&source| {
                let mut local_betweenness: HashMap<NodeIndex, f64> = HashMap::new();
                let mut dist: HashMap<NodeIndex, i32> = HashMap::with_capacity(n);
                let mut paths: HashMap<NodeIndex, f64> = HashMap::with_capacity(n);
                let mut predecessors: HashMap<NodeIndex, Vec<NodeIndex>> =
                    HashMap::with_capacity(n);
                let mut stack: Vec<NodeIndex> = Vec::with_capacity(n);
                let mut queue: VecDeque<NodeIndex> = VecDeque::with_capacity(n);

                dist.insert(source, 0);
                paths.insert(source, 1.0);
                queue.push_back(source);

                // BFS
                while let Some(v) = queue.pop_front() {
                    stack.push(v);
                    let v_dist = dist[&v];

                    for edge in graph.edges_directed(v, Direction::Outgoing) {
                        let w = edge.target();

                        // First visit
                        if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(w) {
                            e.insert(v_dist + 1);
                            queue.push_back(w);
                        }

                        // Shortest path via v
                        if dist[&w] == v_dist + 1 {
                            *paths.entry(w).or_insert(0.0) += *paths.get(&v).unwrap_or(&0.0);
                            predecessors.entry(w).or_default().push(v);
                        }
                    }
                }

                // Accumulate dependencies
                let mut delta: HashMap<NodeIndex, f64> = HashMap::with_capacity(n);
                while let Some(w) = stack.pop() {
                    if let Some(preds) = predecessors.get(&w) {
                        for &v in preds {
                            let coeff = (paths.get(&v).unwrap_or(&0.0)
                                / paths.get(&w).unwrap_or(&1.0))
                                * (1.0 + delta.get(&w).unwrap_or(&0.0));
                            *delta.entry(v).or_insert(0.0) += coeff;
                        }
                    }
                    if w != source {
                        *local_betweenness.entry(w).or_insert(0.0) += delta.get(&w).unwrap_or(&0.0);
                    }
                }

                local_betweenness
            })
            .collect();

        // Merge partial results
        let mut betweenness: HashMap<NodeIndex, f64> =
            graph.node_indices().map(|idx| (idx, 0.0)).collect();

        for partial in partial_betweenness {
            for (idx, value) in partial {
                *betweenness.entry(idx).or_insert(0.0) += value;
            }
        }

        // Normalize betweenness scores
        let norm = if n > 2 {
            1.0 / ((n - 1) * (n - 2)) as f64
        } else {
            1.0
        };

        for value in betweenness.values_mut() {
            *value *= norm;
        }

        betweenness
    }

    /// Detect cycles using Tarjan's strongly connected components.
    fn detect_cycles(&self, graph: &DiGraph<String, ()>) -> Vec<Vec<String>> {
        let sccs = tarjan_scc(graph);

        sccs.into_iter()
            .filter(|scc| {
                // Only include SCCs with multiple nodes or self-loops
                scc.len() > 1 || (scc.len() == 1 && graph.contains_edge(scc[0], scc[0]))
            })
            .map(|scc| scc.into_iter().map(|idx| graph[idx].clone()).collect())
            .collect()
    }

    /// Generate Mermaid diagram.
    pub fn to_mermaid(&self, analysis: &Analysis) -> String {
        let mut output = String::from("graph TD\n");

        // Create node definitions with sanitized IDs
        let mut node_ids: HashMap<&str, String> = HashMap::new();
        for (i, node) in analysis.nodes.iter().enumerate() {
            let id = format!("n{i}");
            node_ids.insert(&node.path, id.clone());

            // Format label with metrics
            let label = format!(
                "{}\\nPR:{:.3} In:{} Out:{}",
                sanitize_mermaid_label(&node.path),
                node.pagerank,
                node.in_degree,
                node.out_degree
            );
            output.push_str(&format!("    {id}[\"{label}\"]\n"));
        }

        // Add edges
        for edge in &analysis.edges {
            if let (Some(from_id), Some(to_id)) = (
                node_ids.get(edge.from.as_str()),
                node_ids.get(edge.to.as_str()),
            ) {
                output.push_str(&format!("    {} --> {}\n", from_id, to_id));
            }
        }

        // Style cycle nodes
        if !analysis.cycles.is_empty() {
            output.push_str("\n    %% Cycle nodes\n");
            let cycle_nodes: HashSet<&str> = analysis
                .cycles
                .iter()
                .flatten()
                .map(|s| s.as_str())
                .collect();

            for node in &cycle_nodes {
                if let Some(id) = node_ids.get(node) {
                    output.push_str(&format!("    style {id} fill:#f96\n"));
                }
            }
        }

        output
    }

    /// Generate DOT format (Graphviz).
    pub fn to_dot(&self, analysis: &Analysis) -> String {
        let mut output = String::from("digraph G {\n");
        output.push_str("    rankdir=LR;\n");
        output.push_str("    node [shape=box];\n\n");

        // Create node definitions
        let mut node_ids: HashMap<&str, String> = HashMap::new();
        for (i, node) in analysis.nodes.iter().enumerate() {
            let id = format!("n{i}");
            node_ids.insert(&node.path, id.clone());

            let label = format!(
                "{}\\nPageRank: {:.3}\\nIn: {} Out: {}",
                node.path.replace('"', "\\\""),
                node.pagerank,
                node.in_degree,
                node.out_degree
            );
            output.push_str(&format!("    {id} [label=\"{label}\"];\n"));
        }

        output.push('\n');

        // Add edges
        for edge in &analysis.edges {
            if let (Some(from_id), Some(to_id)) = (
                node_ids.get(edge.from.as_str()),
                node_ids.get(edge.to.as_str()),
            ) {
                output.push_str(&format!("    {} -> {};\n", from_id, to_id));
            }
        }

        output.push_str("}\n");
        output
    }
}

fn sanitize_mermaid_label(s: &str) -> String {
    s.replace(['/', '.', '-'], "_").replace('"', "'")
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "graph"
    }

    fn description(&self) -> &'static str {
        "Map module dependencies, calculate PageRank/centrality"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let mut analysis = self.analyze_files(ctx)?;
        analysis.summary.cycle_count = analysis.cycles.len();
        Ok(analysis)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub cycles: Vec<Vec<String>>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub path: String,
    pub pagerank: f64,
    pub betweenness: f64,
    pub in_degree: usize,
    pub out_degree: usize,
    pub instability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub avg_degree: f64,
    pub cycle_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "graph");
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!((config.damping - 0.85).abs() < 0.001);
        assert_eq!(config.max_iterations, 100);
        assert!((config.tolerance - 1e-6).abs() < 1e-10);
    }

    #[test]
    fn test_pagerank_empty_graph() {
        let analyzer = Analyzer::new();
        let graph: DiGraph<String, ()> = DiGraph::new();
        let ranks = analyzer.calculate_pagerank(&graph);
        assert!(ranks.is_empty());
    }

    #[test]
    fn test_pagerank_single_node() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        graph.add_node("a.rs".to_string());
        let ranks = analyzer.calculate_pagerank(&graph);
        assert_eq!(ranks.len(), 1);
        // Single node with no incoming edges converges to (1-d)/n = 0.15
        for &rank in ranks.values() {
            assert!((rank - 0.15).abs() < 0.001);
        }
    }

    #[test]
    fn test_pagerank_two_nodes_with_edge() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        graph.add_edge(a, b, ());

        let ranks = analyzer.calculate_pagerank(&graph);
        assert_eq!(ranks.len(), 2);

        // Node b should have higher PageRank (receives link from a)
        let rank_a = ranks[&a];
        let rank_b = ranks[&b];
        assert!(rank_b > rank_a, "Node b should have higher PageRank");
    }

    #[test]
    fn test_betweenness_empty() {
        let analyzer = Analyzer::new();
        let graph: DiGraph<String, ()> = DiGraph::new();
        let betweenness = analyzer.calculate_betweenness(&graph);
        assert!(betweenness.is_empty());
    }

    #[test]
    fn test_betweenness_linear_graph() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        let c = graph.add_node("c.rs".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());

        let betweenness = analyzer.calculate_betweenness(&graph);
        // Node b is on all shortest paths from a to c
        assert!(
            betweenness[&b] > 0.0,
            "Central node should have positive betweenness"
        );
    }

    #[test]
    fn test_cycle_detection_no_cycle() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        graph.add_edge(a, b, ());

        let cycles = analyzer.detect_cycles(&graph);
        assert!(cycles.is_empty());
    }

    #[test]
    fn test_cycle_detection_with_cycle() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        let c = graph.add_node("c.rs".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());
        graph.add_edge(c, a, ());

        let cycles = analyzer.detect_cycles(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
    }

    #[test]
    fn test_cycle_detection_self_loop() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        graph.add_edge(a, a, ());

        let cycles = analyzer.detect_cycles(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 1);
    }

    #[test]
    fn test_instability_calculation() {
        // A node with only outgoing edges has instability = 1.0 (most unstable)
        // A node with only incoming edges has instability = 0.0 (most stable)
        let _analyzer = Analyzer::new(); // Ensure Analyzer compiles
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        let c = graph.add_node("c.rs".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(a, c, ());

        // a: out=2, in=0 -> instability = 2/2 = 1.0
        // b: out=0, in=1 -> instability = 0/1 = 0.0
        // c: out=0, in=1 -> instability = 0/1 = 0.0
        let in_a = graph.edges_directed(a, Direction::Incoming).count();
        let out_a = graph.edges_directed(a, Direction::Outgoing).count();
        let instability_a = out_a as f64 / (in_a + out_a) as f64;
        assert!((instability_a - 1.0).abs() < 0.001);

        let in_b = graph.edges_directed(b, Direction::Incoming).count();
        let out_b = graph.edges_directed(b, Direction::Outgoing).count();
        let instability_b = out_b as f64 / (in_b + out_b) as f64;
        assert!((instability_b - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_mermaid_generation() {
        let analyzer = Analyzer::new();
        let analysis = Analysis {
            nodes: vec![
                Node {
                    path: "src/main.rs".to_string(),
                    pagerank: 0.5,
                    betweenness: 0.2,
                    in_degree: 2,
                    out_degree: 1,
                    instability: 0.333,
                },
                Node {
                    path: "src/lib.rs".to_string(),
                    pagerank: 0.5,
                    betweenness: 0.0,
                    in_degree: 1,
                    out_degree: 2,
                    instability: 0.666,
                },
            ],
            edges: vec![Edge {
                from: "src/main.rs".to_string(),
                to: "src/lib.rs".to_string(),
            }],
            cycles: vec![],
            summary: AnalysisSummary::default(),
        };

        let mermaid = analyzer.to_mermaid(&analysis);
        assert!(mermaid.starts_with("graph TD"));
        assert!(mermaid.contains("n0"));
        assert!(mermaid.contains("n1"));
        assert!(mermaid.contains("-->"));
    }

    #[test]
    fn test_mermaid_with_cycles() {
        let analyzer = Analyzer::new();
        let analysis = Analysis {
            nodes: vec![
                Node {
                    path: "a.rs".to_string(),
                    pagerank: 0.33,
                    betweenness: 0.0,
                    in_degree: 1,
                    out_degree: 1,
                    instability: 0.5,
                },
                Node {
                    path: "b.rs".to_string(),
                    pagerank: 0.33,
                    betweenness: 0.0,
                    in_degree: 1,
                    out_degree: 1,
                    instability: 0.5,
                },
            ],
            edges: vec![
                Edge {
                    from: "a.rs".to_string(),
                    to: "b.rs".to_string(),
                },
                Edge {
                    from: "b.rs".to_string(),
                    to: "a.rs".to_string(),
                },
            ],
            cycles: vec![vec!["a.rs".to_string(), "b.rs".to_string()]],
            summary: AnalysisSummary::default(),
        };

        let mermaid = analyzer.to_mermaid(&analysis);
        assert!(mermaid.contains("Cycle nodes"));
        assert!(mermaid.contains("style"));
        assert!(mermaid.contains("fill:#f96"));
    }

    #[test]
    fn test_dot_generation() {
        let analyzer = Analyzer::new();
        let analysis = Analysis {
            nodes: vec![Node {
                path: "main.rs".to_string(),
                pagerank: 1.0,
                betweenness: 0.0,
                in_degree: 0,
                out_degree: 0,
                instability: 0.0,
            }],
            edges: vec![],
            cycles: vec![],
            summary: AnalysisSummary::default(),
        };

        let dot = analyzer.to_dot(&analysis);
        assert!(dot.starts_with("digraph G"));
        assert!(dot.contains("rankdir=LR"));
        assert!(dot.contains("node [shape=box]"));
        assert!(dot.contains("PageRank"));
    }

    #[test]
    fn test_sanitize_mermaid_label() {
        assert_eq!(sanitize_mermaid_label("src/main.rs"), "src_main_rs");
        assert_eq!(sanitize_mermaid_label("my-file.ts"), "my_file_ts");
        assert_eq!(sanitize_mermaid_label("path/to/file"), "path_to_file");
    }

    #[test]
    fn test_analysis_summary() {
        let summary = AnalysisSummary {
            total_nodes: 10,
            total_edges: 15,
            avg_degree: 3.0,
            cycle_count: 2,
        };
        assert_eq!(summary.total_nodes, 10);
        assert_eq!(summary.total_edges, 15);
        assert!((summary.avg_degree - 3.0).abs() < 0.001);
        assert_eq!(summary.cycle_count, 2);
    }

    #[test]
    fn test_node_fields() {
        let node = Node {
            path: "test.rs".to_string(),
            pagerank: 0.42,
            betweenness: 0.15,
            in_degree: 3,
            out_degree: 2,
            instability: 0.4,
        };
        assert_eq!(node.path, "test.rs");
        assert!((node.pagerank - 0.42).abs() < 0.001);
        assert!((node.betweenness - 0.15).abs() < 0.001);
        assert_eq!(node.in_degree, 3);
        assert_eq!(node.out_degree, 2);
        assert!((node.instability - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_edge_fields() {
        let edge = Edge {
            from: "a.rs".to_string(),
            to: "b.rs".to_string(),
        };
        assert_eq!(edge.from, "a.rs");
        assert_eq!(edge.to, "b.rs");
    }

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("OrderSearcher"), "order_searcher");
        assert_eq!(camel_to_snake("ApplicationRecord"), "application_record");
        assert_eq!(camel_to_snake("HTTPClient"), "http_client");
        assert_eq!(camel_to_snake("Foo"), "foo");
        assert_eq!(camel_to_snake("FooBar"), "foo_bar");
        assert_eq!(camel_to_snake("already_snake"), "already_snake");
        assert_eq!(camel_to_snake("JSON"), "json");
        assert_eq!(camel_to_snake("HTMLParser"), "html_parser");
    }

    #[test]
    fn test_find_match_ruby_constant_to_snake_case() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/app/models/concerns/order_searcher.rb"),
            std::path::PathBuf::from("/project/app/models/concerns/feature_gate.rb"),
            std::path::PathBuf::from("/project/app/models/application_record.rb"),
        ];
        let index = FilePathIndex::new(&files, root);

        // CamelCase constant should resolve to snake_case file
        assert_eq!(
            index.find_match("OrderSearcher", Path::new("app/models/order.rb")),
            Some("app/models/concerns/order_searcher.rb".to_string())
        );
        assert_eq!(
            index.find_match("FeatureGate", Path::new("app/models/order.rb")),
            Some("app/models/concerns/feature_gate.rb".to_string())
        );
        assert_eq!(
            index.find_match("ApplicationRecord", Path::new("app/models/order.rb")),
            Some("app/models/application_record.rb".to_string())
        );
    }

    #[test]
    fn test_find_match_ruby_scoped_constant() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/app/models/concerns/active_model/validations.rb"),
            std::path::PathBuf::from("/project/lib/active_record/base.rb"),
        ];
        let index = FilePathIndex::new(&files, root);

        // Scoped constant ActiveModel::Validations -> active_model/validations
        assert_eq!(
            index.find_match("ActiveModel::Validations", Path::new("app/models/user.rb")),
            Some("app/models/concerns/active_model/validations.rb".to_string())
        );
        assert_eq!(
            index.find_match("ActiveRecord::Base", Path::new("app/models/user.rb")),
            Some("lib/active_record/base.rb".to_string())
        );
    }

    #[test]
    fn test_find_match_scoped_constant_no_substring_collision() {
        let root = Path::new("/project");
        let files = vec![std::path::PathBuf::from(
            "/project/packages/connect/app/controllers/connect/sign_up_controller.rb",
        )];
        let index = FilePathIndex::new(&files, root);

        // T::Sig -> t/sig should NOT match connect/sign_up_controller.rb
        // (substring "t/sig" appears in "connect/sign_up" but not on segment boundaries)
        assert_eq!(
            index.find_match("T::Sig", Path::new("app/models/user.rb")),
            None
        );
    }
}
