//! Builds the change set of a branch.
//!
//! The functions build the shared index at the base and at the head. They
//! read each file from the git tree, thus the working tree does not matter.
//! They compare the two indexes by symbol key.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::model::*;
use crate::analyzers::{complexity, hotspot, satd, stubs};
use crate::config::Config;
use crate::core::{
    is_test_file, AnalysisContext, Analyzer as _, ContentSource, FileSet, Result, TreeSource,
};
use crate::git::GitRepo;
use crate::index::build::CodeIndex;
use crate::index::extract::FileSymbols;
use crate::index::model::{ResolutionBasis, ResolutionStatus, Symbol, SymbolKind};
use crate::index::view::{CallView, DEFAULT_MIN_BASIS};

const SHORT_SHA: usize = 7;

/// The tree of one commit with its index.
struct Side {
    source: TreeSource,
    /// Each file of the tree, with the files that have no supported language.
    tree_files: BTreeSet<String>,
    file_set: FileSet,
    index: Arc<CodeIndex>,
    edges: Edges,
}

/// The calls between definitions that the index resolves, by the key of the
/// caller and the key of the callee.
#[derive(Default)]
struct Edges {
    calls: BTreeMap<(String, String), EdgeInfo>,
    callers_of: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Copy)]
struct EdgeInfo {
    /// The weakest basis of the call sites between the two definitions.
    basis: ResolutionBasis,
    /// The first line of a call site.
    line: u32,
}

impl Edges {
    fn from_view(view: &CallView) -> Self {
        let mut edges = Self::default();
        for call in &view.call_resolutions {
            if !matches!(
                call.status,
                ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
            ) {
                continue;
            }
            let Some(basis) = call.basis.filter(|b| b.rank() >= DEFAULT_MIN_BASIS.rank()) else {
                continue;
            };
            for callee in &call.candidates {
                let key = (
                    call.caller.qualified_name.clone(),
                    callee.qualified_name.clone(),
                );
                edges
                    .calls
                    .entry(key)
                    .and_modify(|info| {
                        if basis.rank() < info.basis.rank() {
                            info.basis = basis;
                        }
                        info.line = info.line.min(call.line);
                    })
                    .or_insert(EdgeInfo {
                        basis,
                        line: call.line,
                    });
                edges
                    .callers_of
                    .entry(callee.qualified_name.clone())
                    .or_default()
                    .insert(call.caller.qualified_name.clone());
            }
        }
        edges
    }

    fn direct_callers(&self, key: &str) -> impl Iterator<Item = &String> {
        self.callers_of.get(key).into_iter().flatten()
    }

    /// The number of definitions that reach `key` through calls.
    fn transitive_callers(&self, key: &str) -> usize {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut queue: VecDeque<&str> = VecDeque::from([key]);
        while let Some(current) = queue.pop_front() {
            for caller in self.direct_callers(current) {
                if caller != key && seen.insert(caller) {
                    queue.push_back(caller);
                }
            }
        }
        seen.len()
    }

    fn weakest_basis(&self, key: &str) -> Option<ResolutionBasis> {
        self.direct_callers(key)
            .filter_map(|caller| self.calls.get(&(caller.clone(), key.to_string())))
            .map(|info| info.basis)
            .min_by_key(|basis| basis.rank())
    }
}

impl Side {
    fn load(repository: &Path, revision: &str, config: &Config) -> Result<Self> {
        let source = TreeSource::new(repository, revision)?;
        let tree_files = source
            .list_files()?
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        let file_set = FileSet::from_tree_source(&source, config)?;
        let index = {
            let context = AnalysisContext::new(&file_set, config, Some(repository))
                .with_content_source(Arc::new(source.clone()));
            context.code_index()?
        };
        let edges = Edges::from_view(&CallView::from_index(&index));
        Ok(Self {
            source,
            tree_files,
            file_set,
            index,
            edges,
        })
    }

