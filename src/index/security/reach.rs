//! Reachability: the paths from the entry points to the symbols that hold a sink.
//!
//! The search is a complete breadth-first search over the call graph of the index, from
//! each entry point. Each edge has the resolution basis of its call. The result gives the
//! shortest path, the number of shortest paths, and the weakest basis of the path.
//!
//! The search also follows a function reference: a use of a function as a value, as
//! `spawn(worker)` uses `worker`. Such an edge is not a call. It says that the symbol can
//! cause the function to run. A step that comes from such an edge has the edge kind
//! `function_reference`.
//!
//! A sink without a path has the state `no_known_entry`. That state does not say that the
//! sink is safe. The graph has only the calls and the function references that the index
//! resolves, and an entry point that the table does not know has no paths.
//!
//! An inline handler, as an arrow function that a route registers, is a node of the graph. A
//! call, a function reference, or a sink belongs to the innermost inline handler that contains
//! it. The inline handler or the function that contains a handler has a function reference to
//! it, because it gives the handler as a value.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use petgraph::algo::dominators::simple_fast;
use petgraph::graph::{DiGraph, NodeIndex};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::entry::EntryPoint;
use super::{Sink, SinkCategory};
use crate::core::Language;
use crate::index::model::{ResolutionBasis, Span};
use crate::index::view::CallView;

/// How one symbol on a path leads to the next symbol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The symbol calls the next symbol.
    #[default]
    Call,
    /// The symbol uses the next symbol as a value and does not call it, as
    /// `spawn(worker)` uses `worker`. The symbol can cause the next symbol to run.
    FunctionReference,
}

