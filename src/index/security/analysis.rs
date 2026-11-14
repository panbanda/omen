//! The security analysis of a set of files: the sinks, the entry points, and the paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::entry::{find_entry_points_with, EntryPoint, EntryRule};
use super::reach::{coverage, reachability, Coverage, Reachability};
use super::spec::SinkSpec;
use super::suppress::{self, InvalidSuppression, Suppressed};
use super::{find_sinks_with, Sink};
use crate::core::Language;
use crate::index::build::{list_directory, CodeIndex, ListDirectory, ReadFile};
use crate::index::extract::FileSymbols;
use crate::index::headers;
use crate::index::model::ResolutionBasis;
use crate::index::view::{CallView, DEFAULT_MIN_BASIS};
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
) -> Analysis {
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
) -> Analysis {
    let by_relative: BTreeMap<String, &PathBuf> = files
        .iter()
        .map(|path| (relative(root, path), path))
        .collect();
    let parsed: Vec<Option<ParseResult>> = index
        .files
        .par_iter()
        .map(|file| {
            let path = by_relative.get(&file.file)?;
            let content = read(path).ok()?;
            headers::parse(path, file.language, &content)
        })
        .collect();
    let mut kept: Vec<FileSymbols> = Vec::new();
    let mut trees: Vec<ParseResult> = Vec::new();
    for (file, tree) in index.files.iter().zip(parsed) {
        if let Some(tree) = tree {
            kept.push(file.clone());
            trees.push(tree);
        }
    }
    let mut all_sinks = find_sinks_with(&trees, &kept, options.sink_rows);
    if !options.experimental {
        all_sinks.retain(|sink| !sink.category.is_experimental());
    }
    let names: Vec<String> = kept.iter().map(|file| file.file.clone()).collect();
    let (sinks, suppressed, invalid_suppressions) = suppress::apply(all_sinks, &trees, &names);
    let entry_points = find_entry_points_with(&trees, &kept, &options.entry_rules);
    let view = CallView::from_index(index);
    let reach = reachability(&view, &sinks, &entry_points, options.min_basis);
    let mut languages: Vec<Language> = Vec::new();
    for file in &kept {
        if !languages.contains(&file.language) {
            languages.push(file.language);
        }
    }
    Analysis {
        sinks,
        suppressed,
        invalid_suppressions,
        entry_points,
        reachability: reach,
        coverage: coverage(&languages, options.experimental),
    }
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
}

/// Analyzes files of the file system with the default minimum basis.
pub fn analyze_files(root: &Path, files: &[PathBuf]) -> Analysis {
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
}
