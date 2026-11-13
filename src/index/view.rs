//! A view of the shared index for the commands that follow calls: `symbol`,
//! `impact`, and the MCP tools `get_symbol` and `impact`.
//!
//! An edge of the view exists only for a call with the status `Resolved` or
//! `UniqueCandidate`. A call with a different status is evidence, not an edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::build::CodeIndex;
use super::model::{Receiver, ResolutionBasis, ResolutionStatus, SymbolKind};
use crate::core::{Error, Language, Result};

/// The weakest basis that gives an edge when the caller does not select
/// one. A candidate that has only the name is not an edge.
pub const DEFAULT_MIN_BASIS: ResolutionBasis = ResolutionBasis::SamePackage;

/// One definition of the view. `qualified_name` is the key of the symbol:
/// `rel/path#Container.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub signature: String,
}

/// The place of a definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolLocation {
    pub qualified_name: String,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
}

impl From<&ViewSymbol> for SymbolLocation {
    fn from(symbol: &ViewSymbol) -> Self {
        Self {
            qualified_name: symbol.qualified_name.clone(),
            file: symbol.file.clone(),
            line: symbol.line,
            end_line: symbol.end_line,
        }
    }
}

/// One call site of a definition, with the result of its resolution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallEvidence {
    pub caller: SymbolLocation,
    /// The callee as the source has it, with its receiver or its path.
    pub call: String,
    pub line: u32,
    pub status: ResolutionStatus,
    /// `None` when no rule applied.
    pub basis: Option<ResolutionBasis>,
    pub candidates: Vec<SymbolLocation>,
}

/// The number of call sites of each status.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolutionSummary {
    pub unique_candidate: usize,
    pub ambiguous: usize,
    pub unresolved: usize,
    #[serde(default)]
    pub external: usize,
}

pub struct CallView {
    /// The definitions, in the order of the file and the position. A field
    /// is not in the view.
    pub symbols: Vec<ViewSymbol>,
    pub by_name: HashMap<String, Vec<usize>>,
    by_key: HashMap<String, usize>,
    /// For each definition, the definitions that it calls, with the basis
    /// of the call. A call whose one candidate has only the name is here
    /// with the basis `NameOnly`. It is an edge only on request.
    callees: Vec<Vec<(usize, ResolutionBasis)>>,
    callers: Vec<Vec<(usize, ResolutionBasis)>>,
    pub call_resolutions: Vec<CallEvidence>,
    call_resolution_ranges: Vec<Range<usize>>,
}

impl CallView {
    pub fn from_index(index: &CodeIndex) -> Self {
        let mut symbols = Vec::new();
        let mut by_key = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for file in &index.files {
            for symbol in &file.symbols {
                if symbol.kind == SymbolKind::Field {
                    continue;
                }
                let position = symbols.len();
                by_key.insert(symbol.key.clone(), position);
                by_name
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(position);
                symbols.push(ViewSymbol {
                    name: symbol.name.clone(),
                    qualified_name: symbol.key.clone(),
                    kind: symbol.kind,
                    file: file.file.clone(),
                    line: symbol.range.start_line,
                    end_line: symbol.range.end_line,
                    signature: symbol.signature.clone(),
                });
            }
        }
        let mut callees: Vec<BTreeSet<(usize, ResolutionBasis)>> =
            vec![BTreeSet::new(); symbols.len()];
        let mut callers: Vec<BTreeSet<(usize, ResolutionBasis)>> =
            vec![BTreeSet::new(); symbols.len()];
        let mut evidence: Vec<Vec<CallEvidence>> = vec![Vec::new(); symbols.len()];
        for reference in index.files.iter().flat_map(|file| &file.references) {
            let caller = reference
                .enclosing
                .as_deref()
                .and_then(|key| by_key.get(key).copied());
            let Some(caller) = caller else {
                continue;
            };
            let resolution = &reference.resolution;
            let candidates: Vec<usize> = resolution
                .candidates
                .iter()
                .filter_map(|key| by_key.get(key).copied())
                .collect();
            let is_edge = matches!(
                resolution.status,
                ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
            );
            // One candidate that has only the name is an edge when the
            // caller of the view asks for the basis `NameOnly`.
            let by_name_only = resolution.status == ResolutionStatus::Ambiguous
                && resolution.basis == Some(ResolutionBasis::NameOnly)
                && candidates.len() == 1;
            let basis = match (is_edge, by_name_only) {
                (true, _) => resolution.basis,
                (false, true) => Some(ResolutionBasis::NameOnly),
                (false, false) => None,
            };
            if let Some(basis) = basis {
                for candidate in &candidates {
                    callees[caller].insert((*candidate, basis));
                    callers[*candidate].insert((caller, basis));
                }
            }
            let call = match (&reference.receiver, &reference.qualifier) {
                (Some(Receiver::SelfValue), _) => format!("self.{}", reference.name),
                (Some(Receiver::Super), _) => format!("super.{}", reference.name),
                (Some(Receiver::Name(name)), _) => format!("{name}.{}", reference.name),
                (Some(Receiver::Expression), _) => format!("(..).{}", reference.name),
                (None, Some(qualifier)) => format!("{qualifier}::{}", reference.name),
                (None, None) => reference.name.clone(),
            };
            evidence[caller].push(CallEvidence {
                caller: SymbolLocation::from(&symbols[caller]),
                call,
                line: reference.name_range.start_line,
                status: resolution.status,
                basis: resolution.basis,
                candidates: candidates
                    .iter()
                    .map(|candidate| SymbolLocation::from(&symbols[*candidate]))
                    .collect(),
            });
        }
        let mut call_resolutions = Vec::new();
        let mut call_resolution_ranges = Vec::with_capacity(symbols.len());
        for calls in evidence {
            let start = call_resolutions.len();
            call_resolutions.extend(calls);
            call_resolution_ranges.push(start..call_resolutions.len());
        }
        Self {
            symbols,
            by_name,
            by_key,
            callees: callees
                .into_iter()
                .map(|set| set.into_iter().collect())
                .collect(),
            callers: callers
                .into_iter()
                .map(|set| set.into_iter().collect())
                .collect(),
            call_resolutions,
            call_resolution_ranges,
        }
    }