    fn symbols(&self) -> BTreeMap<&str, (&FileSymbols, &Symbol)> {
        self.index
            .files
            .iter()
            .flat_map(|file| file.symbols.iter().map(move |symbol| (file, symbol)))
            .filter(|(_, symbol)| symbol.kind != SymbolKind::Field)
            .map(|(file, symbol)| (symbol.key.as_str(), (file, symbol)))
            .collect()
    }

    /// The files of `paths` that the file set has, as a file set that reads
    /// from this tree.
    fn subset(&self, paths: &BTreeSet<String>) -> FileSet {
        let files = self
            .file_set
            .iter()
            .filter(|path| paths.contains(path.to_string_lossy().as_ref()))
            .cloned()
            .collect();
        FileSet::from_files(PathBuf::from("."), files)
    }

    fn content(&self, file: &str) -> Option<Vec<u8>> {
        self.source.read(Path::new(file)).ok()
    }
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor | SymbolKind::Macro
    )
}

fn text_of<'a>(content: &'a [u8], symbol: &Symbol) -> &'a [u8] {
    content
        .get(symbol.range.start_byte..symbol.range.end_byte)
        .unwrap_or_default()
}

fn component_of(file: &str) -> String {
    match file.rsplit_once('/') {
        Some((directory, _)) => directory.to_string(),
        None => ".".to_string(),
    }
}

fn is_test_symbol(file: &str, symbol: Option<&Symbol>) -> bool {
    is_test_file(Path::new(file))
        || symbol.is_some_and(|symbol| {
            symbol.name.starts_with("test_") || symbol.name.starts_with("Test")
        })
}

/// The complexity of each function of the files of one side.
struct ComplexityMap {
    files: BTreeMap<String, Vec<complexity::FunctionResult>>,
}

impl ComplexityMap {
    fn of(side: &Side, paths: &BTreeSet<String>, config: &Config) -> Self {
        let files = side.subset(paths);
        let context = AnalysisContext::new(&files, config, None)
            .with_content_source(Arc::new(side.source.clone()));
        let analysis = complexity::Analyzer::default().analyze(&context);
        let files = analysis
            .map(|analysis| {
                analysis
                    .files
                    .into_iter()
                    .map(|file| (file.file.replace('\\', "/"), file.functions))
                    .collect()
            })
            .unwrap_or_default();
        Self { files }
    }

    fn max_cyclomatic(&self, file: &str) -> Option<u32> {
        self.files
            .get(file)?
            .iter()
            .map(|function| function.metrics.cyclomatic)
            .max()
    }

    /// The function that has the symbol: the function with the same start
    /// line, or else the tightest function that has the name of the symbol
    /// and overlaps the range of the symbol.
    fn of_symbol(&self, file: &str, symbol: &Symbol) -> Option<Complexity> {
        let functions = self.files.get(file)?;
        let line = symbol.range.start_line;
        let end = symbol.range.end_line;
        let found = functions
            .iter()
            .find(|function| function.start_line == line)
            .or_else(|| {
                functions
                    .iter()
                    .filter(|function| {
                        function.start_line <= end
                            && function.end_line >= line
                            && (function.name == symbol.name
                                || function.name.ends_with(&symbol.name))
                    })
                    .min_by_key(|function| {
                        (function.end_line - function.start_line, function.start_line)
                    })
            })?;
        Some(Complexity {
            cyclomatic: found.metrics.cyclomatic,
            cognitive: found.metrics.cognitive,
        })
    }
}

