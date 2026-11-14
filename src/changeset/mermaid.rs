//! The Mermaid `flowchart LR` of a change graph.
//!
//! The output uses only this subset of the Mermaid syntax, which the test
//! parser in this file checks:
//!
//! - the header `flowchart LR`
//! - `subgraph id["label"]` and `end`, one level deep
//! - the node `id["label"]`
//! - the edge `from --> to` and the dashed edge `from -.-> to`
//! - `classDef name property:value,...`
//! - `class id,id name`
//! - `linkStyle index,index property:value,...`

use super::graph::{cap, CappedGraph};
use std::collections::BTreeMap;

use super::model::{ChangeGraph, ChangeState, GraphNode};

/// The colours of the four states. The report uses the same colours.
const STYLES: [(ChangeState, &str); 4] = [
    (
        ChangeState::Added,
        "fill:#dafbe1,stroke:#1a7f37,color:#1f2328",
    ),
    (
        ChangeState::Removed,
        "fill:#ffebe9,stroke:#cf222e,color:#1f2328",
    ),
    (
        ChangeState::Changed,
        "fill:#fff8c5,stroke:#9a6700,color:#1f2328",
    ),
    (
        ChangeState::Unchanged,
        "fill:#f6f8fa,stroke:#8c959f,color:#1f2328",
    ),
];

/// The text of the diagram, or `None` when the graph has no node.
pub fn render(graph: &ChangeGraph, max_nodes: usize) -> Option<String> {
    if graph.nodes.is_empty() {
        return None;
    }
    let capped = cap(graph, max_nodes);
    if capped.components.is_empty() {
        return None;
    }
    Some(render_capped(&capped))
}

pub(crate) fn escape(label: &str) -> String {
    label
        .replace('&', "#amp;")
        .replace('"', "#quot;")
        .replace('<', "#lt;")
        .replace('>', "#gt;")
        .replace('\n', "#10;")
        .replace('\r', "#13;")
}

fn render_capped(capped: &CappedGraph<'_>) -> String {
    let mut out = String::from("flowchart LR\n");
    let mut ids: BTreeMap<&str, String> = BTreeMap::new();
    let mut by_state: BTreeMap<ChangeState, Vec<String>> = BTreeMap::new();
    let mut next = 0usize;
    for (position, component) in capped.components.iter().enumerate() {
        out.push_str(&format!(
            "  subgraph c{position}[\"{}\"]\n",
            escape(component.name)
        ));
        for node in &component.nodes {
            let id = format!("n{next}");
            next += 1;
            out.push_str(&format!("    {id}[\"{}\"]\n", escape(&label_of(node))));
            by_state.entry(node.state).or_default().push(id.clone());
            ids.insert(node.key.as_str(), id);
        }
        if component.omitted > 0 {
            let id = format!("n{next}");
            next += 1;
            out.push_str(&format!("    {id}[\"+{} more\"]\n", component.omitted));
            by_state.entry(ChangeState::Unchanged).or_default().push(id);
        }
        out.push_str("  end\n");
    }
    let mut removed_links = Vec::new();
    let mut added_links = Vec::new();
    let mut index = 0usize;
    for edge in &capped.edges {
        let (Some(from), Some(to)) = (ids.get(edge.from.as_str()), ids.get(edge.to.as_str()))
        else {
            continue;
        };
        let arrow = match edge.state {
            ChangeState::Removed => {
                removed_links.push(index);
                "-.->"
            }
            ChangeState::Added => {
                added_links.push(index);
                "-->"
            }
            _ => "-->",
        };
        out.push_str(&format!("  {from} {arrow} {to}\n"));
        index += 1;
    }
    for (state, style) in STYLES {
        out.push_str(&format!("  classDef {} {style}\n", state.name()));
    }
    for (state, members) in &by_state {
        out.push_str(&format!("  class {} {}\n", members.join(","), state.name()));
    }
    link_style(&mut out, &added_links, "stroke:#1a7f37,stroke-width:2px");
    link_style(&mut out, &removed_links, "stroke:#cf222e,stroke-width:2px");
    out
}

