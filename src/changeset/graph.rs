//! The node cap of the change graph.

use std::collections::BTreeSet;

use super::model::{ChangeGraph, ChangeState, GraphEdge, GraphNode};

/// The nodes of one component after the cap.
#[derive(Debug, PartialEq, Eq)]
pub struct CappedComponent<'a> {
    pub name: &'a str,
    pub nodes: Vec<&'a GraphNode>,
    /// The number of nodes of the component that the cap left out.
    pub omitted: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CappedGraph<'a> {
    /// The components in name order.
    pub components: Vec<CappedComponent<'a>>,
    /// The edges between kept nodes, in key order.
    pub edges: Vec<&'a GraphEdge>,
}

/// Keeps at most `max_nodes` nodes. A node that changed has priority over a
/// node that gives context. In each group the nodes are in key order, thus
/// two runs keep the same nodes.
pub fn cap(graph: &ChangeGraph, max_nodes: usize) -> CappedGraph<'_> {
    let mut ordered: Vec<&GraphNode> = graph.nodes.iter().collect();
    ordered.sort_by(|left, right| {
        (left.state == ChangeState::Unchanged)
            .cmp(&(right.state == ChangeState::Unchanged))
            .then_with(|| left.key.cmp(&right.key))
    });
    let (kept, left_out) = ordered.split_at(max_nodes.min(ordered.len()));
    let kept_keys: BTreeSet<&str> = kept.iter().map(|node| node.key.as_str()).collect();

    let names: BTreeSet<&str> = graph
        .nodes
        .iter()
        .map(|node| node.component.as_str())
        .collect();
    let components = names
        .into_iter()
        .filter_map(|name| {
            let mut nodes: Vec<&GraphNode> = kept
                .iter()
                .copied()
                .filter(|node| node.component == name)
                .collect();
            nodes.sort_by(|left, right| left.key.cmp(&right.key));
            let omitted = left_out
                .iter()
                .filter(|node| node.component == name)
                .count();
            (!nodes.is_empty() || omitted > 0).then_some(CappedComponent {
                name,
                nodes,
                omitted,
            })
        })
        .collect();
    let mut edges: Vec<&GraphEdge> = graph
        .edges
        .iter()
        .filter(|edge| {
            kept_keys.contains(edge.from.as_str()) && kept_keys.contains(edge.to.as_str())
        })
        .collect();
    edges.sort_by(|left, right| (&left.from, &left.to).cmp(&(&right.from, &right.to)));
    CappedGraph { components, edges }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn node(key: &str, component: &str, state: ChangeState) -> GraphNode {
        GraphNode {
            key: key.to_string(),
            label: key.rsplit('#').next().unwrap_or(key).to_string(),
            file: format!("{component}/file.rs"),
            component: component.to_string(),
            state,
        }
    }

    pub fn edge(from: &str, to: &str, state: ChangeState) -> GraphEdge {
        GraphEdge {
            from: from.to_string(),
            to: to.to_string(),
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{edge, node};
    use super::*;

    fn sample() -> ChangeGraph {
        ChangeGraph {
            nodes: vec![
                node("b/x.rs#context", "b", ChangeState::Unchanged),
                node("a/x.rs#alpha", "a", ChangeState::Changed),
                node("a/x.rs#beta", "a", ChangeState::Added),
                node("b/x.rs#gamma", "b", ChangeState::Removed),
            ],
            edges: vec![
                edge("b/x.rs#context", "a/x.rs#alpha", ChangeState::Unchanged),
                edge("a/x.rs#beta", "a/x.rs#alpha", ChangeState::Added),
            ],
        }
    }

    #[test]
    fn test_cap_keeps_every_node_under_the_limit() {
        let graph = sample();
        let capped = cap(&graph, 30);
        let names: Vec<&str> = capped.components.iter().map(|c| c.name).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(capped.components.iter().all(|c| c.omitted == 0));
        assert_eq!(capped.edges.len(), 2);
    }

    #[test]
    fn test_cap_prefers_changed_nodes_to_context_nodes() {
        let graph = sample();
        let capped = cap(&graph, 3);
        let kept: Vec<&str> = capped
            .components
            .iter()
            .flat_map(|c| c.nodes.iter().map(|n| n.key.as_str()))
            .collect();
        assert_eq!(kept, ["a/x.rs#alpha", "a/x.rs#beta", "b/x.rs#gamma"]);
        let b = capped.components.iter().find(|c| c.name == "b").unwrap();
        assert_eq!(b.omitted, 1);
    }

    #[test]
    fn test_cap_drops_an_edge_that_has_an_omitted_end() {
        let graph = sample();
        let capped = cap(&graph, 3);
        assert_eq!(capped.edges.len(), 1);
        assert_eq!(capped.edges[0].from, "a/x.rs#beta");
    }

    #[test]
    fn test_cap_of_zero_leaves_a_more_node_for_each_component() {
        let graph = sample();
        let capped = cap(&graph, 0);
        assert!(capped.components.iter().all(|c| c.nodes.is_empty()));
        let omitted: Vec<usize> = capped.components.iter().map(|c| c.omitted).collect();
        assert_eq!(omitted, [2, 2]);
    }

    #[test]
    fn test_cap_does_not_depend_on_the_order_of_the_nodes() {
        let graph = sample();
        let mut reversed = sample();
        reversed.nodes.reverse();
        assert_eq!(cap(&graph, 2).components, cap(&reversed, 2).components);
    }
}
