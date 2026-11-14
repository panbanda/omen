//! The security analysis of a set of files: the sinks, the entry points, and the paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::entry::{find_entry_points, EntryPoint};
use super::reach::{coverage, reachability, Coverage, Reachability};
use super::{find_sinks, Sink};
use crate::core::Language;
use crate::index::build::{list_directory, CodeIndex, ListDirectory, ReadFile};
use crate::index::extract::FileSymbols;
use crate::index::headers;
use crate::index::model::ResolutionBasis;
use crate::index::view::{CallView, DEFAULT_MIN_BASIS};
use crate::parser::ParseResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    pub sinks: Vec<Sink>,
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
    let sinks = find_sinks(&trees, &kept);
    let entry_points = find_entry_points(&trees, &kept);
    let view = CallView::from_index(&index);
    let reach = reachability(&view, &sinks, &entry_points, min_basis);
    let mut languages: Vec<Language> = Vec::new();
    for file in &kept {
        if !languages.contains(&file.language) {
            languages.push(file.language);
        }
    }
    Analysis {
        sinks,
        entry_points,
        reachability: reach,
        coverage: coverage(&languages),
    }
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
    use std::collections::HashMap;

    use super::*;

    fn run(sources: &[(&str, &str)], min_basis: ResolutionBasis) -> Analysis {
        let contents: HashMap<PathBuf, Vec<u8>> = sources
            .iter()
            .map(|(name, text)| (PathBuf::from(name), text.as_bytes().to_vec()))
            .collect();
        let paths: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();
        analyze(
            Path::new(""),
            &paths,
            &|path| {
                contents
                    .get(path)
                    .cloned()
                    .ok_or_else(|| crate::core::Error::analysis("missing"))
            },
            &|_| Vec::new(),
            min_basis,
        )
    }

    const APP: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\", methods=[\"POST\"])\ndef run_command():\n    return helper()\n\ndef helper():\n    return deep()\n\ndef deep():\n    os.system(\"x\" + os.environ[\"A\"])\n\ndef dead(expression):\n    eval(expression)\n";

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
}