/// Builds the change set of `HEAD` against the merge base of `HEAD` and
/// `target`. Without `target`, the function uses `main` or `master`.
pub fn compute(repository: &Path, target: Option<&str>, config: &Config) -> Result<ChangeSet> {
    let target = match target {
        Some(target) => target.to_string(),
        None => crate::analyzers::changes::detect_default_branch(repository)?,
    };
    let repo = GitRepo::open(repository)?;
    let base_sha = repo.merge_base(&target, "HEAD")?;
    let head_sha = repo.head_sha()?;
    let file_changes = repo.diff_stats(&base_sha, "HEAD")?;

    let (base, head) = (
        Side::load(repository, &base_sha, config)?,
        Side::load(repository, &head_sha, config)?,
    );
    let changed_files: BTreeSet<String> = file_changes
        .iter()
        .map(|change| change.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let base_complexity = ComplexityMap::of(&base, &changed_files, config);
    let head_complexity = ComplexityMap::of(&head, &changed_files, config);

    let symbols = compare_symbols(
        &base,
        &head,
        &changed_files,
        &base_complexity,
        &head_complexity,
    );
    let graph = build_graph(&base, &head, &symbols);
    let new_findings = new_findings(&base, &head, &changed_files, config);
    let files = file_entries(
        repository,
        &repo,
        &file_changes,
        &base,
        &head,
        &symbols,
        &head_complexity,
    );
    let summary = summarize(&symbols, &files, &new_findings, &file_changes);

    Ok(ChangeSet {
        target,
        base: base_sha.chars().take(SHORT_SHA).collect(),
        head: head_sha.chars().take(SHORT_SHA).collect(),
        summary,
        symbols,
        files,
        new_findings,
        graph,
    })
}

fn compare_symbols(
    base: &Side,
    head: &Side,
    changed_files: &BTreeSet<String>,
    base_complexity: &ComplexityMap,
    head_complexity: &ComplexityMap,
) -> Vec<SymbolChange> {
    let before = base.symbols();
    let after = head.symbols();
    let keys: BTreeSet<&str> = before
        .keys()
        .chain(after.keys())
        .copied()
        .filter(|key| {
            let file = key.split_once('#').map_or(*key, |(file, _)| file);
            changed_files.contains(file)
        })
        .collect();

    let mut contents: BTreeMap<(bool, String), Option<Vec<u8>>> = BTreeMap::new();
    let mut content = |is_head: bool, side: &Side, file: &str| {
        contents
            .entry((is_head, file.to_string()))
            .or_insert_with(|| side.content(file))
            .clone()
    };

    let mut changes = Vec::new();
    for key in keys {
        let (state, symbol_before, symbol_after) = match (before.get(key), after.get(key)) {
            (None, Some((file, symbol))) => (ChangeState::Added, None, Some((*file, *symbol))),
            (Some((file, symbol)), None) => (ChangeState::Removed, Some((*file, *symbol)), None),
            (Some((file_before, old)), Some((file_after, new))) => {
                let same_text = !is_callable(new.kind)
                    || content(false, base, &file_before.file)
                        .zip(content(true, head, &file_after.file))
                        .is_some_and(|(old_content, new_content)| {
                            text_of(&old_content, old) == text_of(&new_content, new)
                        });
                if old.signature == new.signature && same_text {
                    continue;
                }
                (
                    ChangeState::Changed,
                    Some((*file_before, *old)),
                    Some((*file_after, *new)),
                )
            }
            (None, None) => continue,
        };
        let primary = symbol_after
            .or(symbol_before)
            .expect("one side has the key");
        let (file, symbol) = primary;
        let (edges, definitions) = if state == ChangeState::Removed {
            (&base.edges, &before)
        } else {
            (&head.edges, &after)
        };
        let mut direct_callers: Vec<CallerRef> = edges
            .direct_callers(key)
            .map(|caller| {
                let info = edges.calls.get(&(caller.clone(), key.to_string()));
                let caller_symbol = definitions.get(caller.as_str()).copied();
                let caller_file = caller.split_once('#').map_or("", |(file, _)| file);
                CallerRef {
                    key: caller.clone(),
                    file: caller_file.to_string(),
                    line: info.map_or(0, |info| info.line),
                    is_test: is_test_symbol(caller_file, caller_symbol.map(|(_, s)| s)),
                }
            })
            .collect();
        direct_callers.sort_by(|left, right| left.key.cmp(&right.key));
        changes.push(SymbolChange {
            key: key.to_string(),
            name: symbol.name.clone(),
            kind: symbol.kind,
            file: file.file.clone(),
            line: symbol.range.start_line,
            state,
            signature_before: symbol_before.map(|(_, s)| s.signature.clone()),
            signature_after: symbol_after.map(|(_, s)| s.signature.clone()),
            complexity_before: symbol_before
                .filter(|(_, s)| is_callable(s.kind))
                .and_then(|(f, s)| base_complexity.of_symbol(&f.file, s)),
            complexity_after: symbol_after
                .filter(|(_, s)| is_callable(s.kind))
                .and_then(|(f, s)| head_complexity.of_symbol(&f.file, s)),
            direct_callers,
            transitive_callers: edges.transitive_callers(key),
            weakest_basis: edges
                .weakest_basis(key)
                .map(|basis| basis.name().to_string()),
        });
    }
    changes.sort_by(|left, right| {
        (&left.file, left.line, &left.key).cmp(&(&right.file, right.line, &right.key))
    });
    changes
}

/// The changed symbols, their direct callers as context, and the calls
/// between them. A call is added when only the head has it, and removed when
/// only the base has it.
fn build_graph(base: &Side, head: &Side, symbols: &[SymbolChange]) -> ChangeGraph {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let node_of = |key: &str, state: ChangeState| {
        let file = key.split_once('#').map_or(key, |(file, _)| file);
        GraphNode {
            key: key.to_string(),
            label: key
                .split_once('#')
                .map_or(key, |(_, name)| name)
                .to_string(),
            file: file.to_string(),
            component: component_of(file),
            state,
        }
    };
    for symbol in symbols {
        nodes.insert(symbol.key.clone(), node_of(&symbol.key, symbol.state));
    }
    for symbol in symbols {
        for caller in &symbol.direct_callers {
            nodes
                .entry(caller.key.clone())
                .or_insert_with(|| node_of(&caller.key, ChangeState::Unchanged));
        }
    }
    let state_of = |from: &str, to: &str| {
        let pair = (from.to_string(), to.to_string());
        match (
            base.edges.calls.contains_key(&pair),
            head.edges.calls.contains_key(&pair),
        ) {
            (true, true) => ChangeState::Unchanged,
            (false, true) => ChangeState::Added,
            _ => ChangeState::Removed,
        }
    };
    let pairs: BTreeSet<&(String, String)> = base
        .edges
        .calls
        .keys()
        .chain(head.edges.calls.keys())
        .collect();
    let edges = pairs
        .into_iter()
        .filter(|(from, to)| nodes.contains_key(from) && nodes.contains_key(to))
        .map(|(from, to)| GraphEdge {
            from: from.clone(),
            to: to.clone(),
            state: state_of(from, to),
        })
        .collect();
    ChangeGraph {
        nodes: nodes.into_values().collect(),
        edges,
    }
}

/// The stubs and the debt comments of the changed files that the head has
/// more often than the base. A finding that only moved to a different line
/// is not new.
fn new_findings(
    base: &Side,
    head: &Side,
    changed_files: &BTreeSet<String>,
    config: &Config,
) -> Vec<NewFinding> {
    type Key = (FindingKind, String, String);
    let collect = |side: &Side| -> Vec<(Key, u32)> {
        let files = side.subset(changed_files);
        let context = AnalysisContext::new(&files, config, None)
            .with_content_source(Arc::new(side.source.clone()));
        let mut found: Vec<(Key, u32)> = Vec::new();
        if let Ok(analysis) = stubs::Analyzer::new().analyze(&context) {
            for stub in analysis.stubs {
                found.push((
                    (
                        FindingKind::Stub,
                        stub.file.clone(),
                        stub.snippet.trim().to_string(),
                    ),
                    stub.line,
                ));
            }
        }
        if let Ok(analysis) = satd::Analyzer::default().analyze(&context) {
            for item in analysis.items {
                found.push((
                    (
                        FindingKind::Satd,
                        item.file.clone(),
                        item.text.trim().to_string(),
                    ),
                    item.line,
                ));
            }
        }
        found.sort();
        found
    };
    let mut old_counts: BTreeMap<Key, usize> = BTreeMap::new();
    for (key, _) in collect(base) {
        *old_counts.entry(key).or_default() += 1;
    }
    let mut findings = Vec::new();
    for (key, line) in collect(head) {
        match old_counts.get_mut(&key) {
            Some(count) if *count > 0 => *count -= 1,
            _ => findings.push(NewFinding {
                kind: key.0,
                file: key.1.replace('\\', "/"),
                line,
                detail: key.2,
            }),
        }
    }
    findings.sort_by(|left, right| {
        (&left.file, left.line, &left.detail).cmp(&(&right.file, right.line, &right.detail))
    });
    findings
}

fn relative_hotspot_path(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .trim_start_matches("./")
        .replace('\\', "/")
}

fn file_entries(
    repository: &Path,
    repo: &GitRepo,
    file_changes: &[crate::git::FileChange],
    base: &Side,
    head: &Side,
    symbols: &[SymbolChange],
    head_complexity: &ComplexityMap,
) -> Vec<FileChange> {
    let paths: Vec<String> = file_changes
        .iter()
        .map(|change| change.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let owners = repo.file_churn(&paths).unwrap_or_default();
    let hotspots: BTreeMap<String, String> = hotspot::Analyzer::new()
        .analyze_project(repository)
        .map(|analysis| {
            analysis
                .hotspots
                .into_iter()
                .map(|entry| {
                    (
                        relative_hotspot_path(repository, &entry.file),
                        entry.severity.to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let mut files: Vec<FileChange> = file_changes
        .iter()
        .map(|change| {
            let file = change.path.to_string_lossy().replace('\\', "/");
            let state = match (
                base.tree_files.contains(&file),
                head.tree_files.contains(&file),
            ) {
                (false, true) => ChangeState::Added,
                (true, false) => ChangeState::Removed,
                _ => ChangeState::Changed,
            };
            let mut authors: Vec<String> = owners
                .get(&file)
                .map(|entry| entry.authors.iter().cloned().collect())
                .unwrap_or_default();
            authors.sort();
            FileChange {
                symbols_changed: symbols.iter().filter(|symbol| symbol.file == file).count(),
                max_cyclomatic: head_complexity.max_cyclomatic(&file),
                hotspot: hotspots.get(&file).cloned(),
                owners: authors,
                additions: change.additions,
                deletions: change.deletions,
                state,
                file,
            }
        })
        .collect();
    files.sort_by(|left, right| left.file.cmp(&right.file));
    files
}

fn summarize(
    symbols: &[SymbolChange],
    files: &[FileChange],
    new_findings: &[NewFinding],
    file_changes: &[crate::git::FileChange],
) -> Summary {
    let count = |state: ChangeState| symbols.iter().filter(|s| s.state == state).count();
    let changed_keys: BTreeSet<&str> = symbols.iter().map(|s| s.key.as_str()).collect();
    let affected: BTreeSet<&str> = symbols
        .iter()
        .flat_map(|symbol| symbol.direct_callers.iter())
        .map(|caller| caller.key.as_str())
        .filter(|key| !changed_keys.contains(key))
        .collect();
    let sum = |select: fn(&SymbolChange) -> Option<Complexity>| -> u32 {
        symbols
            .iter()
            .filter_map(select)
            .map(|c| c.cyclomatic)
            .sum()
    };
    let count_findings = |kind: FindingKind| new_findings.iter().filter(|f| f.kind == kind).count();
    Summary {
        files_changed: file_changes.len(),
        lines_added: file_changes.iter().map(|c| c.additions).sum(),
        lines_deleted: file_changes.iter().map(|c| c.deletions).sum(),
        symbols_added: count(ChangeState::Added),
        symbols_removed: count(ChangeState::Removed),
        symbols_changed: count(ChangeState::Changed),
        callers_affected: affected.len(),
        cyclomatic_before: sum(|s| s.complexity_before),
        cyclomatic_after: sum(|s| s.complexity_after),
        new_stubs: count_findings(FindingKind::Stub),
        new_satd: count_findings(FindingKind::Satd),
        hotspots_touched: files
            .iter()
            .filter(|file| matches!(file.hotspot.as_deref(), Some("critical" | "high")))
            .count(),
    }
}
