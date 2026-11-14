//! A view of the shared index for the commands that follow calls: `symbol`,
//! `impact`, and the MCP tools `get_symbol` and `impact`.
//!
//! An edge of the view exists only for a call with the status `Resolved` or
//! `UniqueCandidate`. A call with a different status is evidence, not an edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

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
    /// The first byte of the body, or of the declaration when it has no body.
    pub body_start: usize,
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

impl ResolutionSummary {
    fn count(&mut self, status: ResolutionStatus) {
        match status {
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate => {
                self.unique_candidate += 1;
            }
            ResolutionStatus::Ambiguous => self.ambiguous += 1,
            ResolutionStatus::Unresolved => self.unresolved += 1,
            ResolutionStatus::External => self.external += 1,
        }
    }
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
    /// The counts of all call sites of the input, also of the calls that
    /// are not in a definition.
    summary: ResolutionSummary,
    /// The number of source files that the index read.
    pub sources: usize,
    /// The hash of the names and the contents of the source files and the
    /// manifest files. `None` when the view does not come from a `ViewCache`.
    pub fingerprint: Option<String>,
}

impl CallView {
    pub fn from_index(index: &CodeIndex) -> Self {
        Self::from_index_where(index, |_| true)
    }

    /// The view of the files for which `keep` is true. A call from a file that
    /// `keep` rejects is not in the view.
    pub fn from_index_where(index: &CodeIndex, keep: impl Fn(&str) -> bool) -> Self {
        let mut symbols = Vec::new();
        let mut by_key = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for file in index.files.iter().filter(|file| keep(&file.file)) {
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
                    body_start: symbol
                        .body_range
                        .map_or(symbol.range.start_byte, |body| body.start_byte),
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
        let mut summary = ResolutionSummary::default();
        for reference in index
            .files
            .iter()
            .filter(|file| keep(&file.file))
            .flat_map(|file| &file.references)
        {
            summary.count(reference.resolution.status);
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
            summary,
            sources: index.files.len(),
            fingerprint: None,
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
                // A destructor name also has `~`.
                let path = path
                    .rsplit_once('~')
                    .filter(|(_, ordinal)| {
                        !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .map_or(path, |(name, _)| name);
                file.is_none_or(|file| symbol.file == file)
                    && (path == name || path.ends_with(&format!(".{name}")))
            })
            .collect()
    }

    /// Each edge as a pair of a caller and a callee, once, in order. An edge
    /// counts when its basis is `min_basis` or a stronger basis.
    pub fn edges(&self, min_basis: ResolutionBasis) -> Vec<(usize, usize)> {
        let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
        for (caller, callees) in self.callees.iter().enumerate() {
            for (callee, basis) in callees {
                if basis.rank() >= min_basis.rank() {
                    edges.insert((caller, *callee));
                }
            }
        }
        edges.into_iter().collect()
    }

    /// The position of the definition with the key.
    pub fn position_of(&self, key: &str) -> Option<usize> {
        self.by_key.get(key).copied()
    }

    /// The definitions that one definition calls, with the basis of each call. A target
    /// can occur with more than one basis. The list is in the order of the definitions.
    pub fn callee_edges(&self, symbol: usize) -> &[(usize, ResolutionBasis)] {
        &self.callees[symbol]
    }

    /// The call sites in the body of one definition, in source order.
    pub fn calls_of(&self, symbol: usize) -> &[CallEvidence] {
        &self.call_resolutions[self.call_resolution_ranges[symbol].clone()]
    }

    /// The number of call sites of each status in all files of the input.
    pub fn resolution_summary(&self) -> ResolutionSummary {
        self.summary.clone()
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

/// The hash of the relative names and the content hashes of the two file
/// sets. The sets are in the order of their names, thus the result is
/// the same for the same content.
fn fingerprint(
    root: &Path,
    sources: &BTreeMap<PathBuf, blake3::Hash>,
    manifests: &BTreeMap<PathBuf, blake3::Hash>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for (kind, files) in [("source", sources), ("manifest", manifests)] {
        hasher.update(&(files.len() as u64).to_le_bytes());
        for (path, hash) in files {
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            hasher.update(kind.as_bytes());
            hasher.update(&(relative.len() as u64).to_le_bytes());
            hasher.update(relative.as_bytes());
            hasher.update(hash.as_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}

/// The view of the last set of files of a session. A second request for
/// files with the same content and the same manifest files gets the same view.
#[derive(Default)]
pub struct ViewCache {
    root: PathBuf,
    hashes: BTreeMap<PathBuf, blake3::Hash>,
    manifests: BTreeMap<PathBuf, blake3::Hash>,
    last: Option<Arc<CallView>>,
}

impl ViewCache {
    pub fn load(&mut self, root: &Path, files: &[PathBuf]) -> Result<Arc<CallView>> {
        self.load_with(
            root,
            files,
            &|path| Ok(std::fs::read(path)?),
            &super::build::list_directory,
        )
    }

    /// Reads each source file and each manifest file one time. The hashes and
    /// the index come from those bytes, thus a view always has the content
    /// that its hashes name.
    pub fn load_with(
        &mut self,
        root: &Path,
        files: &[PathBuf],
        read: super::build::ReadFile<'_>,
        list: super::build::ListDirectory<'_>,
    ) -> Result<Arc<CallView>> {
        let root = root.canonicalize()?;
        let mut sources: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        for path in files {
            let path = path.canonicalize()?;
            if !path.starts_with(&root) {
                return Err(Error::analysis("Index file escapes repository"));
            }
            if Language::detect(&path).is_none() {
                continue;
            }
            let content = read(&path)?;
            sources.insert(path, content);
        }
        let hashes: BTreeMap<PathBuf, blake3::Hash> = sources
            .iter()
            .map(|(path, content)| (path.clone(), blake3::hash(content)))
            .collect();
        let paths: Vec<PathBuf> = sources.keys().cloned().collect();
        let listings = Mutex::new(BTreeMap::new());
        let record = |directory: &Path| -> Vec<PathBuf> {
            let found = list(directory);
            listings
                .lock()
                .insert(directory.to_path_buf(), found.clone());
            found
        };
        // A manifest that cannot be read has no effect on the index.
        let mut snapshot: BTreeMap<PathBuf, Option<Vec<u8>>> = sources
            .into_iter()
            .map(|(path, content)| (path, Some(content)))
            .collect();
        let mut manifests = BTreeMap::new();
        for path in super::build::manifest_paths(&root, &paths, &record) {
            let content = read(&path).ok();
            if let Some(content) = &content {
                manifests.insert(path.clone(), blake3::hash(content));
            }
            snapshot.insert(path, content);
        }
        if let Some(last) = self
            .last
            .as_ref()
            .filter(|_| self.root == root && self.hashes == hashes && self.manifests == manifests)
        {
            return Ok(Arc::clone(last));
        }
        let snapshot_read = |path: &Path| -> Result<Vec<u8>> {
            match snapshot.get(path) {
                Some(Some(content)) => Ok(content.clone()),
                Some(None) => Err(Error::analysis("File was not readable")),
                None => read(path),
            }
        };
        let snapshot_list = |directory: &Path| -> Vec<PathBuf> {
            listings.lock().get(directory).cloned().unwrap_or_default()
        };
        let index = CodeIndex::build(&root, &paths, &snapshot_read, &snapshot_list);
        let mut view = CallView::from_index(&index);
        view.fingerprint = Some(fingerprint(&root, &hashes, &manifests));
        let view = Arc::new(view);
        self.root = root;
        self.hashes = hashes;
        self.manifests = manifests;
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

    #[test]
    fn test_edges_list_each_edge_one_time_in_definition_order() {
        let view = view(FILES);
        let name = |position: usize| view.symbols[position].qualified_name.as_str();

        let edges: Vec<(&str, &str)> = view
            .edges(DEFAULT_MIN_BASIS)
            .into_iter()
            .map(|(caller, callee)| (name(caller), name(callee)))
            .collect();

        assert_eq!(
            edges,
            [
                ("shop.py#Item.name", "shop.py#helper"),
                ("shop.py#helper", "shop.py#leaf"),
                ("shop.py#run", "shop.py#Item.name"),
            ]
        );
    }

    #[test]
    fn test_view_of_chosen_files_holds_only_their_definitions_and_calls() {
        let mut facts = index(&[
            ("a.py", "def run():\n    return helper()\n"),
            ("b.py", "def helper():\n    return missing()\n"),
        ]);
        resolve_references(&mut facts);
        let index = CodeIndex { files: facts };

        let view = CallView::from_index_where(&index, |file| file == "b.py");

        let names: Vec<&str> = view
            .symbols
            .iter()
            .map(|symbol| symbol.qualified_name.as_str())
            .collect();
        assert_eq!(names, ["b.py#helper"]);
        assert_eq!(view.resolution_summary().unresolved, 1);
        assert_eq!(view.resolution_summary().unique_candidate, 0);
        assert!(view.edges(DEFAULT_MIN_BASIS).is_empty());
    }

    #[test]
    fn test_definition_has_the_byte_of_the_start_of_its_body() {
        let source = FILES[0].1;
        let view = view(FILES);
        let leaf = view.resolve("leaf")[0];

        assert!(source[view.symbols[leaf].body_start..].starts_with("return len"));
    }

    /// A call outside of each definition is a call site of the input. The
    /// summary counts it. It has no caller, thus it gives no edge.
    #[test]
    fn test_summary_counts_calls_outside_of_definitions() {
        let view = view(&[("a.py", "def f():\n    pass\n\n\nf()\nmissing()\n")]);
        let summary = view.resolution_summary();
        assert_eq!(summary.unique_candidate, 1);
        assert_eq!(summary.unresolved, 1);
        assert!(view
            .callers(&view.resolve("f"), 1, DEFAULT_MIN_BASIS)
            .is_empty());
    }

    /// `~` starts a C++ destructor name. Only `~` and digits at the end of a
    /// key are an ordinal.
    #[test]
    fn test_query_forms_name_a_destructor() {
        let source = "struct X {\n    ~X() {}\n    void run() {}\n    void run(int value) {}\n};\n";
        let view = view(&[("a.cpp", source)]);
        let names = |query: &str| -> Vec<&str> {
            view.resolve(query)
                .into_iter()
                .map(|position| view.symbols[position].qualified_name.as_str())
                .collect()
        };
        assert_eq!(names("a.cpp#X.~X"), ["a.cpp#X.~X"]);
        assert_eq!(names("~X"), ["a.cpp#X.~X"]);
        assert_eq!(names("X.~X"), ["a.cpp#X.~X"]);
        assert_eq!(names("a.cpp:~X"), ["a.cpp#X.~X"]);
        assert_eq!(names("X.run"), ["a.cpp#X.run", "a.cpp#X.run~2"]);
    }

    /// The view must come from the bytes that the cache hashed. A file that
    /// changes between the hash and the build must not give a view that has a
    /// different content from its hash.
    #[test]
    fn test_cache_builds_the_view_from_the_hashed_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let file = root.join("a.py");
        std::fs::write(&file, "def before():\n    pass\n").unwrap();
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let read = |path: &Path| -> Result<Vec<u8>> {
            let _ = path;
            let count = reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if count == 0 {
                b"def before():\n    pass\n".to_vec()
            } else {
                b"def after():\n    pass\n".to_vec()
            })
        };
        let mut cache = ViewCache::default();
        let view = cache
            .load_with(root, &[file], &read, &super::super::build::list_directory)
            .unwrap();
        assert_eq!(view.resolve("before").len(), 1);
        assert!(view.resolve("after").is_empty());
    }

    /// The index reads the manifest files. A change of a manifest without a
    /// change of a source file gives a new view.
    #[test]
    fn test_cache_reads_the_manifests_again() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("lib/src")).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let manifest = root.join("lib/package.json");
        std::fs::write(&manifest, r#"{"name": "shop", "exports": "./src/a.js"}"#).unwrap();
        std::fs::write(root.join("lib/src/a.js"), "export function run() {}\n").unwrap();
        std::fs::write(root.join("lib/src/b.js"), "export function run() {}\n").unwrap();
        std::fs::write(
            root.join("app/main.js"),
            "import { run } from \"shop\";\n\nexport function go() {\n  run();\n}\n",
        )
        .unwrap();
        let files: Vec<PathBuf> = ["lib/src/a.js", "lib/src/b.js", "app/main.js"]
            .iter()
            .map(|file| root.join(file))
            .collect();
        let callee = |view: &CallView| -> Vec<String> {
            let go = view.resolve("go");
            view.callees(&go, 1, DEFAULT_MIN_BASIS)
                .into_iter()
                .flatten()
                .map(|position| view.symbols[position].qualified_name.clone())
                .collect()
        };
        let mut cache = ViewCache::default();
        let first = cache.load(root, &files).unwrap();
        assert_eq!(callee(&first), ["lib/src/a.js#run"]);
        let again = cache.load(root, &files).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        std::fs::write(&manifest, r#"{"name": "shop", "exports": "./src/b.js"}"#).unwrap();
        let changed = cache.load(root, &files).unwrap();
        assert_eq!(callee(&changed), ["lib/src/b.js#run"]);
    }

    /// The fingerprint names the content: a rewrite with the same bytes keeps
    /// it, a change of a source or a manifest file gives a new one.
    #[test]
    fn test_fingerprint_follows_content() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let source = root.join("a.js");
        let manifest = root.join("package.json");
        std::fs::write(&source, "export function run() {}\n").unwrap();
        std::fs::write(&manifest, r#"{"name": "shop"}"#).unwrap();
        let mut cache = ViewCache::default();
        let fingerprint = |cache: &mut ViewCache| -> String {
            let view = cache.load(root, std::slice::from_ref(&source)).unwrap();
            assert_eq!(view.sources, 1);
            view.fingerprint.clone().expect("a fingerprint")
        };
        let first = fingerprint(&mut cache);
        assert_eq!(first.len(), 64);
        std::fs::write(&source, "export function run() {}\n").unwrap();
        assert_eq!(fingerprint(&mut cache), first);
        std::fs::write(&manifest, r#"{"name": "other"}"#).unwrap();
        let after_manifest = fingerprint(&mut cache);
        assert_ne!(after_manifest, first);
        std::fs::write(&source, "export function walk() {}\n").unwrap();
        assert_ne!(fingerprint(&mut cache), after_manifest);
        let alone = ViewCache::default()
            .load(root, std::slice::from_ref(&source))
            .unwrap();
        assert_eq!(alone.fingerprint, Some(fingerprint(&mut cache)));
    }
}