impl EdgeKind {
    fn is_call(&self) -> bool {
        *self == Self::Call
    }
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// One symbol on a path, with the basis of the edge that leads to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// The key of the symbol.
    pub symbol: String,
    /// The basis of the resolution of the name in the previous step. The first step has none.
    pub basis: Option<ResolutionBasis>,
    /// The kind of the edge from the previous step. The output has this field
    /// only for an edge that is not a call.
    #[serde(default, skip_serializing_if = "EdgeKind::is_call")]
    pub edge: EdgeKind,
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
    /// The number of edges of `steps` that are function references. Such a
    /// path says that the entry point can cause the sink to run. It does not
    /// say that a chain of calls goes to the sink. The output has this field
    /// only when the number is not zero.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub function_references: usize,
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
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => &[
            "express",
            "fastify",
            "koa",
            "nestjs",
            "next",
            "repository_decorators",
            "routing-controllers",
            "tsoa",
        ],
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
        "Omen does not follow data, and it follows only the calls and the function references \
         that the index resolves. \
         An entry point with `guards_complete: false` can have a guard that omen did not read, \
         or can lack a guard that omen lists. \
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
type Reached = (usize, u64, Option<(usize, ResolutionBasis, EdgeKind)>);

/// The result of one search from one entry point.
struct Search {
    reached: BTreeMap<usize, Reached>,
}

/// What the searches of some entry points give.
#[derive(Default)]
struct Found {
    paths: Vec<SinkPath>,
    /// For each symbol, the number of entry points that reach it. The list
    /// is empty before the first search.
    entries: Vec<u32>,
}

impl Found {
    /// The results of `self` and then the results of `later`.
    fn merge(mut self, later: Self) -> Self {
        self.paths.extend(later.paths);
        if self.entries.is_empty() {
            self.entries = later.entries;
        } else {
            for (count, more) in self.entries.iter_mut().zip(later.entries) {
                *count += more;
            }
        }
        self
    }
}

/// The edges from one symbol: its calls and its function references. An edge
/// counts when the basis of its resolution is `min_basis` or a stronger basis.
/// A target can have several edges. A call is the edge before a function
/// reference, and then the strongest basis is the basis of the edge.
fn edges_of(
    view: &CallView,
    symbol: usize,
    min_basis: ResolutionBasis,
) -> BTreeMap<usize, (ResolutionBasis, EdgeKind)> {
    let mut edges: BTreeMap<usize, (ResolutionBasis, EdgeKind)> = BTreeMap::new();
    let kinds = [
        (EdgeKind::Call, view.callee_edges(symbol)),
        (
            EdgeKind::FunctionReference,
            view.function_reference_edges(symbol),
        ),
    ];
    for (kind, targets) in kinds {
        for (target, basis) in targets {
            if basis.rank() < min_basis.rank() {
                continue;
            }
            let best = edges.entry(*target).or_insert((*basis, kind));
            if best.1 == kind && basis.rank() > best.0.rank() {
                best.0 = *basis;
            }
        }
    }
    edges
}

fn search(view: &CallView, start: usize, min_basis: ResolutionBasis) -> Search {
    let mut reached: BTreeMap<usize, Reached> = BTreeMap::new();
    reached.insert(start, (0, 1, None));
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        let (distance, count, _) = reached[&current];
        for (target, (basis, kind)) in edges_of(view, current, min_basis) {
            match reached.get_mut(&target) {
                None => {
                    reached.insert(target, (distance + 1, count, Some((current, basis, kind))));
                    queue.push_back(target);
                }
                Some(entry) if entry.0 == distance + 1 => {
                    entry.1 = entry.1.saturating_add(count);
                    // The path that the result shows uses a call when a call
                    // and a function reference go to the symbol in the same
                    // number of steps.
                    let by_reference = entry
                        .2
                        .is_some_and(|previous| previous.2 == EdgeKind::FunctionReference);
                    if by_reference && kind == EdgeKind::Call {
                        entry.2 = Some((current, basis, kind));
                    }
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
    let mut callers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for symbol in result.reached.keys() {
        for callee in edges_of(view, *symbol, min_basis).into_keys() {
            if result.reached.contains_key(&callee) {
                callers.entry(callee).or_default().push(*symbol);
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

/// One inline handler: its range, the index of the region that contains it, and its value.
struct Region {
    start: usize,
    end: usize,
    parent: Option<usize>,
    value: usize,
}

/// The regions of one file and one symbol around them, in the order of their start. Two regions
/// of one syntax tree are nested or apart.
#[derive(Default)]
struct Place {
    regions: Vec<Region>,
    /// From each boundary of a region to the next one, the value of the innermost region, in the
    /// order of the boundaries. A search of a byte is then one binary search, also below a deep
    /// nest of regions.
    segments: Vec<(usize, Option<usize>)>,
}

/// The inline handlers of the entry points, by the file and the symbol around them, each with a
/// value that the caller gives.
pub(super) struct InlineRegions<'e> {
    by_place: BTreeMap<(&'e str, Option<&'e str>), Place>,
}

impl<'e> InlineRegions<'e> {
    /// The regions of the entries for which `value_of` gives a value. Of two entries with one
    /// range, the first one gives the value.
    pub(super) fn new(
        entries: &'e [EntryPoint],
        value_of: impl Fn(usize, &EntryPoint) -> Option<usize>,
    ) -> Self {
        let mut by_place: BTreeMap<(&'e str, Option<&'e str>), Place> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let (Some(inline), Some(value)) =
                (entry.inline_handler.as_ref(), value_of(index, entry))
            else {
                continue;
            };
            by_place
                .entry((entry.file.as_str(), inline.enclosing.as_deref()))
                .or_default()
                .regions
                .push(Region {
                    start: inline.range.start_byte,
                    end: inline.range.end_byte,
                    parent: None,
                    value,
                });
        }
        for place in by_place.values_mut() {
            let Place { regions, segments } = place;
            regions.sort_by_key(|region| (region.start, std::cmp::Reverse(region.end)));
            regions.dedup_by_key(|region| (region.start, region.end));
            let mut open: Vec<usize> = Vec::new();
            for index in 0..regions.len() {
                while let Some(last) = open
                    .last()
                    .copied()
                    .filter(|last| regions[*last].end <= regions[index].start)
                {
                    open.pop();
                    let around = open.last().map(|outer| regions[*outer].value);
                    segments.push((regions[last].end, around));
                }
                regions[index].parent = open.last().copied();
                open.push(index);
                segments.push((regions[index].start, Some(regions[index].value)));
            }
            while let Some(last) = open.pop() {
                let around = open.last().map(|outer| regions[*outer].value);
                segments.push((regions[last].end, around));
            }
        }
        Self { by_place }
    }

    /// The value of the innermost inline handler of the place that contains the byte.
    pub(super) fn innermost(
        &self,
        file: &str,
        enclosing: Option<&str>,
        byte: usize,
    ) -> Option<usize> {
        let segments = &self.by_place.get(&(file, enclosing))?.segments;
        let after = segments.partition_point(|(boundary, _)| *boundary <= byte);
        segments[after.checked_sub(1)?].1
    }

    /// The value of the inline handler that directly contains the inline handler with the range.
    pub(super) fn parent(
        &self,
        file: &str,
        enclosing: Option<&str>,
        range: &Span,
    ) -> Option<usize> {
        let regions = &self.by_place.get(&(file, enclosing))?.regions;
        let key = (range.start_byte, std::cmp::Reverse(range.end_byte));
        let index = regions
            .binary_search_by_key(&key, |region| (region.start, std::cmp::Reverse(region.end)))
            .ok()?;
        regions[index].parent.map(|parent| regions[parent].value)
    }

    /// The innermost inline handler that holds the sink.
    fn holder(&self, sink: &Sink) -> Option<usize> {
        self.innermost(&sink.file, sink.enclosing.as_deref(), sink.range.start_byte)
    }
}

/// Finds the paths from the entry points to the sinks.
///
/// An entry point or a sink in test code is not in the result. A sink in generated code is
/// not in the result either. An entry point without a symbol in the index has no paths.
pub fn reachability(
    view: &CallView,
    sinks: &[Sink],
    entries: &[EntryPoint],
    min_basis: ResolutionBasis,
) -> Reachability {
    let regions = InlineRegions::new(entries, |_, entry| {
        entry
            .symbol
            .as_deref()
            .and_then(|key| view.position_of(key))
    });
    let sink_symbols: Vec<(&Sink, Option<usize>)> = sinks
        .iter()
        .filter(|sink| !sink.flags.in_test && !sink.flags.in_generated)
        .map(|sink| {
            let symbol = regions.holder(sink).or_else(|| {
                sink.enclosing
                    .as_deref()
                    .and_then(|key| view.position_of(key))
            });
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
    // Each entry point has one search. A search can reach a large part of the
    // repository, thus the result of a search does not stay in memory: only
    // its paths stay, and a count for each symbol that it reaches. The threads
    // give their results in the order of the entry points.
    let paths_of = |found: &mut Found, entry: &EntryPoint, start: usize| {
        let result = search(view, start, min_basis);
        if found.entries.is_empty() {
            found.entries = vec![0; view.symbols.len()];
        }
        for symbol in result.reached.keys() {
            found.entries[*symbol] += 1;
        }
        for (sink, symbol) in &sink_symbols {
            let Some(symbol) = symbol else {
                continue;
            };
            let Some((_, count, _)) = result.reached.get(symbol) else {
                continue;
            };
            let mut chain = vec![*symbol];
            let mut edges = Vec::new();
            let mut current = *symbol;
            while let Some((_, _, Some((previous, basis, kind)))) = result.reached.get(&current) {
                edges.push((*basis, *kind));
                chain.push(*previous);
                current = *previous;
            }
            debug_assert_eq!(current, start);
            chain.reverse();
            edges.reverse();
            let steps = chain
                .iter()
                .enumerate()
                .map(|(position, symbol)| {
                    let edge = position.checked_sub(1).map(|edge| edges[edge]);
                    Step {
                        symbol: view.symbols[*symbol].qualified_name.clone(),
                        basis: edge.map(|(basis, _)| basis),
                        edge: edge.map_or(EdgeKind::Call, |(_, kind)| kind),
                    }
                })
                .collect();
            let members = shortest_path_members(view, &result, *symbol, min_basis);
            found.paths.push(SinkPath {
                entry: entry.key.clone(),
                sink: sink.key.clone(),
                steps,
                weakest_basis: edges
                    .iter()
                    .map(|(basis, _)| *basis)
                    .min_by_key(|basis| basis.rank()),
                path_count: *count,
                members,
                function_references: edges
                    .iter()
                    .filter(|(_, kind)| *kind == EdgeKind::FunctionReference)
                    .count(),
            });
        }
    };
    let found = starts
        .par_iter()
        .fold(Found::default, |mut found, (entry, start)| {
            paths_of(&mut found, entry, *start);
            found
        })
        .reduce(Found::default, Found::merge);
    let mut paths = found.paths;
    paths.sort_by(|left, right| (&left.sink, &left.entry).cmp(&(&right.sink, &right.entry)));

    let reached_sinks: BTreeSet<&str> = paths.iter().map(|path| path.sink.as_str()).collect();
    let no_known_entry: Vec<String> = sink_symbols
        .iter()
        .filter(|(sink, _)| !reached_sinks.contains(sink.key.as_str()))
        .map(|(sink, _)| sink.key.clone())
        .collect();
    let choke_points = choke_points(view, &sink_symbols, &starts, &found.entries, min_basis);
    Reachability {
        paths,
        no_known_entry,
        choke_points,
    }
}

/// The symbols that dominate a sink from a virtual root that has an edge to each entry point.
/// `entries` has, for each symbol, the number of entry points that reach the symbol.
fn choke_points(
    view: &CallView,
    sinks: &[(&Sink, Option<usize>)],
    starts: &[(&EntryPoint, usize)],
    entries: &[u32],
    min_basis: ResolutionBasis,
) -> Vec<ChokePoint> {
    let reached: BTreeSet<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(symbol, _)| symbol)
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
    for symbol in &reached {
        let targets = edges_of(view, *symbol, min_basis)
            .into_keys()
            .filter(|target| reached.contains(target));
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
            entries: entries[symbol] as usize,
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
