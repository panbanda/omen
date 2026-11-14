//! Reachability: the paths from the entry points to the symbols that hold a sink.
//!
//! The search is a complete breadth-first search over the call graph of the index, from
//! each entry point. Each edge has the resolution basis of its call. The result gives the
//! shortest path, the number of shortest paths, and the weakest basis of the path.
//!
//! A sink without a path has the state `no_known_entry`. That state does not say that the
//! sink is safe. The call graph has only the calls that the index resolves, and an entry
//! point that the table does not know has no paths.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use petgraph::algo::dominators::simple_fast;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

use super::entry::EntryPoint;
use super::{Sink, SinkCategory};
use crate::core::Language;
use crate::index::model::ResolutionBasis;
use crate::index::view::CallView;

/// One symbol on a path, with the basis of the call that leads to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// The key of the symbol.
    pub symbol: String,
    /// The basis of the call from the previous step. The first step has none.
    pub basis: Option<ResolutionBasis>,
}

/// The shortest path from one entry point to one sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkPath {
    pub entry: String,
    pub sink: String,
    /// From the symbol of the entry point to the symbol that holds the sink.
    pub steps: Vec<Step>,
    /// The weakest basis of the calls on the path. A path of one step has none.
    pub weakest_basis: Option<ResolutionBasis>,
    /// The number of shortest paths from the entry point to the symbol of the sink.
    pub path_count: u64,
    /// The keys of the symbols on any of the shortest paths, not only on `steps`.
    pub members: Vec<String>,
}

/// A symbol that each known path to some sinks goes through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChokePoint {
    pub symbol: String,
    /// The number of entry points that reach the symbol.
    pub entries: usize,
    /// The number of sinks for which the symbol is on each known path.
    pub sinks: usize,
    /// The keys of those sinks, in order.
    pub sink_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reachability {
    pub paths: Vec<SinkPath>,
    /// The keys of the sinks that no entry point reaches. It is not "safe".
    pub no_known_entry: Vec<String>,
    pub choke_points: Vec<ChokePoint>,
}

/// What omen recognises for the languages of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub sink_categories: Vec<String>,
    /// The experimental categories that this analysis did not look for.
    pub experimental_off: Vec<String>,
    pub statement: String,
}

fn language_name(language: Language) -> String {
    serde_json::to_value(language)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The frameworks that the entry point table recognises for a language.
fn frameworks_of(language: Language) -> &'static [&'static str] {
    match language {
        Language::Python => &["flask", "fastapi", "django", "celery", "python"],
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => {
            &["express", "next"]
        }
        Language::Java => &["spring", "java"],
        Language::CSharp => &["aspnet", "csharp"],
        Language::Go => &["net/http", "gin", "go"],
        Language::Rust => &["axum", "actix", "rocket", "ffi", "rust"],
        Language::Php => &["laravel"],
        Language::Ruby => &["rails"],
        Language::C => &["c"],
        Language::Cpp => &["cpp"],
        Language::Bash => &["bash"],
    }
}