    /// The definitions that a query names, in the order of the file and the
    /// position. The query is a key (`rel/path#Container.name`), a file and a
    /// name (`rel/path:name`), a name with its containers (`Container.name`),
    /// or a name.
    pub fn resolve(&self, query: &str) -> Vec<usize> {
        if let Some(position) = self.by_key.get(query) {
            return vec![*position];
        }
        let (file, name) = match query.rsplit_once(['#', ':']) {
            Some((file, name)) if !file.is_empty() => (Some(file), name),
            _ => (None, query),
        };
        let bare = name.rsplit('.').next().unwrap_or(name);
        self.by_name
            .get(bare)
            .into_iter()
            .flatten()
            .copied()
            .filter(|position| {
                let symbol = &self.symbols[*position];
                let path = symbol
                    .qualified_name
                    .split_once('#')
                    .map_or("", |(_, path)| path);
                // An ordinal after `~` is a part of the key, not of the name.
                let path = path.split('~').next().unwrap_or(path);
                file.is_none_or(|file| symbol.file == file)
                    && (path == name || path.ends_with(&format!(".{name}")))
            })
            .collect()
    }

    /// The call sites in the body of one definition, in source order.
    pub fn calls_of(&self, symbol: usize) -> &[CallEvidence] {
        &self.call_resolutions[self.call_resolution_ranges[symbol].clone()]
    }

    /// The number of call sites of each status, for all definitions.
    pub fn resolution_summary(&self) -> ResolutionSummary {
        let mut summary = ResolutionSummary::default();
        for call in &self.call_resolutions {
            match call.status {
                ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate => {
                    summary.unique_candidate += 1;
                }
                ResolutionStatus::Ambiguous => summary.ambiguous += 1,
                ResolutionStatus::Unresolved => summary.unresolved += 1,
                ResolutionStatus::External => summary.external += 1,
            }
        }
        summary
    }

    /// The callers of `roots`, one list for each level, up to `depth` levels.
    /// Each list is in the order of the definitions. An edge counts when its
    /// basis is `min_basis` or a stronger basis.
    pub fn callers(
        &self,
        roots: &[usize],
        depth: usize,
        min_basis: ResolutionBasis,
    ) -> Vec<Vec<usize>> {
        self.levels(roots, depth, &self.callers, min_basis)
    }

    /// The callees of `roots`, one list for each level, up to `depth` levels.
    pub fn callees(
        &self,
        roots: &[usize],
        depth: usize,
        min_basis: ResolutionBasis,
    ) -> Vec<Vec<usize>> {
        self.levels(roots, depth, &self.callees, min_basis)
    }

