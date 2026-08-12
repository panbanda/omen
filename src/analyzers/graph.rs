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

use crate::analyzers::import_resolver::{
    build_dependency_graph, extract_resolved_imports, ImportIndex,
};
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::{extract_imports, ImportKind, Parser};

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
        let file_paths: Vec<std::path::PathBuf> = files.iter().map(|p| (*p).clone()).collect();

        let file_imports: Vec<(String, Vec<String>)> = if self.config.resolve_imports {
            // Shared with `smells` (same resolver AND same extraction step)
            // so the two analyzers can never disagree on the edges they
            // build for the same input (issue #479).
            let file_index = ImportIndex::new(&file_paths, ctx.root);
            extract_resolved_imports(&file_paths, ctx, &file_index, self.config.include_external)
        } else {
            // No CLI flag ever sets `resolve_imports = false`; kept for API
            // completeness. Uses raw unresolved import specifiers as
            // pseudo-targets instead of resolving them to files.
            files
                .par_iter()
                .filter_map(|file| {
                    let rel_path = file.strip_prefix(ctx.root).unwrap_or(file);
                    let path_str = rel_path.to_string_lossy().to_string();

                    let content = ctx.read_file(file).ok()?;
                    let lang = Language::detect(file)?;

                    let parser = Parser::new();
                    let result = parser.parse(&content, lang, file).ok()?;
                    let imports: Vec<String> = extract_imports(&result)
                        .into_iter()
                        .filter(|imp| imp.kind == ImportKind::Use)
                        .map(|imp| imp.path)
                        .collect();

                    Some((path_str, imports))
                })
                .collect()
        };

        // Build graph applying the shared edge rules (no self-loops, no
        // parallel edges for a repeated import) -- the same rules `smells`
        // applies, via the same function.
        let (graph, node_indices) =
            build_dependency_graph(&file_imports, self.config.include_external);

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
        let initial_rank = 1.0 / n as f64;
        let mut rank = vec![initial_rank; n];
        let mut new_rank = vec![0.0; n];
        let out_degrees: Vec<usize> = graph
            .node_indices()
            .map(|node| graph.edges_directed(node, Direction::Outgoing).count())
            .collect();

        for _ in 0..self.config.max_iterations {
            let mut diff = 0.0;

            for node in graph.node_indices() {
                let incoming: f64 = graph
                    .edges_directed(node, Direction::Incoming)
                    .map(|e| {
                        let source = e.source();
                        let out_deg = out_degrees[source.index()];
                        if out_deg > 0 {
                            rank[source.index()] / out_deg as f64
                        } else {
                            0.0
                        }
                    })
                    .sum();

                let new_score = (1.0 - d) / n as f64 + d * incoming;
                diff += (new_score - rank[node.index()]).abs();
                new_rank[node.index()] = new_score;
            }

            std::mem::swap(&mut rank, &mut new_rank);

            if diff < self.config.tolerance {
                break;
            }
        }

        graph
            .node_indices()
            .map(|node| (node, rank[node.index()]))
            .collect()
    }

    /// Calculate betweenness centrality using Brandes' algorithm with parallel BFS.
    fn calculate_betweenness(&self, graph: &DiGraph<String, ()>) -> HashMap<NodeIndex, f64> {
        let n = graph.node_count();
        if n <= 2 {
            return graph.node_indices().map(|idx| (idx, 0.0)).collect();
        }

        // Use all nodes as sources (no sampling - per project requirements)
        let sources: Vec<NodeIndex> = graph.node_indices().collect();

        struct BetweennessState {
            totals: Vec<f64>,
            dist: Vec<i32>,
            sigma: Vec<f64>,
            delta: Vec<f64>,
            predecessors: Vec<Vec<u32>>,
            stack: Vec<usize>,
            queue: VecDeque<usize>,
        }

        impl BetweennessState {
            fn new(n: usize) -> Self {
                Self {
                    totals: vec![0.0; n],
                    dist: vec![-1; n],
                    sigma: vec![0.0; n],
                    delta: vec![0.0; n],
                    predecessors: (0..n).map(|_| Vec::new()).collect(),
                    stack: Vec::with_capacity(n),
                    queue: VecDeque::with_capacity(n),
                }
            }

            fn reset_scratch(&mut self) {
                self.dist.fill(-1);
                self.sigma.fill(0.0);
                self.delta.fill(0.0);
                for predecessors in &mut self.predecessors {
                    predecessors.clear();
                }
                self.stack.clear();
                self.queue.clear();
            }
        }

        // Fold source contributions into per-worker totals, then merge worker
        // totals incrementally to keep peak memory proportional to worker count.
        let state = sources
            .par_iter()
            .fold(
                || BetweennessState::new(n),
                |mut state, &source| {
                    state.reset_scratch();
                    let source_idx = source.index();
                    state.dist[source_idx] = 0;
                    state.sigma[source_idx] = 1.0;
                    state.queue.push_back(source_idx);

                    // BFS
                    while let Some(v) = state.queue.pop_front() {
                        state.stack.push(v);
                        let v_dist = state.dist[v];

                        for edge in graph.edges_directed(NodeIndex::new(v), Direction::Outgoing) {
                            let w = edge.target().index();

                            // First visit
                            if state.dist[w] < 0 {
                                state.dist[w] = v_dist + 1;
                                state.queue.push_back(w);
                            }

                            // Shortest path via v
                            if state.dist[w] == v_dist + 1 {
                                state.sigma[w] += state.sigma[v];
                                state.predecessors[w].push(v as u32);
                            }
                        }
                    }

                    // Accumulate dependencies
                    while let Some(w) = state.stack.pop() {
                        for predecessor_index in 0..state.predecessors[w].len() {
                            let v = state.predecessors[w][predecessor_index] as usize;
                            let coeff = (state.sigma[v] / state.sigma[w]) * (1.0 + state.delta[w]);
                            state.delta[v] += coeff;
                        }
                        if w != source_idx {
                            state.totals[w] += state.delta[w];
                        }
                    }

                    state
                },
            )
            .reduce(
                || BetweennessState::new(n),
                |mut left, right| {
                    for (left_value, right_value) in left.totals.iter_mut().zip(right.totals) {
                        *left_value += right_value;
                    }
                    left
                },
            );

        // Normalize betweenness scores
        let norm = if n > 2 {
            1.0 / ((n - 1) * (n - 2)) as f64
        } else {
            1.0
        };

        let mut betweenness: HashMap<NodeIndex, f64> = graph
            .node_indices()
            .map(|node| (node, state.totals[node.index()]))
            .collect();
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

    /// Write files (relative path -> content) into a fresh temp dir and build
    /// the dependency graph over it from the filesystem.
    fn analyze_fixture(files: &[(&str, &str)]) -> Analysis {
        let temp_dir = tempfile::tempdir().unwrap();
        for (rel_path, content) in files {
            let full_path = temp_dir.path().join(rel_path);
            std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
            std::fs::write(full_path, content).unwrap();
        }

        let analyzer = Analyzer::new();
        let mut analysis = analyzer.analyze_project(temp_dir.path()).unwrap();
        analysis.summary.cycle_count = analysis.cycles.len();
        analysis
    }

    #[test]
    fn test_no_false_cycle_for_rust_sibling_modules_via_parent() {
        // Bug 2: tdg and defect each import a sibling module (hotspot)
        // through a grouped `use crate::analyzers::{hotspot};`. Neither tdg
        // nor defect actually depend on each other, or on analyzers/mod.rs
        // as anything but their container -- so no cycle should be reported.
        let analysis = analyze_fixture(&[
            ("src/lib.rs", "pub mod analyzers;\n"),
            (
                "src/analyzers/mod.rs",
                "pub mod tdg;\npub mod defect;\npub mod hotspot;\n",
            ),
            (
                "src/analyzers/tdg.rs",
                "use crate::analyzers::{hotspot};\npub fn a() {}\n",
            ),
            (
                "src/analyzers/defect.rs",
                "use crate::analyzers::{hotspot};\npub fn b() {}\n",
            ),
            ("src/analyzers/hotspot.rs", "pub fn h() {}\n"),
        ]);

        assert_eq!(
            analysis.summary.cycle_count, 0,
            "no cycle should be reported for sibling modules through a parent, found: {:?}",
            analysis.cycles
        );
    }

    #[test]
    fn test_real_rust_cycle_is_still_detected() {
        // Do not over-correct: a genuine two-module cycle must still be caught.
        let analysis = analyze_fixture(&[
            ("src/lib.rs", "pub mod a;\npub mod b;\n"),
            ("src/a.rs", "use crate::b;\npub fn f() { b::g(); }\n"),
            ("src/b.rs", "use crate::a;\npub fn g() { a::f(); }\n"),
        ]);

        assert_eq!(
            analysis.summary.cycle_count, 1,
            "the real a.rs <-> b.rs cycle must still be reported, cycles: {:?}",
            analysis.cycles
        );
    }

    #[test]
    fn test_real_rust_cycle_via_item_not_submodule_is_still_detected() {
        // Regression: `use crate::a::{b};` where `b` is a function defined
        // directly in a.rs (not a submodule file `a/b.rs`) must still
        // resolve to a.rs. Over-correcting the grouped-import expansion so
        // that per-leaf resolution gives up when `a/b.rs` doesn't exist
        // silently drops the real a.rs <-> c.rs dependency and hides the
        // cycle.
        let analysis = analyze_fixture(&[
            ("src/lib.rs", "pub mod a;\npub mod c;\n"),
            ("src/a.rs", "use crate::c;\npub fn b() {}\n"),
            ("src/c.rs", "use crate::a::{b};\n"),
        ]);

        assert_eq!(
            analysis.summary.cycle_count, 1,
            "the real a.rs <-> c.rs cycle must still be reported, cycles: {:?}",
            analysis.cycles
        );
    }

    #[test]
    fn test_graph_and_smells_agree_on_edges_and_cycles() {
        // graph and smells must build the SAME edge set -- not just the same
        // cycle count -- for the same input, since they now share both the
        // resolver and the extraction/edge-building step. Includes: a real
        // cross-package cycle, a self-import (must not become a self-loop
        // edge in either), and a barrel re-export (`export * from`, the
        // issue #479 monorepo pattern).
        use crate::analyzers::import_resolver::{build_dependency_graph, extract_resolved_imports};
        use crate::analyzers::smells;
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet};
        use std::collections::HashSet;

        let files: &[(&str, &str)] = &[
            (
                "packages/brain/src/types.ts",
                "import type { M } from '../../mcp/src/types.js';\nexport type B = M;\n",
            ),
            ("packages/mcp/src/types.ts", "export type M = number;\n"),
            ("a.ts", "import { b } from './b.js';\nexport const a = b;\n"),
            ("b.ts", "import { a } from './a.js';\nexport const b = a;\n"),
            (
                "self.ts",
                "import { helper } from './self.js';\nexport function helper() {}\n",
            ),
            ("pkg/index.ts", "export * from './widget';\n"),
            ("pkg/widget.ts", "export const widget = 1;\n"),
        ];

        let temp_dir = tempfile::tempdir().unwrap();
        for (rel_path, content) in files {
            let full_path = temp_dir.path().join(rel_path);
            std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
            std::fs::write(full_path, content).unwrap();
        }

        let graph_analysis = Analyzer::new().analyze_project(temp_dir.path()).unwrap();

        let config = Config::default();
        let file_set = FileSet::from_path(temp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));
        let smells_analysis = smells::Analyzer::new().analyze_repo(&ctx).unwrap();

        // graph's public edge list, as a set: must have no self-loops and no
        // duplicate (parallel) edges.
        let graph_edges: HashSet<(String, String)> = graph_analysis
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        assert_eq!(
            graph_edges.len(),
            graph_analysis.edges.len(),
            "graph's edge list must not contain duplicates"
        );
        assert!(
            graph_edges.iter().all(|(from, to)| from != to),
            "graph's edge list must not contain self-loops: {:?}",
            graph_edges
        );

        // Independently rebuild the canonical edge set via the exact shared
        // functions both analyzers call, and require graph's public edges
        // to match it exactly.
        let all_files: Vec<_> = file_set.iter().cloned().collect();
        let file_index = ImportIndex::new(&all_files, temp_dir.path());
        let file_imports = extract_resolved_imports(&all_files, &ctx, &file_index, false);
        let (canonical_graph, canonical_indices) = build_dependency_graph(&file_imports, false);
        let canonical_edges: HashSet<(String, String)> = canonical_graph
            .edge_indices()
            .map(|e| {
                let (from, to) = canonical_graph.edge_endpoints(e).unwrap();
                (canonical_graph[from].clone(), canonical_graph[to].clone())
            })
            .collect();
        assert_eq!(
            graph_edges, canonical_edges,
            "graph's public edges must match the canonical shared-builder edge set"
        );

        // smells doesn't expose raw edges, but its per-file fan_in/fan_out
        // are derived directly from its internal graph's degrees -- if that
        // graph has the same edge set as the canonical one, these must
        // match graph's public in_degree/out_degree for every file exactly.
        let graph_degrees: HashMap<&str, (usize, usize)> = graph_analysis
            .nodes
            .iter()
            .map(|n| (n.path.as_str(), (n.in_degree, n.out_degree)))
            .collect();
        for component in &smells_analysis.components {
            let (expected_in, expected_out) = graph_degrees
                .get(component.id.as_str())
                .unwrap_or_else(|| panic!("graph has no node for {}", component.id));
            assert_eq!(
                (component.fan_in, component.fan_out),
                (*expected_in, *expected_out),
                "fan_in/fan_out for {} must match graph's in_degree/out_degree",
                component.id
            );
        }
        assert_eq!(smells_analysis.components.len(), graph_analysis.nodes.len());

        // canonical builder must also agree with graph's degree sequence,
        // and with cycle counts on both sides.
        for (path, &idx) in &canonical_indices {
            let in_deg = canonical_graph
                .edges_directed(idx, Direction::Incoming)
                .count();
            let out_deg = canonical_graph
                .edges_directed(idx, Direction::Outgoing)
                .count();
            let (expected_in, expected_out) = graph_degrees[path.as_str()];
            assert_eq!((in_deg, out_deg), (expected_in, expected_out));
        }

        assert_eq!(
            graph_analysis.cycles.len(),
            smells_analysis.summary.cyclic_count,
            "graph and smells must agree on cycle count: graph={:?} smells cyclic_count={}",
            graph_analysis.cycles,
            smells_analysis.summary.cyclic_count
        );
        // The known real cycle (a.ts <-> b.ts) is the only one either should
        // find; the self-import and the barrel re-export must not add any.
        assert_eq!(graph_analysis.cycles.len(), 1);
    }

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
    fn test_pagerank_pins_asymmetric_fixture_scores() {
        let analyzer = Analyzer::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        let c = graph.add_node("c.rs".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(a, c, ());
        graph.add_edge(b, c, ());

        let ranks = analyzer.calculate_pagerank(&graph);

        assert!((ranks[&a] - 0.05).abs() < 1e-12);
        assert!((ranks[&b] - 0.07125).abs() < 1e-12);
        assert!((ranks[&c] - 0.1318125).abs() < 1e-12);
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
}