fn label_of(node: &GraphNode) -> String {
    match node.state.prefix() {
        "" => node.label.clone(),
        prefix => format!("{prefix} {}", node.label),
    }
}

fn link_style(out: &mut String, indexes: &[usize], style: &str) {
    if indexes.is_empty() {
        return;
    }
    let list: Vec<String> = indexes.iter().map(usize::to_string).collect();
    out.push_str(&format!("  linkStyle {} {style}\n", list.join(",")));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::graph::fixtures::{edge, node};
    use super::*;
    use std::collections::BTreeSet;

    /// A parser for the documented subset. It panics on a line outside the subset.
    pub(crate) fn check_subset(text: &str) {
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("flowchart LR"));
        let mut ids = BTreeSet::new();
        let mut class_names = BTreeSet::new();
        let mut edges = 0usize;
        let mut in_subgraph = false;
        let mut uses: Vec<(Vec<String>, String)> = Vec::new();
        let mut link_styles: Vec<Vec<usize>> = Vec::new();
        let mut edge_ends: Vec<(String, String)> = Vec::new();
        for raw in lines {
            let line = raw.trim();
            if let Some(rest) = line.strip_prefix("subgraph ") {
                assert!(!in_subgraph, "nested subgraph: {line}");
                in_subgraph = true;
                let (id, label) = rest.split_once('[').expect("subgraph label");
                assert!(is_id(id), "subgraph id: {id}");
                assert!(ids.insert(id.to_string()), "duplicate id: {id}");
                assert_label(label);
            } else if line == "end" {
                assert!(in_subgraph, "end without subgraph");
                in_subgraph = false;
            } else if let Some(rest) = line.strip_prefix("classDef ") {
                let (name, properties) = rest.split_once(' ').expect("classDef properties");
                assert!(is_id(name));
                assert_properties(properties);
                class_names.insert(name.to_string());
            } else if let Some(rest) = line.strip_prefix("class ") {
                let (list, name) = rest.split_once(' ').expect("class name");
                uses.push((
                    list.split(',').map(str::to_string).collect(),
                    name.to_string(),
                ));
            } else if let Some(rest) = line.strip_prefix("linkStyle ") {
                let (list, properties) = rest.split_once(' ').expect("linkStyle properties");
                assert_properties(properties);
                link_styles.push(list.split(',').map(|n| n.parse().expect("index")).collect());
            } else if let Some((from, to)) = line.split_once(" --> ") {
                edges += 1;
                edge_ends.push((from.to_string(), to.to_string()));
            } else if let Some((from, to)) = line.split_once(" -.-> ") {
                edges += 1;
                edge_ends.push((from.to_string(), to.to_string()));
            } else {
                let (id, label) = line
                    .split_once('[')
                    .unwrap_or_else(|| panic!("line: {line}"));
                assert!(is_id(id), "node id: {id}");
                assert!(ids.insert(id.to_string()), "duplicate id: {id}");
                assert_label(label);
            }
        }
        assert!(!in_subgraph, "subgraph without end");
        for (from, to) in &edge_ends {
            assert!(ids.contains(from) && ids.contains(to), "edge {from} {to}");
        }
        for (list, name) in &uses {
            assert!(class_names.contains(name), "class {name}");
            assert!(list.iter().all(|id| ids.contains(id)), "class ids {list:?}");
        }
        for list in &link_styles {
            assert!(list.iter().all(|index| *index < edges));
        }
    }

    fn is_id(text: &str) -> bool {
        !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    fn assert_label(text: &str) {
        let inner = text
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix("\"]"))
            .unwrap_or_else(|| panic!("label: {text}"));
        assert!(!inner.contains('"') && !inner.contains('<') && !inner.contains('>'));
    }

    fn assert_properties(text: &str) {
        for pair in text.split(',') {
            let (key, value) = pair.split_once(':').expect("property");
            assert!(is_id(&key.replace('-', "_")) && !value.is_empty(), "{pair}");
        }
    }

    fn sample() -> ChangeGraph {
        ChangeGraph {
            nodes: vec![
                node("src/a.rs#run", "src", ChangeState::Changed),
                node("src/a.rs#helper", "src", ChangeState::Added),
                node("src/b.rs#old", "src", ChangeState::Removed),
                node("app/main.rs#main", "app", ChangeState::Unchanged),
            ],
            edges: vec![
                edge("app/main.rs#main", "src/a.rs#run", ChangeState::Unchanged),
                edge("src/a.rs#run", "src/a.rs#helper", ChangeState::Added),
                edge("src/a.rs#run", "src/b.rs#old", ChangeState::Removed),
            ],
        }
    }

    #[test]
    fn test_render_gives_none_for_an_empty_graph() {
        assert_eq!(render(&ChangeGraph::default(), 30), None);
    }

    #[test]
    fn test_render_stays_in_the_documented_subset() {
        check_subset(&render(&sample(), 30).unwrap());
    }

    #[test]
    fn test_render_stays_in_the_subset_when_the_cap_leaves_nodes_out() {
        check_subset(&render(&sample(), 2).unwrap());
        check_subset(&render(&sample(), 1).unwrap());
        assert_eq!(render(&sample(), 0), None);
    }

    #[test]
    fn test_render_prefixes_each_label_with_the_sign_of_its_state() {
        let text = render(&sample(), 30).unwrap();
        assert!(text.contains("[\"~ run\"]"), "{text}");
        assert!(text.contains("[\"+ helper\"]"), "{text}");
        assert!(text.contains("[\"- old\"]"), "{text}");
        assert!(text.contains("[\"main\"]"), "{text}");
    }

    #[test]
    fn test_render_draws_a_removed_edge_dashed() {
        let text = render(&sample(), 30).unwrap();
        assert_eq!(text.matches(" -.-> ").count(), 1, "{text}");
        assert_eq!(text.matches(" --> ").count(), 2, "{text}");
    }

    #[test]
    fn test_render_defines_one_class_for_each_state() {
        let text = render(&sample(), 30).unwrap();
        for name in ["added", "removed", "changed", "unchanged"] {
            assert!(text.contains(&format!("classDef {name} ")), "{name}");
        }
    }

    #[test]
    fn test_render_gives_one_more_node_for_each_component_that_lost_nodes() {
        let text = render(&sample(), 2).unwrap();
        assert!(
            text.contains("[\"+1 more\"]") && text.contains("[\"+3 more\"]"),
            "{text}"
        );
    }

    #[test]
    fn test_render_gives_one_more_node_in_all_when_the_components_do_not_fit() {
        let text = render(&sample(), 1).unwrap();
        assert_eq!(text.matches("more\"]").count(), 1, "{text}");
        assert!(text.contains("[\"+4 more\"]"), "{text}");
    }

    #[test]
    fn test_render_escapes_the_characters_of_a_label() {
        let mut graph = sample();
        graph.nodes[0].label = "a<\"b\">&".to_string();
        let text = render(&graph, 30).unwrap();
        check_subset(&text);
        assert!(text.contains("a#lt;#quot;b#quot;#gt;#amp;"), "{text}");
    }

    #[test]
    fn test_render_escapes_line_breaks_of_a_label() {
        let mut graph = sample();
        graph.nodes[0].label = "a\nb\rc".to_string();
        graph.nodes[0].component = "dir\nname".to_string();
        let text = render(&graph, 30).unwrap();
        check_subset(&text);
        assert!(text.contains("a#10;b#13;c"), "{text}");
        assert!(text.contains("dir#10;name"), "{text}");
    }

    #[test]
    fn test_render_is_the_same_for_two_runs() {
        assert_eq!(render(&sample(), 3), render(&sample(), 3));
    }

    #[test]
    fn test_render_does_not_depend_on_the_order_of_the_input() {
        let mut shuffled = sample();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();
        assert_eq!(render(&sample(), 30), render(&shuffled, 30));
    }
}
