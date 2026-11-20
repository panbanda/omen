//! A directed graph whose strongly connected components are single nodes. A
//! join of a value over each component that a component reaches then reads
//! each component once, also when many walks share parts of the graph.

use std::collections::HashMap;

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};

pub(super) struct Condensed {
    /// The component of each node.
    component: Vec<usize>,
    /// The nodes of each component, in ascending order.
    members: Vec<Vec<usize>>,
    /// The components that each component has an edge to, in ascending
    /// order, without the component itself.
    next: Vec<Vec<usize>>,
}

impl Condensed {
    /// `edges[node]` gives the nodes that `node` has an edge to.
    pub(super) fn new(edges: &[Vec<usize>]) -> Self {
        let mut graph = DiGraph::<(), ()>::with_capacity(edges.len(), 0);
        for _ in edges {
            graph.add_node(());
        }
        for (node, targets) in edges.iter().enumerate() {
            for target in targets {
                graph.add_edge(NodeIndex::new(node), NodeIndex::new(*target), ());
            }
        }
        let mut component = vec![0; edges.len()];
        let mut members = Vec::new();
        for (index, nodes) in tarjan_scc(&graph).into_iter().enumerate() {
            let mut nodes: Vec<usize> = nodes.into_iter().map(NodeIndex::index).collect();
            nodes.sort_unstable();
            for node in &nodes {
                component[*node] = index;
            }
            members.push(nodes);
        }
        let next = members
            .iter()
            .enumerate()
            .map(|(index, nodes)| {
                let mut next: Vec<usize> = nodes
                    .iter()
                    .flat_map(|node| &edges[*node])
                    .map(|target| component[*target])
                    .filter(|target| *target != index)
                    .collect();
                next.sort_unstable();
                next.dedup();
                next
            })
            .collect();
        Self {
            component,
            members,
            next,
        }
    }

    pub(super) fn component(&self, node: usize) -> usize {
        self.component[node]
    }

    pub(super) fn members(&self, component: usize) -> &[usize] {
        &self.members[component]
    }

    /// The join of `own` over each component that `start` reaches, `start`
    /// included. `known` has the result of each component whose walk is
    /// complete, and gets the results of this walk. The walk uses a stack,
    /// not recursion, thus a long chain does not overflow the stack.
    pub(super) fn join<T: Clone>(
        &self,
        start: usize,
        known: &mut HashMap<usize, T>,
        mut own: impl FnMut(usize) -> T,
        join: impl Fn(T, &T) -> T,
    ) -> T {
        let mut pending = vec![(start, false)];
        while let Some((component, expanded)) = pending.pop() {
            if known.contains_key(&component) {
                continue;
            }
            if !expanded {
                pending.push((component, true));
                for next in &self.next[component] {
                    if !known.contains_key(next) {
                        pending.push((*next, false));
                    }
                }
                continue;
            }
            // The condensed graph has no cycle, thus each next component is
            // complete before this one.
            let value = self.next[component]
                .iter()
                .fold(own(component), |value, next| join(value, &known[next]));
            known.insert(component, value);
        }
        known[&start].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Condensed;
    use std::collections::HashMap;

    fn reached(graph: &Condensed, start: usize) -> Vec<usize> {
        let mut known = HashMap::new();
        let mut nodes = graph.join(
            graph.component(start),
            &mut known,
            |component| graph.members(component).to_vec(),
            |mut value, next| {
                value.extend(next);
                value
            },
        );
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    #[test]
    fn join_reads_each_node_that_a_node_reaches_also_through_a_cycle() {
        // 0 -> 1 -> 2 -> 1, 2 -> 3, and 4 alone.
        let graph = Condensed::new(&[vec![1], vec![2], vec![1, 3], vec![], vec![]]);
        assert_eq!(graph.component(1), graph.component(2));
        assert_eq!(reached(&graph, 0), [0, 1, 2, 3]);
        assert_eq!(reached(&graph, 2), [1, 2, 3]);
        assert_eq!(reached(&graph, 3), [3]);
        assert_eq!(reached(&graph, 4), [4]);
    }

    #[test]
    fn join_reads_each_component_once_for_many_walks() {
        // A diamond: 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3.
        let graph = Condensed::new(&[vec![1, 2], vec![3], vec![3], vec![]]);
        let mut known = HashMap::new();
        let mut reads = Vec::new();
        for start in [3, 1, 0] {
            graph.join(
                graph.component(start),
                &mut known,
                |component| {
                    reads.push(component);
                    1
                },
                |value, next| value + next,
            );
        }
        reads.sort_unstable();
        let mut components: Vec<usize> = (0..4).map(|node| graph.component(node)).collect();
        components.sort_unstable();
        assert_eq!(reads, components);
    }
}