    fn levels(
        &self,
        roots: &[usize],
        depth: usize,
        edges: &[Vec<(usize, ResolutionBasis)>],
        min_basis: ResolutionBasis,
    ) -> Vec<Vec<usize>> {
        let mut seen: BTreeSet<usize> = roots.iter().copied().collect();
        let mut frontier: Vec<usize> = roots.to_vec();
        let mut levels = Vec::new();
        for _ in 0..depth {
            let mut next = BTreeSet::new();
            for symbol in &frontier {
                for (other, basis) in &edges[*symbol] {
                    if basis.rank() >= min_basis.rank() && seen.insert(*other) {
                        next.insert(*other);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next.iter().copied().collect();
            levels.push(frontier.clone());
        }
        levels
    }
}

/// The view of the last set of files of a session. A second request for
/// files with the same content gets the same view.
#[derive(Default)]
pub struct ViewCache {
    root: PathBuf,
    hashes: BTreeMap<PathBuf, blake3::Hash>,
    last: Option<Arc<CallView>>,
}

impl ViewCache {
    pub fn load(&mut self, root: &Path, files: &[PathBuf]) -> Result<Arc<CallView>> {
        let root = root.canonicalize()?;
        let mut hashes = BTreeMap::new();
        for path in files {
            let path = path.canonicalize()?;
            if !path.starts_with(&root) {
                return Err(Error::analysis("Index file escapes repository"));
            }
            if Language::detect(&path).is_none() {
                continue;
            }
            hashes.insert(path.clone(), blake3::hash(&std::fs::read(&path)?));
        }
        if let Some(last) = self
            .last
            .as_ref()
            .filter(|_| self.root == root && self.hashes == hashes)
        {
            return Ok(Arc::clone(last));
        }
        let paths: Vec<PathBuf> = hashes.keys().cloned().collect();
        let view = Arc::new(CallView::from_index(&CodeIndex::from_files(&root, &paths)));
        self.root = root;
        self.hashes = hashes;
        self.last = Some(Arc::clone(&view));
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::imports::testing::index;
    use crate::index::resolve::resolve_references;

    fn view(files: &[(&str, &str)]) -> CallView {
        let mut facts = index(files);
        resolve_references(&mut facts);
        CallView::from_index(&CodeIndex { files: facts })
    }

    const FILES: &[(&str, &str)] = &[(
        "shop.py",
        "class Item:\n    def name(self):\n        return helper()\n\n\ndef helper():\n    return leaf()\n\n\ndef leaf():\n    return len(\"a\")\n\n\ndef run(item: Item):\n    return item.name()\n",
    )];

    #[test]
    fn test_query_forms_name_one_definition() {
        let view = view(FILES);
        let names = |query: &str| -> Vec<&str> {
            view.resolve(query)
                .into_iter()
                .map(|position| view.symbols[position].qualified_name.as_str())
                .collect()
        };
        assert_eq!(names("shop.py#Item.name"), ["shop.py#Item.name"]);
        assert_eq!(names("shop.py:name"), ["shop.py#Item.name"]);
        assert_eq!(names("Item.name"), ["shop.py#Item.name"]);
        assert_eq!(names("name"), ["shop.py#Item.name"]);
        assert_eq!(names("other.py:name"), Vec::<&str>::new());
        assert_eq!(names("missing"), Vec::<&str>::new());
    }

    #[test]
    fn test_name_only_candidate_is_an_edge_only_on_request() {
        let view = view(&[
            ("a.py", "def run():\n    return helper()\n"),
            ("b.py", "def helper():\n    return 1\n"),
        ]);
        let run = view.resolve("run");
        let calls = view.calls_of(run[0]);
        assert_eq!(calls[0].status, ResolutionStatus::Ambiguous);
        assert_eq!(calls[0].basis, Some(ResolutionBasis::NameOnly));
        assert_eq!(calls[0].candidates[0].qualified_name, "b.py#helper");
        assert!(view.callees(&run, 1, DEFAULT_MIN_BASIS).is_empty());
        let lowered = view.callees(&run, 1, ResolutionBasis::NameOnly);
        assert_eq!(view.symbols[lowered[0][0]].qualified_name, "b.py#helper");
    }

    #[test]
    fn test_levels_follow_only_edges() {
        let view = view(FILES);
        let names = |levels: Vec<Vec<usize>>| -> Vec<Vec<&str>> {
            levels
                .into_iter()
                .map(|level| {
                    level
                        .into_iter()
                        .map(|position| view.symbols[position].qualified_name.as_str())
                        .collect()
                })
                .collect()
        };
        let run = view.resolve("run");
        assert_eq!(
            names(view.callees(&run, 5, DEFAULT_MIN_BASIS)),
            [
                vec!["shop.py#Item.name"],
                vec!["shop.py#helper"],
                vec!["shop.py#leaf"]
            ]
        );
        assert_eq!(
            names(view.callees(&run, 1, DEFAULT_MIN_BASIS)),
            [vec!["shop.py#Item.name"]]
        );
        assert!(view.callees(&run, 0, DEFAULT_MIN_BASIS).is_empty());
        let leaf = view.resolve("leaf");
        assert_eq!(
            names(view.callers(&leaf, 2, DEFAULT_MIN_BASIS)),
            [vec!["shop.py#helper"], vec!["shop.py#Item.name"]]
        );
        // `len` is not in the repository. The call is evidence, not an edge.
        let calls = view.calls_of(leaf[0]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].call, "len");
        assert_eq!(calls[0].status, ResolutionStatus::Unresolved);
        assert_eq!(view.resolution_summary().unique_candidate, 3);
    }
}
