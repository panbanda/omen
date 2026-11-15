//! The graph prior of the search experiment: the PageRank of each function,
//! method, and constructor in the call graph of the shared index, divided by
//! the largest PageRank. See `docs/benchmarks/search-experiments.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use petgraph::graph::DiGraph;

use crate::core::Result;
use crate::index::graph::{pagerank, PageRankOptions};
use crate::index::model::SymbolKind;
use crate::index::view::{CallView, ViewCache, DEFAULT_MIN_BASIS};

const OPTIONS: PageRankOptions = PageRankOptions {
    damping: 0.85,
    max_iterations: 100,
    tolerance: 1e-6,
};

/// The file, the name, and the first line of a definition.
pub type DefinitionPlace = (String, String, u32);

/// The PageRank of each node divided by the largest PageRank. Each value is in
/// the range 0 to 1.
pub fn normalised_pagerank(node_count: usize, edges: &[(usize, usize)]) -> Vec<f64> {
    let mut graph: DiGraph<(), ()> = DiGraph::new();
    let nodes: Vec<_> = (0..node_count).map(|_| graph.add_node(())).collect();
    for &(from, to) in edges {
        graph.add_edge(nodes[from], nodes[to], ());
    }
    let scores = pagerank(&graph, &OPTIONS);
    let largest = scores.iter().copied().fold(0.0, f64::max);
    if largest <= 0.0 {
        return scores;
    }
    scores.into_iter().map(|score| score / largest).collect()
}

/// The prior of each callable definition of the view. A definition that is not
/// a function, a method, or a constructor has no entry, and its prior is 0.
pub fn priors_of(view: &CallView) -> HashMap<DefinitionPlace, f64> {
    let is_callable = |kind: SymbolKind| {
        matches!(
            kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        )
    };
    let mut node_of = vec![None; view.symbols.len()];
    let mut positions = Vec::new();
    for (position, symbol) in view.symbols.iter().enumerate() {
        if is_callable(symbol.kind) {
            node_of[position] = Some(positions.len());
            positions.push(position);
        }
    }
    let edges: Vec<(usize, usize)> = view
        .edges(DEFAULT_MIN_BASIS)
        .into_iter()
        .filter_map(|(caller, callee)| Some((node_of[caller]?, node_of[callee]?)))
        .collect();
    let scores = normalised_pagerank(positions.len(), &edges);
    positions
        .iter()
        .zip(scores)
        .map(|(&position, score)| {
            let symbol = &view.symbols[position];
            ((symbol.file.clone(), symbol.name.clone(), symbol.line), score)
        })
        .collect()
}

/// The priors of the files below `root`. A file path is relative to `root`.
pub fn priors_of_files(root: &Path, files: &[PathBuf]) -> Result<HashMap<DefinitionPlace, f64>> {
    let absolute: Vec<PathBuf> = files.iter().map(|file| root.join(file)).collect();
    let view = ViewCache::default().load(root, &absolute)?;
    Ok(priors_of(&view))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_called_node_has_prior_one_and_the_others_are_below_one() {
        let scores = normalised_pagerank(3, &[(0, 2), (1, 2)]);

        assert_eq!(scores[2], 1.0);
        assert!(scores[0] > 0.0 && scores[0] < 1.0, "{scores:?}");
        assert!(scores[1] > 0.0 && scores[1] < 1.0, "{scores:?}");
    }

    #[test]
    fn a_graph_without_nodes_has_no_priors() {
        assert!(normalised_pagerank(0, &[]).is_empty());
    }

    #[test]
    fn the_same_graph_gives_the_same_bits() {
        let edges = [(0, 1), (1, 2), (2, 0), (3, 2)];
        let bits = |scores: Vec<f64>| scores.into_iter().map(f64::to_bits).collect::<Vec<_>>();

        assert_eq!(
            bits(normalised_pagerank(4, &edges)),
            bits(normalised_pagerank(4, &edges))
        );
    }

    #[test]
    fn a_called_function_has_a_higher_prior_than_its_caller() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("lib.rs"),
            "fn helper() {}\nfn first() { helper(); }\nfn second() { helper(); }\n",
        )
        .unwrap();

        let priors = priors_of_files(directory.path(), &[PathBuf::from("lib.rs")]).unwrap();

        let prior = |name: &str, line: u32| priors[&("lib.rs".to_string(), name.to_string(), line)];
        assert_eq!(prior("helper", 1), 1.0);
        assert!(prior("first", 2) < 1.0);
    }
}
