//! The security analysis of a set of files: the sinks, the entry points, and the paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::entry::{
    entry_points_of_file, find_entry_points_with, EntryPoint, EntryRule, OnDemandTrees,
};
use super::matcher::{file_sinks, Repository};
use super::reach::{coverage, reachability, Coverage, InlineRegions, Reachability};
use super::spec::SinkSpec;
use super::suppress::{self, InvalidSuppression, Suppressed};
use super::{find_sinks_with, Sink};
use crate::core::Language;
use crate::index::build::{list_directory, CodeIndex, ListDirectory, ReadFile};
use crate::index::extract::FileSymbols;
use crate::index::headers;
use crate::index::model::{Reference, ResolutionBasis};
use crate::index::view::{CallView, FunctionValue, DEFAULT_MIN_BASIS};
use crate::parser::ParseResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    /// The sinks that no comment suppresses.
    pub sinks: Vec<Sink>,
    pub suppressed: Vec<Suppressed>,
    pub invalid_suppressions: Vec<InvalidSuppression>,
    pub entry_points: Vec<EntryPoint>,
    pub reachability: Reachability,
    pub coverage: Coverage,
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The options of an analysis.
#[derive(Debug, Clone)]
pub struct Options {
    /// An edge of a path has this basis or a stronger basis.
    pub min_basis: ResolutionBasis,
    /// More rows for sinks, from the configuration. The rows live as long as the process.
    pub sink_rows: &'static [SinkSpec],
    /// More entry points, from the configuration.
    pub entry_rules: Vec<EntryRule>,
    /// Look for the sinks of the experimental categories also.
    pub experimental: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_basis: DEFAULT_MIN_BASIS,
            sink_rows: &[],
            entry_rules: Vec::new(),
            experimental: false,
        }
    }
}

/// Analyzes `files`. The paths in the result are relative to `root`.
///
/// An edge of a path has the basis `min_basis` or a stronger basis.
pub fn analyze(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    list: ListDirectory<'_>,
    min_basis: ResolutionBasis,
) -> crate::core::Result<Analysis> {
    let index = CodeIndex::build(root, files, read, list);
    let options = Options {
        min_basis,
        ..Options::default()
    };
    analyze_with_index(&index, root, files, read, &options)
}

/// Analyzes `files` with an index that the caller already built from the same files.
pub fn analyze_with_index(
    index: &CodeIndex,
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    options: &Options,
) -> crate::core::Result<Analysis> {
    let by_relative: BTreeMap<String, &PathBuf> = files
        .iter()
        .map(|path| (relative(root, path), path))
        .collect();
    let parse_file = |position: usize| -> crate::core::Result<Option<ParseResult>> {
        let file = &index.files[position];
        let Some(path) = by_relative.get(&file.file) else {
            return Ok(None);
        };
        headers::parse_unchanged(
            file,
            || read(path),
            |content| headers::parse(path, file.language, content),
        )
    };
    let (all_sinks, comments, entry_points, kept) =
        match findings_one_tree_at_a_time(index, &parse_file, options)? {
            Some(findings) => findings,
            None => findings_with_all_trees(index, &parse_file, options)?,
        };
    let mut all_sinks = all_sinks;
    if !options.experimental {
        all_sinks.retain(|sink| !sink.category.is_experimental());
    }
    let names: Vec<String> = kept
        .iter()
        .map(|position| index.files[*position].file.clone())
        .collect();
    let (sinks, suppressed, invalid_suppressions) = suppress::split(all_sinks, &names, comments);
    let view =
        CallView::from_index_with_function_values(index, &inline_handlers(index, &entry_points));
    let reach = reachability(&view, &sinks, &entry_points, options.min_basis);
    let mut languages: Vec<Language> = Vec::new();
    for position in &kept {
        let language = index.files[*position].language;
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    Ok(Analysis {
        sinks,
        suppressed,
        invalid_suppressions,
        entry_points,
        reachability: reach,
        coverage: coverage(&languages, options.experimental),
    })
}

/// The inline handlers of the entry points, in the order of the file and the start. Each call
/// and each function reference that the index gives to the symbol around a handler belongs to
/// the innermost handler that contains it.
fn inline_handlers<'i>(index: &'i CodeIndex, entries: &[EntryPoint]) -> Vec<FunctionValue<'i>> {
    let mut values: Vec<FunctionValue<'i>> = Vec::new();
    let mut value_of_entry: Vec<Option<usize>> = vec![None; entries.len()];
    let mut by_key: BTreeMap<&str, usize> = BTreeMap::new();
    let mut order: Vec<usize> = (0..entries.len())
        .filter(|position| {
            entries[*position].inline_handler.is_some() && entries[*position].symbol.is_some()
        })
        .collect();
    order.sort_by_key(|position| {
        let entry = &entries[*position];
        let range = entry.inline_handler.as_ref().map(|inline| inline.range);
        (
            entry.file.as_str(),
            range.map(|range| range.start_byte),
            std::cmp::Reverse(range.map(|range| range.end_byte)),
        )
    });
    for position in order {
        let entry = &entries[position];
        let (Some(inline), Some(key)) = (&entry.inline_handler, &entry.symbol) else {
            continue;
        };
        let value = *by_key.entry(key.as_str()).or_insert_with(|| {
            values.push(FunctionValue {
                key: key.clone(),
                file: entry.file.clone(),
                range: inline.range,
                enclosing: inline.enclosing.clone(),
                parent: None,
                references: Vec::new(),
                function_references: Vec::new(),
                is_test: entry.in_test,
            });
            values.len() - 1
        });
        value_of_entry[position] = Some(value);
    }
    let regions = InlineRegions::new(entries, |position, _| value_of_entry[position]);
    let parents: Vec<Option<String>> = values
        .iter()
        .map(|value| {
            regions
                .parent(&value.file, value.enclosing.as_deref(), &value.range)
                .map(|parent| values[parent].key.clone())
        })
        .collect();
    for (value, parent) in values.iter_mut().zip(parents) {
        value.parent = parent;
    }
    let files: BTreeSet<String> = values.iter().map(|value| value.file.clone()).collect();
    for file in index.files.iter().filter(|file| files.contains(&file.file)) {
        let holder = |reference: &Reference| {
            regions.innermost(
                &file.file,
                reference.enclosing.as_deref(),
                reference.name_range.start_byte,
            )
        };
        for reference in &file.references {
            if let Some(value) = holder(reference) {
                values[value].references.push(reference);
            }
        }
        for reference in &file.function_references {
            if let Some(value) = holder(reference) {
                values[value].function_references.push(reference);
            }
        }
    }
    values
}