/// The frameworks and the sink categories that omen recognises for `languages`.
pub fn coverage(languages: &[Language], experimental: bool) -> Coverage {
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut frameworks: BTreeSet<String> = BTreeSet::new();
    let mut categories: BTreeSet<&'static str> = BTreeSet::new();
    let mut experimental_off: BTreeSet<&'static str> = BTreeSet::new();
    for language in languages {
        names.insert(language_name(*language));
        frameworks.extend(frameworks_of(*language).iter().map(|name| name.to_string()));
        for spec in super::spec::specs_for(*language) {
            if spec.category.is_experimental() && !experimental {
                experimental_off.insert(spec.category.name());
            } else {
                categories.insert(spec.category.name());
            }
        }
    }
    let mut statement = format!(
        "Omen recognises these frameworks: {}. Omen recognises these sink categories: {}. ",
        frameworks.iter().cloned().collect::<Vec<_>>().join(", "),
        categories.iter().copied().collect::<Vec<_>>().join(", "),
    );
    if !experimental_off.is_empty() {
        statement.push_str(&format!(
            "These sink categories are experimental, and this analysis did not look for them: {}. \
             Use the option `experimental` to look for them. ",
            experimental_off
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    statement.push_str(
        "Omen does not follow data, and it follows only the calls that the index resolves. \
         An empty result does not mean that the code is safe.",
    );
    Coverage {
        languages: names.into_iter().collect(),
        frameworks: frameworks.into_iter().collect(),
        sink_categories: categories.into_iter().map(str::to_string).collect(),
        experimental_off: experimental_off.into_iter().map(str::to_string).collect(),
        statement,
    }
}

/// The distance, the number of shortest paths, and the previous step of a reached symbol.
type Reached = (usize, u64, Option<(usize, ResolutionBasis)>);

/// The result of one search from one entry point.
struct Search {
    reached: BTreeMap<usize, Reached>,
}

fn search(view: &CallView, start: usize, min_basis: ResolutionBasis) -> Search {
    let mut reached: BTreeMap<usize, Reached> = BTreeMap::new();
    reached.insert(start, (0, 1, None));
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        let (distance, count, _) = reached[&current];
        // A target can have several calls. The strongest basis is the basis of the edge.
        let mut edges: BTreeMap<usize, ResolutionBasis> = BTreeMap::new();
        for (target, basis) in view.callee_edges(current) {
            if basis.rank() >= min_basis.rank() {
                let best = edges.entry(*target).or_insert(*basis);
                if basis.rank() > best.rank() {
                    *best = *basis;
                }
            }
        }
        for (target, basis) in edges {
            match reached.get_mut(&target) {
                None => {
                    reached.insert(target, (distance + 1, count, Some((current, basis))));
                    queue.push_back(target);
                }
                Some(entry) if entry.0 == distance + 1 => {
                    entry.1 = entry.1.saturating_add(count);
                }
                Some(_) => {}
            }
        }
    }
    Search { reached }
}

/// The keys of the symbols on any shortest path from the start of `result` to `target`. A
/// symbol is on one when its distance from the start and its distance to the target add up
/// to the distance of the target.
fn shortest_path_members(
    view: &CallView,
    result: &Search,
    target: usize,
    min_basis: ResolutionBasis,
) -> Vec<String> {
    let Some((total, _, _)) = result.reached.get(&target) else {
        return Vec::new();
    };
    let minimum = min_basis.rank();
    let mut callers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for symbol in result.reached.keys() {
        for (callee, basis) in view.callee_edges(*symbol) {
            if basis.rank() >= minimum && result.reached.contains_key(callee) {
                callers.entry(*callee).or_default().push(*symbol);
            }
        }
    }
    let mut to_target: BTreeMap<usize, usize> = BTreeMap::from([(target, 0)]);
    let mut queue = VecDeque::from([target]);
    while let Some(current) = queue.pop_front() {
        let distance = to_target[&current];
        for caller in callers.get(&current).into_iter().flatten() {
            if !to_target.contains_key(caller) {
                to_target.insert(*caller, distance + 1);
                queue.push_back(*caller);
            }
        }
    }
    let mut members: Vec<String> = result
        .reached
        .iter()
        .filter(|(symbol, (from_start, _, _))| {
            to_target
                .get(symbol)
                .is_some_and(|to| from_start + to == *total)
        })
        .map(|(symbol, _)| view.symbols[*symbol].qualified_name.clone())
        .collect();
    members.sort();
    members
}

/// Finds the paths from the entry points to the sinks.
///
/// An entry point or a sink in test code is not in the result. An entry point without a
/// symbol in the index has no paths.
pub fn reachability(
    view: &CallView,
    sinks: &[Sink],
    entries: &[EntryPoint],
    min_basis: ResolutionBasis,
) -> Reachability {
    let sink_symbols: Vec<(&Sink, Option<usize>)> = sinks
        .iter()
        .filter(|sink| !sink.flags.in_test)
        .map(|sink| {
            let symbol = sink
                .enclosing
                .as_deref()
                .and_then(|key| view.position_of(key));
            (sink, symbol)
        })
        .collect();
    let starts: Vec<(&EntryPoint, usize)> = entries
        .iter()
        .filter(|entry| !entry.in_test)
        .filter_map(|entry| {
            let symbol = entry
                .symbol
                .as_deref()
                .and_then(|key| view.position_of(key))?;
            Some((entry, symbol))
        })
        .collect();
    let searches: Vec<Search> = starts
        .iter()
        .map(|(_, start)| search(view, *start, min_basis))
        .collect();

    let mut paths = Vec::new();
    let mut reached_sinks: BTreeSet<&str> = BTreeSet::new();
    for ((entry, start), result) in starts.iter().zip(&searches) {
        for (sink, symbol) in &sink_symbols {
            let Some(symbol) = symbol else {
                continue;
            };
            let Some((_, count, _)) = result.reached.get(symbol) else {
                continue;
            };
            let mut chain = vec![*symbol];
            let mut bases = Vec::new();
            let mut current = *symbol;
            while let Some((_, _, Some((previous, basis)))) = result.reached.get(&current) {
                bases.push(*basis);
                chain.push(*previous);
                current = *previous;
            }
            debug_assert_eq!(current, *start);
            chain.reverse();
            bases.reverse();
            let steps = chain
                .iter()
                .enumerate()
                .map(|(position, symbol)| Step {
                    symbol: view.symbols[*symbol].qualified_name.clone(),
                    basis: position.checked_sub(1).map(|edge| bases[edge]),
                })
                .collect();
            reached_sinks.insert(sink.key.as_str());
            let members = shortest_path_members(view, result, *symbol, min_basis);
            paths.push(SinkPath {
                entry: entry.key.clone(),
                sink: sink.key.clone(),
                steps,
                weakest_basis: bases.iter().copied().min_by_key(|basis| basis.rank()),
                path_count: *count,
                members,
            });
        }
    }
    paths.sort_by(|left, right| (&left.sink, &left.entry).cmp(&(&right.sink, &right.entry)));

    let no_known_entry: Vec<String> = sink_symbols
        .iter()
        .filter(|(sink, _)| !reached_sinks.contains(sink.key.as_str()))
        .map(|(sink, _)| sink.key.clone())
        .collect();
    let choke_points = choke_points(view, &sink_symbols, &starts, &searches, min_basis);
    Reachability {
        paths,
        no_known_entry,
        choke_points,
    }
}

/// The symbols that dominate a sink from a virtual root that has an edge to each entry point.
fn choke_points(
    view: &CallView,
    sinks: &[(&Sink, Option<usize>)],
    starts: &[(&EntryPoint, usize)],
    searches: &[Search],
    min_basis: ResolutionBasis,
) -> Vec<ChokePoint> {
    let reached: BTreeSet<usize> = searches
        .iter()
        .flat_map(|search| search.reached.keys().copied())
        .collect();
    if reached.is_empty() {
        return Vec::new();
    }
    let mut graph: DiGraph<Option<usize>, ()> = DiGraph::new();
    let root = graph.add_node(None);
    let nodes: BTreeMap<usize, NodeIndex> = reached
        .iter()
        .map(|symbol| (*symbol, graph.add_node(Some(*symbol))))
        .collect();
    let first_starts: BTreeSet<usize> = starts.iter().map(|(_, start)| *start).collect();
    for start in &first_starts {
        graph.add_edge(root, nodes[start], ());
    }
    let minimum = min_basis.rank();
    for symbol in &reached {
        let targets: BTreeSet<usize> = view
            .callee_edges(*symbol)
            .iter()
            .filter(|(_, basis)| basis.rank() >= minimum)
            .map(|(target, _)| *target)
            .filter(|target| reached.contains(target))
            .collect();
        for target in targets {
            graph.add_edge(nodes[symbol], nodes[&target], ());
        }
    }
    let dominators = simple_fast(&graph, root);
    let mut by_symbol: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for (sink, symbol) in sinks {
        let Some(node) = symbol.and_then(|symbol| nodes.get(&symbol).copied()) else {
            continue;
        };
        let Some(chain) = dominators.dominators(node) else {
            continue;
        };
        for dominator in chain {
            if let Some(Some(symbol_position)) = graph.node_weight(dominator) {
                if Some(*symbol_position) != *symbol {
                    by_symbol
                        .entry(*symbol_position)
                        .or_default()
                        .insert(sink.key.as_str());
                }
            }
        }
    }
    let mut points: Vec<ChokePoint> = by_symbol
        .into_iter()
        .map(|(symbol, sinks)| ChokePoint {
            symbol: view.symbols[symbol].qualified_name.clone(),
            entries: searches
                .iter()
                .filter(|search| search.reached.contains_key(&symbol))
                .count(),
            sinks: sinks.len(),
            sink_keys: sinks.iter().map(|key| key.to_string()).collect(),
        })
        .collect();
    points.sort_by(|left, right| {
        right
            .sinks
            .cmp(&left.sinks)
            .then(left.symbol.cmp(&right.symbol))
    });
    points
}

/// The categories of a list of sinks, with the number of sinks of each.
pub fn count_by_category(sinks: &[Sink]) -> BTreeMap<SinkCategory, usize> {
    let mut counts = BTreeMap::new();
    for sink in sinks {
        *counts.entry(sink.category).or_insert(0) += 1;
    }
    counts
}
