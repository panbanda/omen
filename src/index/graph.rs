//! Graph algorithms that the analyzers share.
//!
//! This module has the only PageRank implementation and the only call to Tarjan's algorithm.
//! The results depend only on the node order and the edge order of the graph.

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Parameters of the PageRank power iteration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRankOptions {
    pub damping: f64,
    pub max_iterations: usize,
    /// The iteration stops when the sum of the absolute score changes is less than this value.
    pub tolerance: f64,
}

/// Calculates the PageRank score of each node with power iteration.
///
/// The score of a node is at the position `NodeIndex::index()` in the result.
///
/// A node without outgoing edges gives its score to no node, thus the scores
/// do not always add up to 1.
/// Parallel edges count one time each in the out-degree and in the incoming sum.
pub fn pagerank<N, E>(graph: &DiGraph<N, E>, options: &PageRankOptions) -> Vec<f64> {
    let node_count = graph.node_count();
    if node_count == 0 {
        return Vec::new();
    }

    let damping = options.damping;
    let mut rank = vec![1.0 / node_count as f64; node_count];
    let mut new_rank = vec![0.0; node_count];
    let out_degrees: Vec<usize> = graph
        .node_indices()
        .map(|node| graph.edges_directed(node, Direction::Outgoing).count())
        .collect();

    for _ in 0..options.max_iterations {
        let mut diff = 0.0;

        for node in graph.node_indices() {
            // Floating-point addition is not associative. Keep this summation
            // order, because a different order changes the low bits of the scores.
            let incoming: f64 = graph
                .edges_directed(node, Direction::Incoming)
                .map(|edge| {
                    let source = edge.source().index();
                    let out_degree = out_degrees[source];
                    if out_degree > 0 {
                        rank[source] / out_degree as f64
                    } else {
                        0.0
                    }
                })
                .sum();

            let new_score = (1.0 - damping) / node_count as f64 + damping * incoming;
            diff += (new_score - rank[node.index()]).abs();
            new_rank[node.index()] = new_score;
        }

        std::mem::swap(&mut rank, &mut new_rank);

        if diff < options.tolerance {
            break;
        }
    }

    rank
}

/// Finds the strongly connected components with Tarjan's algorithm.
///
/// The components are in reverse topological order.
pub fn strongly_connected_components<N, E>(graph: &DiGraph<N, E>) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(graph)
}