type Comments = Vec<(Vec<suppress::Allow>, Vec<InvalidSuppression>)>;
type Findings = (Vec<Sink>, Comments, Vec<EntryPoint>, Vec<usize>);

/// The sinks, the suppression comments, and the entry points of all files. The function
/// parses each file in the thread that uses it, and it drops the tree after use. Thus
/// the memory holds one tree for each thread, not one tree for each file.
///
/// `None` means that a file has no tree. The result for the files that have a tree
/// depends on the set of those files, thus the caller must use the other function.
fn findings_one_tree_at_a_time(
    index: &CodeIndex,
    parse_file: &(dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    options: &Options,
) -> crate::core::Result<Option<Findings>> {
    let files = &index.files;
    let repository = Repository::new(files);
    let trees = OnDemandTrees::new(parse_file);
    let per_file: Vec<Option<(Vec<Sink>, _, Vec<EntryPoint>)>> = (0..files.len())
        .into_par_iter()
        .map(|position| {
            let Some(tree) = parse_file(position)? else {
                return Ok(None);
            };
            let file = &files[position];
            Ok(Some((
                file_sinks(&tree, file, &repository, options.sink_rows),
                suppress::comments_of(&tree, &file.file),
                entry_points_of_file(position, &tree, files, &trees, &options.entry_rules),
            )))
        })
        .collect::<crate::core::Result<_>>()?;
    trees.finish()?;
    let mut sinks = Vec::new();
    let mut comments = Vec::new();
    let mut entry_points = Vec::new();
    for found in per_file {
        let Some((file_sinks, file_comments, file_entries)) = found else {
            return Ok(None);
        };
        sinks.extend(file_sinks);
        comments.push(file_comments);
        entry_points.extend(file_entries);
    }
    Ok(Some((
        sinks,
        comments,
        entry_points,
        (0..files.len()).collect(),
    )))
}

/// The same result as `findings_one_tree_at_a_time` for the files that have a tree.
/// The function keeps the trees of all files and a copy of their facts.
fn findings_with_all_trees(
    index: &CodeIndex,
    parse_file: &(dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    options: &Options,
) -> crate::core::Result<Findings> {
    let parsed: Vec<Option<ParseResult>> = (0..index.files.len())
        .into_par_iter()
        .map(parse_file)
        .collect::<crate::core::Result<_>>()?;
    let mut kept: Vec<FileSymbols> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    let mut trees: Vec<ParseResult> = Vec::new();
    for (position, (file, tree)) in index.files.iter().zip(parsed).enumerate() {
        if let Some(tree) = tree {
            kept.push(file.clone());
            positions.push(position);
            trees.push(tree);
        }
    }
    let sinks = find_sinks_with(&trees, &kept, options.sink_rows);
    let comments = trees
        .iter()
        .zip(&kept)
        .map(|(tree, file)| suppress::comments_of(tree, &file.file))
        .collect();
    let entry_points = find_entry_points_with(&trees, &kept, &options.entry_rules);
    Ok((sinks, comments, entry_points, positions))
}

/// Analyzes files that the caller already has as text. The names are relative paths.
pub fn analyze_sources(sources: &[(&str, &str)], min_basis: ResolutionBasis) -> Analysis {
    analyze_sources_with(
        sources,
        &Options {
            min_basis,
            ..Options::default()
        },
    )
}

/// The same as `analyze_sources`, with all the options.
pub fn analyze_sources_with(sources: &[(&str, &str)], options: &Options) -> Analysis {
    let contents: std::collections::HashMap<PathBuf, Vec<u8>> = sources
        .iter()
        .map(|(name, text)| (PathBuf::from(name), text.as_bytes().to_vec()))
        .collect();
    let paths: Vec<PathBuf> = sources
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect();
    let read = |path: &Path| {
        contents
            .get(path)
            .cloned()
            .ok_or_else(|| crate::core::Error::analysis("missing"))
    };
    let index = CodeIndex::build(Path::new(""), &paths, &read, &|_| Vec::new());
    analyze_with_index(&index, Path::new(""), &paths, &read, options)
        .expect("the sources are in memory and do not change")
}

/// Analyzes files of the file system with the default minimum basis.
pub fn analyze_files(root: &Path, files: &[PathBuf]) -> crate::core::Result<Analysis> {
    analyze(
        root,
        files,
        &|path| Ok(std::fs::read(path)?),
        &list_directory,
        DEFAULT_MIN_BASIS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sources: &[(&str, &str)], min_basis: ResolutionBasis) -> Analysis {
        analyze_sources(sources, min_basis)
    }

    const APP: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\", methods=[\"POST\"])\ndef run_command():\n    return helper()\n\ndef helper():\n    return deep()\n\ndef deep():\n    os.system(\"x\" + os.environ[\"A\"])\n\ndef dead(expression):\n    eval(expression)\n";

    #[test]
    fn one_tree_at_a_time_gives_the_findings_of_all_trees_at_once() {
        let sources = [
            ("app.py", APP),
            (
                "other.py",
                "import subprocess\n\ndef go(command):\n    subprocess.run(command, shell=True)  # omen:allow command_execution: tested\n",
            ),
        ];
        let paths: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::analysis("missing"))
        };
        let index = CodeIndex::build(Path::new(""), &paths, &read, &|_| Vec::new());
        let parse_file = |position: usize| -> crate::core::Result<Option<ParseResult>> {
            let file = &index.files[position];
            let path = Path::new(&file.file);
            Ok(headers::parse(path, file.language, &read(path)?))
        };
        let options = Options::default();

        let (sinks, comments, entries, kept) =
            findings_one_tree_at_a_time(&index, &parse_file, &options)
                .unwrap()
                .unwrap();
        let (all_sinks, all_comments, all_entries, all_kept) =
            findings_with_all_trees(&index, &parse_file, &options).unwrap();

        assert!(!sinks.is_empty());
        assert!(!entries.is_empty());
        assert_eq!(sinks, all_sinks);
        assert_eq!(entries, all_entries);
        assert_eq!(kept, all_kept);
        let counts = |comments: &Comments| -> Vec<(usize, usize)> {
            comments
                .iter()
                .map(|(allowed, invalid)| (allowed.len(), invalid.len()))
                .collect()
        };
        assert_eq!(counts(&comments), counts(&all_comments));
        assert_eq!(counts(&comments)[1], (1, 0));
    }

    /// A reader whose second read of `a.py` fails or gives other bytes.
    fn analyze_with_unstable_read(
        second_read: Option<&'static str>,
    ) -> crate::core::Result<Analysis> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        const DANGEROUS: &str = "def f(x):\n    eval(x)\n";
        let sources = [("a.py", DANGEROUS), ("b.py", "def g():\n    return 1\n")];
        let stable = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::analysis("missing"))
        };
        let paths: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();
        let index = CodeIndex::build(Path::new(""), &paths, &stable, &|_| Vec::new());
        let reads = AtomicUsize::new(0);
        let unstable = |path: &Path| {
            if path == Path::new("a.py") && reads.fetch_add(1, Ordering::SeqCst) == 0 {
                return match second_read {
                    Some(text) => Ok(text.as_bytes().to_vec()),
                    None => Err(crate::core::Error::analysis("gone")),
                };
            }
            stable(path)
        };
        analyze_with_index(
            &index,
            Path::new(""),
            &paths,
            &unstable,
            &Options::default(),
        )
    }

    const CHANGED: &str =
        "a.py changed or became unreadable during analysis; run the command again";

    #[test]
    fn a_file_that_fails_to_read_after_the_index_is_an_error() {
        let error = analyze_with_unstable_read(None).unwrap_err();
        assert!(error.to_string().contains(CHANGED), "{error}");
    }

    #[test]
    fn a_file_whose_bytes_changed_after_the_index_is_an_error() {
        let error = analyze_with_unstable_read(Some("def f(x):\n    return x\n")).unwrap_err();
        assert!(error.to_string().contains(CHANGED), "{error}");
    }

    #[test]
    fn a_file_without_a_tree_sends_the_analysis_to_the_slow_path() {
        let sources = [("a.py", "def a():\n    return 1\n")];
        let paths = vec![PathBuf::from("a.py")];
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::analysis("missing"))
        };
        let index = CodeIndex::build(Path::new(""), &paths, &read, &|_| Vec::new());
        let no_tree = |_: usize| Ok(None);
        assert!(
            findings_one_tree_at_a_time(&index, &no_tree, &Options::default())
                .unwrap()
                .is_none()
        );
        let (sinks, _, entries, kept) =
            findings_with_all_trees(&index, &no_tree, &Options::default()).unwrap();
        assert!(sinks.is_empty() && entries.is_empty() && kept.is_empty());
    }

    #[test]
    fn an_experimental_category_is_off_unless_the_options_turn_it_on() {
        let sources = [("a.py", "token = \"AKIAJ3KD92LQ7MZP5XWB\"\n")];
        let off = analyze_sources_with(&sources, &Options::default());
        assert!(off.sinks.is_empty());
        assert_eq!(off.coverage.experimental_off, ["hardcoded_credential"]);
        assert!(!off
            .coverage
            .sink_categories
            .contains(&"hardcoded_credential".to_string()));
        assert!(off.coverage.statement.contains("experimental"));

        let on = analyze_sources_with(
            &sources,
            &Options {
                experimental: true,
                ..Options::default()
            },
        );
        assert_eq!(on.sinks.len(), 1);
        assert!(on.coverage.experimental_off.is_empty());
        assert!(on
            .coverage
            .sink_categories
            .contains(&"hardcoded_credential".to_string()));
    }

    #[test]
    fn each_branch_of_a_diamond_is_on_a_shortest_path() {
        let source = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry():\n    left()\n    right()\n\ndef left():\n    leaf()\n\ndef right():\n    leaf()\n\ndef leaf():\n    os.system(\"x\" + os.environ[\"A\"])\n";
        let analysis = run(&[("app.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.reachability.paths.len(), 1);
        assert_eq!(analysis.reachability.paths[0].path_count, 2);
        for branch in ["app.py#left", "app.py#right"] {
            let security = crate::index::security::symbol_security(&analysis, branch);
            assert_eq!(security.reachable_from.len(), 1, "{branch}");
            assert_eq!(security.reaches.len(), 1, "{branch}");
        }
        let unrelated = crate::index::security::symbol_security(&analysis, "app.py#other");
        assert!(unrelated.reachable_from.is_empty());
    }

    #[test]
    fn a_sink_has_the_shortest_path_from_an_entry_point() {
        let analysis = run(&[("app.py", APP)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.entry_points.len(), 1);
        let path = &analysis.reachability.paths[0];
        let symbols: Vec<&str> = path.steps.iter().map(|step| step.symbol.as_str()).collect();
        assert_eq!(
            symbols,
            ["app.py#run_command", "app.py#helper", "app.py#deep"]
        );
        assert_eq!(path.steps[0].basis, None);
        assert!(path.steps[1].basis.is_some());
        assert_eq!(path.path_count, 1);
        assert_eq!(
            path.weakest_basis,
            path.steps[1..]
                .iter()
                .filter_map(|s| s.basis)
                .min_by_key(|b| b.rank())
        );
    }

    const CALLBACK: &str = "from fastapi import APIRouter\n\nrouter = APIRouter()\n\n\n@router.post(\"/test\")\nasync def test_code(request):\n    def load_module() -> None:\n        exec(request.code)\n\n    await call_off_loop(load_module, 5, label=\"load\")\n";

    #[test]
    fn a_nested_function_that_the_handler_gives_as_a_callback_has_a_path() {
        use crate::index::security::reach::EdgeKind;
        let analysis = run(&[("app.py", CALLBACK)], DEFAULT_MIN_BASIS);
        assert!(analysis.reachability.no_known_entry.is_empty());
        let path = &analysis.reachability.paths[0];
        let steps: Vec<(&str, EdgeKind, Option<ResolutionBasis>)> = path
            .steps
            .iter()
            .map(|step| (step.symbol.as_str(), step.edge, step.basis))
            .collect();
        assert_eq!(
            steps,
            [
                ("app.py#test_code", EdgeKind::Call, None),
                (
                    "app.py#test_code.load_module",
                    EdgeKind::FunctionReference,
                    Some(ResolutionBasis::LexicalScope)
                ),
            ]
        );
        assert_eq!(path.function_references, 1);
        assert_eq!(path.weakest_basis, Some(ResolutionBasis::LexicalScope));
        let json = serde_json::to_value(path).unwrap();
        assert_eq!(json["steps"][0].get("edge"), None);
        assert_eq!(json["steps"][1]["edge"], "function_reference");
        assert_eq!(json["function_references"], 1);
    }

    #[test]
    fn an_inline_handler_follows_a_function_reference_in_it() {
        let source = "const express = require('express');\nconst { exec } = require('child_process');\nconst app = express();\nfunction worker() { exec('x'); }\napp.post('/run', (req, res) => { setImmediate(worker); });\n";
        let analysis = run(&[("server.js", source)], DEFAULT_MIN_BASIS);
        use crate::index::security::reach::EdgeKind;
        let steps: Vec<Vec<(&str, EdgeKind)>> = analysis
            .reachability
            .paths
            .iter()
            .map(|path| {
                path.steps
                    .iter()
                    .map(|step| (step.symbol.as_str(), step.edge))
                    .collect()
            })
            .collect();
        assert_eq!(
            steps,
            [[
                ("server.js#<inline@5:17>", EdgeKind::Call),
                ("server.js#worker", EdgeKind::FunctionReference),
            ]]
        );
    }

    #[test]
    fn a_path_of_calls_only_has_no_function_reference_fields() {
        let analysis = run(&[("app.py", APP)], DEFAULT_MIN_BASIS);
        let json = serde_json::to_value(&analysis.reachability.paths[0]).unwrap();
        assert_eq!(json.get("function_references"), None);
        assert!(json["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.get("edge").is_none()));
    }

    #[test]
    fn a_call_is_the_edge_when_a_function_is_also_a_value() {
        use crate::index::security::reach::EdgeKind;
        let source = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry(pool):\n    pool.submit(leaf)\n    leaf()\n\ndef leaf():\n    os.system(\"x\" + os.environ[\"A\"])\n";
        let analysis = run(&[("app.py", source)], DEFAULT_MIN_BASIS);
        let path = &analysis.reachability.paths[0];
        assert_eq!(path.steps[1].edge, EdgeKind::Call);
        assert_eq!(path.function_references, 0);
        assert_eq!(path.path_count, 1);
    }

    #[test]
    fn a_function_reference_with_only_the_name_gives_no_path() {
        let files = [
            (
                "app.py",
                "from flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry(pool):\n    pool.submit(leaf)\n",
            ),
            (
                "other.py",
                "import os\n\ndef leaf():\n    os.system(\"x\" + os.environ[\"A\"])\n",
            ),
        ];
        assert!(run(&files, DEFAULT_MIN_BASIS).reachability.paths.is_empty());
        let lowered = run(&files, ResolutionBasis::NameOnly);
        assert!(lowered.reachability.paths.is_empty());
    }

    #[test]
    fn a_sink_that_no_entry_point_reaches_is_no_known_entry() {
        let analysis = run(&[("app.py", APP)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.reachability.no_known_entry.len(), 1);
        assert!(analysis.reachability.no_known_entry[0].contains("code_evaluation"));
    }

    #[test]
    fn the_symbols_on_each_path_to_a_sink_are_choke_points() {
        let analysis = run(&[("app.py", APP)], DEFAULT_MIN_BASIS);
        let chokes: Vec<(&str, usize, usize)> = analysis
            .reachability
            .choke_points
            .iter()
            .map(|point| (point.symbol.as_str(), point.entries, point.sinks))
            .collect();
        assert_eq!(
            chokes,
            [("app.py#helper", 1, 1), ("app.py#run_command", 1, 1)]
        );
    }

    #[test]
    fn two_entry_points_share_the_choke_point_below_them() {
        let source = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef first():\n    return helper()\n\n@app.get(\"/b\")\ndef second():\n    return helper()\n\ndef helper():\n    return deep()\n\ndef deep():\n    os.system(\"x\" + os.environ[\"A\"])\n";
        let analysis = run(&[("app.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.reachability.paths.len(), 2);
        let chokes: Vec<(&str, usize)> = analysis
            .reachability
            .choke_points
            .iter()
            .map(|point| (point.symbol.as_str(), point.entries))
            .collect();
        assert_eq!(chokes, [("app.py#helper", 2)]);
    }

    #[test]
    fn the_path_count_is_the_number_of_shortest_paths() {
        let source = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry():\n    left()\n    right()\n\ndef left():\n    sink()\n\ndef right():\n    sink()\n\ndef sink():\n    os.system(\"x\" + os.environ[\"A\"])\n";
        let analysis = run(&[("app.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.reachability.paths.len(), 1);
        assert_eq!(analysis.reachability.paths[0].path_count, 2);
        assert_eq!(analysis.reachability.paths[0].steps.len(), 3);
    }

    #[test]
    fn the_minimum_basis_removes_the_weaker_edges() {
        let analysis = run(&[("app.py", APP)], ResolutionBasis::Scip);
        assert!(analysis.reachability.paths.is_empty());
        assert_eq!(analysis.reachability.no_known_entry.len(), 2);
    }

    #[test]
    fn a_sink_in_test_code_is_not_in_the_reachability() {
        let analysis = run(
            &[(
                "tests/test_app.py",
                "import os\n\ndef test_it(command):\n    os.system(command)\n",
            )],
            DEFAULT_MIN_BASIS,
        );
        assert_eq!(analysis.sinks.len(), 1);
        assert!(analysis.reachability.no_known_entry.is_empty());
    }

    #[test]
    fn a_sink_in_generated_code_is_not_in_the_reachability() {
        let generated = "// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage main\n\nimport \"os/exec\"\n\nfunc run(program string) {\n\texec.Command(program)\n}\n";
        let analysis = run(&[("api.pb.go", generated)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.sinks.len(), 1);
        assert!(analysis.sinks[0].flags.in_generated);
        assert!(analysis.reachability.no_known_entry.is_empty());
        let written = generated.replace("// Code generated by protoc-gen-go. DO NOT EDIT.\n\n", "");
        let analysis = run(&[("api.go", written.as_str())], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.reachability.no_known_entry.len(), 1);
    }

    #[test]
    fn the_coverage_names_the_frameworks_and_the_categories_of_the_languages() {
        let analysis = run(&[("app.py", APP)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.coverage.languages, ["python"]);
        assert!(analysis.coverage.frameworks.contains(&"flask".to_string()));
        assert!(!analysis
            .coverage
            .frameworks
            .contains(&"express".to_string()));
        assert!(analysis
            .coverage
            .sink_categories
            .contains(&"command_execution".to_string()));
        assert!(analysis
            .coverage
            .statement
            .contains("does not mean that the code is safe"));
    }

    #[test]
    fn a_comment_with_a_reason_suppresses_the_sink_on_the_next_line_and_counts_it() {
        let source = "import os\n\ndef run(command):\n    # omen:allow command_execution: the command comes from the build script\n    os.system(command)\n";
        let analysis = run(&[("a.py", source)], DEFAULT_MIN_BASIS);
        assert!(analysis.sinks.is_empty());
        assert_eq!(analysis.suppressed.len(), 1);
        assert_eq!(
            analysis.suppressed[0].reason,
            "the command comes from the build script"
        );
        assert_eq!(analysis.suppressed[0].comment_line, 4);
        assert!(analysis.invalid_suppressions.is_empty());
    }

    #[test]
    fn a_comment_on_the_same_line_suppresses_the_sink() {
        let source = "import os\n\ndef run(command):\n    os.system(command)  # omen:allow command_execution: fixed input\n";
        let analysis = run(&[("a.py", source)], DEFAULT_MIN_BASIS);
        assert!(analysis.sinks.is_empty());
        assert_eq!(analysis.suppressed.len(), 1);
    }

    #[test]
    fn a_suppression_without_a_reason_does_not_suppress_and_is_reported() {
        let source = "import os\n\ndef run(command):\n    # omen:allow command_execution:\n    os.system(command)\n    # omen:allow\n    # omen:allow unknown_category: x\n";
        let analysis = run(&[("a.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.sinks.len(), 1);
        assert!(analysis.suppressed.is_empty());
        let problems: Vec<(u32, bool)> = analysis
            .invalid_suppressions
            .iter()
            .map(|invalid| (invalid.line, invalid.problem.contains("reason")))
            .collect();
        assert_eq!(problems, [(4, true), (6, true), (7, false)]);
    }

    #[test]
    fn a_suppression_of_one_category_does_not_hide_another_category() {
        let source = "import os\n\ndef run(command):\n    # omen:allow weak_hash: not a password\n    os.system(command)\n";
        let analysis = run(&[("a.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.sinks.len(), 1);
        assert!(analysis.suppressed.is_empty());
    }

    #[test]
    fn the_marker_in_a_string_is_not_a_suppression() {
        let source = "import os\n\ndef run(command):\n    text = \"omen:allow command_execution: x\"\n    os.system(command)\n    return text\n";
        let analysis = run(&[("a.py", source)], DEFAULT_MIN_BASIS);
        assert_eq!(analysis.sinks.len(), 1);
        assert!(analysis.invalid_suppressions.is_empty());
    }

    /// The steps of each path, with the line of its entry point and of its sink.
    fn path_steps(analysis: &Analysis) -> Vec<(u32, u32, Vec<String>)> {
        let line = |key: &str| -> u32 {
            key.split(':')
                .nth(1)
                .and_then(|line| line.parse().ok())
                .unwrap_or(0)
        };
        let mut found: Vec<(u32, u32, Vec<String>)> = analysis
            .reachability
            .paths
            .iter()
            .map(|path| {
                (
                    line(&path.entry),
                    line(&path.sink),
                    path.steps.iter().map(|step| step.symbol.clone()).collect(),
                )
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn an_inline_handler_starts_a_path_to_the_sinks_in_it_and_below_it() {
        let source = "import { Controller, Get, Post, UseGuards } from '@nestjs/common';\nimport express from 'express';\nimport { exec } from 'child_process';\nconst PATH = '/metrics';\n@Controller('cats')\nexport class CatsController {\n  @Get(':id') find() { return 1; }\n  @UseGuards(AuthGuard) @Post() run(cmd: string) { exec(cmd); }\n}\nexport class Server {\n  app = express();\n  setup() {\n    const app = this.app; app.get('/health', (_req, res) => res.send('ok'));\n    this.app.post(PATH, (req, res) => { exec(req.body.cmd); res.end(); });\n    this.app.put('/deep', (req, res) => deep(req.body));\n  }\n}\nfunction deep(value: string) {\n  exec(value);\n}\n";
        let analysis = run(&[("src/c.ts", source)], DEFAULT_MIN_BASIS);
        assert_eq!(
            path_steps(&analysis),
            [
                (8, 8, vec!["src/c.ts#CatsController.run".to_string()]),
                (
                    14,
                    14,
                    vec!["src/c.ts#Server.setup.<inline@14:24>".to_string()]
                ),
                (
                    15,
                    19,
                    vec![
                        "src/c.ts#Server.setup.<inline@15:26>".to_string(),
                        "src/c.ts#deep".to_string()
                    ]
                ),
            ]
        );
    }

    #[test]
    fn a_caller_of_the_function_around_an_inline_handler_reaches_its_sinks_through_it() {
        let source = "const express = require('express');\nconst { exec } = require('child_process');\nconst app = express();\napp.get('/boot', boot);\nfunction boot(req, res) {\n  setup();\n}\nfunction setup() {\n  app.post('/run', (req, res) => exec(req.body.cmd));\n}\n";
        let analysis = run(&[("server.js", source)], DEFAULT_MIN_BASIS);
        assert_eq!(
            path_steps(&analysis),
            [
                (
                    4,
                    9,
                    vec![
                        "server.js#boot".to_string(),
                        "server.js#setup".to_string(),
                        "server.js#setup.<inline@9:19>".to_string()
                    ]
                ),
                (9, 9, vec!["server.js#setup.<inline@9:19>".to_string()]),
            ]
        );
    }

    /// The steps of each path with the kind of the edge to each step.
    fn path_edges(analysis: &Analysis) -> Vec<Vec<(String, EdgeKind)>> {
        let mut found: Vec<Vec<(String, EdgeKind)>> = analysis
            .reachability
            .paths
            .iter()
            .map(|path| {
                path.steps
                    .iter()
                    .map(|step| (step.symbol.clone(), step.edge))
                    .collect()
            })
            .collect();
        found.sort();
        found
    }

    use crate::index::security::reach::EdgeKind;

    #[test]
    fn a_call_in_an_inline_handler_is_an_edge_of_the_handler_and_not_of_the_function_around_it() {
        let source = "const express = require('express');\nconst { exec } = require('child_process');\nconst app = express();\napp.get('/boot', boot);\nfunction boot(req, res) {\n  setup();\n}\nfunction setup() {\n  app.post('/run', (req, res) => deep(req.body.cmd));\n}\nfunction deep(command) {\n  exec(command);\n}\n";
        let analysis = run(&[("server.js", source)], DEFAULT_MIN_BASIS);
        let inline = "server.js#setup.<inline@9:19>".to_string();
        let deep = "server.js#deep".to_string();
        assert_eq!(
            path_edges(&analysis),
            [
                vec![
                    ("server.js#boot".to_string(), EdgeKind::Call),
                    ("server.js#setup".to_string(), EdgeKind::Call),
                    (inline.clone(), EdgeKind::FunctionReference),
                    (deep.clone(), EdgeKind::Call),
                ],
                vec![(inline, EdgeKind::Call), (deep, EdgeKind::Call)],
            ]
        );
    }

    #[test]
    fn an_inline_handler_that_registers_an_inline_handler_reaches_it() {
        let source = "const express = require('express');\nconst { exec } = require('child_process');\nconst app = express();\nfunction setup() {\n  app.get('/outer', (req, res) => {\n    app.post('/inner', (q, s) => exec(q.body.c));\n  });\n}\n";
        let analysis = run(&[("server.js", source)], DEFAULT_MIN_BASIS);
        let outer = "server.js#setup.<inline@5:20>".to_string();
        let inner = "server.js#setup.<inline@6:23>".to_string();
        assert_eq!(
            path_edges(&analysis),
            [
                vec![
                    (outer, EdgeKind::Call),
                    (inner.clone(), EdgeKind::FunctionReference)
                ],
                vec![(inner, EdgeKind::Call)],
            ]
        );
    }

    /// `depth` nested inline handlers that close at one byte, and `depth` bytes after it that
    /// only the outermost handler contains.
    fn innermost_after_a_deep_nest(depth: usize) -> std::time::Duration {
        use crate::index::model::Span;
        use crate::index::security::entry::{EntryBasis, EntryKind, InlineHandler};
        let entries: Vec<EntryPoint> = (0..depth)
            .map(|level| {
                let end = if level == 0 { 3 * depth } else { depth + 1 };
                let range = Span {
                    start_byte: level,
                    end_byte: end,
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                };
                EntryPoint {
                    key: format!("a.ts:{level}"),
                    file: "a.ts".to_string(),
                    kind: EntryKind::HttpHandler,
                    range,
                    symbol: None,
                    framework: "express".to_string(),
                    route: None,
                    guards: Vec::new(),
                    guards_complete: true,
                    guards_unknown_reason: None,
                    basis: EntryBasis::Registered,
                    in_test: false,
                    inline_handler: Some(InlineHandler {
                        range,
                        enclosing: None,
                    }),
                }
            })
            .collect();
        let regions = InlineRegions::new(&entries, |position, _| Some(position));
        let start = crate::index::timing::Stopwatch::start();
        for byte in depth + 1..2 * depth + 1 {
            assert_eq!(regions.innermost("a.ts", None, byte), Some(0));
        }
        assert_eq!(regions.innermost("a.ts", None, depth), Some(depth - 1));
        start.elapsed()
    }

    #[test]
    fn a_byte_after_a_nest_belongs_to_the_handler_around_the_nest() {
        innermost_after_a_deep_nest(10);
    }

    #[test]
    #[ignore = "timing test: run in release with --ignored"]
    fn timing_bytes_after_a_deep_nest_of_inline_handlers_resolve_in_linear_time() {
        crate::index::timing::assert_scales_linearly(
            "bytes after a nest",
            20_000,
            innermost_after_a_deep_nest,
        );
    }

    #[test]
    #[ignore = "timing test: run in release with --ignored"]
    fn timing_inline_handlers_resolve_in_linear_time() {
        let source = |size: usize| {
            let mut text = String::from(
                "const express = require('express');\nconst { exec } = require('child_process');\nconst app = express();\nfunction setup() {\n",
            );
            for route in 0..size {
                text.push_str(&format!(
                    "  app.post('/r{route}', (req, res) => {{ exec(req.body.c); res.end(); }});\n"
                ));
            }
            text.push_str("}\n");
            text
        };
        // The index of one large function is outside the measure. Only the steps of the inline
        // handlers are measured.
        crate::index::timing::assert_scales_linearly("inline handlers", 8_000, |size| {
            let text = source(size);
            let paths = [PathBuf::from("server.js")];
            let read = |_: &Path| Ok(text.as_bytes().to_vec());
            let index = CodeIndex::build(Path::new(""), &paths, &read, &|_| Vec::new());
            let analysis = run(&[("server.js", text.as_str())], DEFAULT_MIN_BASIS);
            assert_eq!(analysis.reachability.paths.len(), size);
            let start = crate::index::timing::Stopwatch::start();
            let view = CallView::from_index_with_function_values(
                &index,
                &inline_handlers(&index, &analysis.entry_points),
            );
            let found = reachability(
                &view,
                &analysis.sinks,
                &analysis.entry_points,
                DEFAULT_MIN_BASIS,
            );
            let took = start.elapsed();
            assert_eq!(found.paths.len(), size);
            took
        });
    }
}