/// Finds the strongly connected components that contain a cycle.
///
/// A component contains a cycle if it has two or more nodes, or if its one node has a self loop.
pub fn cyclic_components<N, E>(graph: &DiGraph<N, E>) -> Vec<Vec<NodeIndex>> {
    strongly_connected_components(graph)
        .into_iter()
        .filter(|component| component.len() > 1 || graph.contains_edge(component[0], component[0]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_OPTIONS: PageRankOptions = PageRankOptions {
        damping: 0.85,
        max_iterations: 100,
        tolerance: 1e-6,
    };

    /// These options stop the iteration at the limit of three, before the scores converge.
    const EARLY_STOP_OPTIONS: PageRankOptions = PageRankOptions {
        damping: 0.5,
        max_iterations: 3,
        tolerance: 1e-2,
    };

    /// The expected values are the outputs of the implementations that this module replaced.
    /// The scores are IEEE 754 bit patterns, because the scores must stay bit-identical.
    struct Case {
        name: &'static str,
        node_count: usize,
        edges: &'static [(usize, usize)],
        default_scores: &'static [u64],
        early_stop_scores: &'static [u64],
        components: &'static [&'static [usize]],
        cycles: &'static [&'static [usize]],
    }

    const CASES: &[Case] = &[
        Case {
            name: "empty graph",
            node_count: 0,
            edges: &[],
            default_scores: &[],
            early_stop_scores: &[],
            components: &[],
            cycles: &[],
        },
        Case {
            name: "single node",
            node_count: 1,
            edges: &[],
            default_scores: &[0x3fc3333333333334],
            early_stop_scores: &[0x3fe0000000000000],
            components: &[&[0]],
            cycles: &[],
        },
        Case {
            name: "self loop",
            node_count: 1,
            edges: &[(0, 0)],
            default_scores: &[0x3ff0000000000000],
            early_stop_scores: &[0x3ff0000000000000],
            components: &[&[0]],
            cycles: &[&[0]],
        },
        Case {
            name: "self loop with a second source",
            node_count: 2,
            edges: &[(0, 0), (1, 0)],
            default_scores: &[0x3fed99999999999a, 0x3fb3333333333334],
            early_stop_scores: &[0x3fe8000000000000, 0x3fd0000000000000],
            components: &[&[0], &[1]],
            cycles: &[&[0]],
        },
        Case {
            name: "cycle",
            node_count: 3,
            edges: &[(0, 1), (1, 2), (2, 0)],
            default_scores: &[0x3fd5555555555555, 0x3fd5555555555555, 0x3fd5555555555555],
            early_stop_scores: &[0x3fd5555555555555, 0x3fd5555555555555, 0x3fd5555555555555],
            components: &[&[2, 1, 0]],
            cycles: &[&[2, 1, 0]],
        },
        Case {
            name: "two components",
            node_count: 4,
            edges: &[(0, 1), (2, 3)],
            default_scores: &[
                0x3fa3333333333334,
                0x3fb1c28f5c28f5c3,
                0x3fa3333333333334,
                0x3fb1c28f5c28f5c3,
            ],
            early_stop_scores: &[
                0x3fc0000000000000,
                0x3fc8000000000000,
                0x3fc0000000000000,
                0x3fc8000000000000,
            ],
            components: &[&[1], &[0], &[3], &[2]],
            cycles: &[],
        },
        Case {
            name: "two cycles",
            node_count: 5,
            edges: &[(3, 4), (4, 3), (0, 1), (1, 0)],
            default_scores: &[
                0x3fc999999999999a,
                0x3fc999999999999a,
                0x3f9eb851eb851eba,
                0x3fc999999999999a,
                0x3fc999999999999a,
            ],
            early_stop_scores: &[
                0x3fc999999999999a,
                0x3fc999999999999a,
                0x3fb999999999999a,
                0x3fc999999999999a,
                0x3fc999999999999a,
            ],
            components: &[&[1, 0], &[2], &[4, 3]],
            cycles: &[&[1, 0], &[4, 3]],
        },
        Case {
            name: "dangling node",
            node_count: 3,
            edges: &[(0, 1), (0, 2), (1, 2)],
            default_scores: &[0x3fa999999999999b, 0x3fb23d70a3d70a3e, 0x3fc0df3b645a1cad],
            early_stop_scores: &[0x3fc5555555555555, 0x3fcaaaaaaaaaaaaa, 0x3fd4000000000000],
            components: &[&[2], &[1], &[0]],
            cycles: &[],
        },
        Case {
            name: "ties",
            node_count: 4,
            edges: &[(1, 0), (2, 0), (3, 0)],
            default_scores: &[
                0x3fc10a3d70a3d70b,
                0x3fa3333333333334,
                0x3fa3333333333334,
                0x3fa3333333333334,
            ],
            early_stop_scores: &[
                0x3fd4000000000000,
                0x3fc0000000000000,
                0x3fc0000000000000,
                0x3fc0000000000000,
            ],
            components: &[&[0], &[1], &[2], &[3]],
            cycles: &[],
        },
        Case {
            name: "parallel edges",
            node_count: 3,
            edges: &[(0, 1), (0, 1), (0, 2), (2, 0)],
            default_scores: &[0x3fbf312f92555e78, 0x3fbe79c140384db8, 0x3fb5a34706828d42],
            early_stop_scores: &[0x3fd1c71c71c71c72, 0x3fd097b425ed097b, 0x3fcb425ed097b426],
            components: &[&[1], &[2, 0]],
            cycles: &[&[2, 0]],
        },
        Case {
            name: "cycle with a tail and a dangling node",
            node_count: 7,
            edges: &[
                (0, 1),
                (1, 2),
                (2, 0),
                (2, 3),
                (3, 4),
                (5, 3),
                (5, 0),
                (4, 4),
                (1, 3),
            ],
            default_scores: &[
                0x3faa51dcf4843e3c,
                0x3fb0abf17f475922,
                0x3fa9247003f554e8,
                0x3fb43eceb6c071cd,
                0x3fe2e96f0c759956,
                0x3f95f15f15f15f17,
                0x3f95f15f15f15f17,
            ],
            early_stop_scores: &[
                0x3fbdb6db6db6db6d,
                0x3fc0924924924924,
                0x3fbb6db6db6db6db,
                0x3fc36db6db6db6db,
                0x3fd2000000000000,
                0x3fb2492492492492,
                0x3fb2492492492492,
            ],
            components: &[&[4], &[3], &[2, 1, 0], &[5], &[6]],
            cycles: &[&[4], &[2, 1, 0]],
        },
    ];

    fn build<N>(case: &Case, weight: impl Fn(usize) -> N) -> DiGraph<N, ()> {
        let mut graph = DiGraph::new();
        let nodes: Vec<NodeIndex> = (0..case.node_count)
            .map(|position| graph.add_node(weight(position)))
            .collect();
        for &(source, target) in case.edges {
            graph.add_edge(nodes[source], nodes[target], ());
        }
        graph
    }

    fn score_bits<N>(graph: &DiGraph<N, ()>, options: &PageRankOptions) -> Vec<u64> {
        pagerank(graph, options)
            .into_iter()
            .map(f64::to_bits)
            .collect()
    }

    fn positions(components: Vec<Vec<NodeIndex>>) -> Vec<Vec<usize>> {
        components
            .into_iter()
            .map(|component| component.into_iter().map(NodeIndex::index).collect())
            .collect()
    }

    /// The file graph has `String` nodes and the symbol graph has `usize` nodes.
    /// The node weight must not change the scores.
    #[test]
    fn test_pagerank_pins_scores_for_default_options() {
        for case in CASES {
            let file_graph = build(case, |position| position.to_string());
            let symbol_graph = build(case, |position| position);
            assert_eq!(
                score_bits(&file_graph, &DEFAULT_OPTIONS),
                case.default_scores,
                "{}",
                case.name
            );
            assert_eq!(
                score_bits(&symbol_graph, &DEFAULT_OPTIONS),
                case.default_scores,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn test_pagerank_pins_scores_for_early_stop_options() {
        for case in CASES {
            let file_graph = build(case, |position| position.to_string());
            let symbol_graph = build(case, |position| position);
            assert_eq!(
                score_bits(&file_graph, &EARLY_STOP_OPTIONS),
                case.early_stop_scores,
                "{}",
                case.name
            );
            assert_eq!(
                score_bits(&symbol_graph, &EARLY_STOP_OPTIONS),
                case.early_stop_scores,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn test_strongly_connected_components_pins_order() {
        for case in CASES {
            let graph = build(case, |position| position);
            assert_eq!(
                positions(strongly_connected_components(&graph)),
                case.components,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn test_cyclic_components_pins_order() {
        for case in CASES {
            let graph = build(case, |position| position.to_string());
            assert_eq!(
                positions(cyclic_components(&graph)),
                case.cycles,
                "{}",
                case.name
            );
        }
    }
}
