//! The rows of a report that are test code.
//!
//! Each row of a list of files or of symbols gets `is_test`. The rule is the
//! one of `crate::index::test_code`: the path of the file, and the syntax
//! when the row has a line. With `--exclude-tests`, the rows that are test
//! code are not in the report, and `tests_excluded` gives their number.

use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;
use serde_json::Value;

use crate::core::{AnalysisContext, Language};
use crate::index::test_code::{is_in_lines, is_test_path, test_lines_of_tree};

/// The key of the flag in a row.
pub const IS_TEST: &str = "is_test";

/// The key of the count that `--exclude-tests` adds to a report.
pub const TESTS_EXCLUDED: &str = "tests_excluded";

/// One place that a row names: the key of its file, and the key of its line.
type Place = (&'static str, Option<&'static str>);

/// One list of rows in the report of an analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowList {
    /// The key of the list at the top level of the report.
    pub list: &'static str,
    /// The places of a row. A row is test code when one of its places is.
    pub places: &'static [Place],
    /// The key of a list in each row whose elements are rows of the same
    /// file, with the key of their line. `complexity` has the functions of a
    /// file in such a list.
    pub children: Option<(&'static str, &'static str)>,
}

const fn files(list: &'static str) -> RowList {
    RowList {
        list,
        places: &[("file", None)],
        children: None,
    }
}

const FILES: [RowList; 1] = [files("files")];
const HOTSPOTS: [RowList; 1] = [files("hotspots")];
const NODES: [RowList; 1] = [files("nodes")];

/// The lists of the report of each analyzer that hold files or symbols.
///
/// `deadcode` and `security` set the flag themselves, because they read the
/// role of a symbol from the index. `flags` and `changes` list flags and
/// commits, which are not in one file.
pub fn row_lists(analyzer: &str) -> &'static [RowList] {
    match analyzer {
        "complexity" => &[RowList {
            list: "files",
            places: &[("file", None)],
            children: Some(("functions", "start_line")),
        }],
        "satd" => &[RowList {
            list: "items",
            places: &[("file", Some("line"))],
            children: None,
        }],
        "stubs" => &[RowList {
            list: "stubs",
            places: &[("file", Some("line"))],
            children: None,
        }],
        "deadcode" => &[RowList {
            list: "items",
            places: &[],
            children: None,
        }],
        "churn" | "defect" | "tdg" | "ownership" => &FILES,
        "hotspot" => &HOTSPOTS,
        "graph" => &NODES,
        "temporal" => &[RowList {
            list: "couplings",
            places: &[("file_a", None), ("file_b", None)],
            children: None,
        }],
        // `clones` is the name of the command, and `duplicates` is the name of the analyzer.
        "duplicates" | "clones" => &[RowList {
            list: "clones",
            places: &[
                ("file_a", Some("start_line_a")),
                ("file_b", Some("start_line_b")),
            ],
            children: None,
        }],
        "repomap" => &[RowList {
            list: "symbols",
            places: &[("file", Some("line"))],
            children: None,
        }],
        "cohesion" => &[RowList {
            list: "classes",
            places: &[("file", Some("start_line"))],
            children: None,
        }],
        "smells" => &[RowList {
            list: "components",
            places: &[("id", None)],
            children: None,
        }],
        _ => &[],
    }
}

/// A file, the answer for the whole file, and the line ranges of its test code.
type FileAnswer = (String, bool, Option<Vec<(u32, u32)>>);

/// The answer for each file that the rows name.
struct TestCode {
    /// The files that are test code as a whole.
    whole: HashMap<String, bool>,
    /// The line ranges of test code in the other files.
    lines: HashMap<String, Vec<(u32, u32)>>,
}

impl TestCode {
    fn is_test(&self, file: &str, line: Option<u32>) -> bool {
        if self.whole.get(file).copied().unwrap_or(false) {
            return true;
        }
        match (line, self.lines.get(file)) {
            (Some(line), Some(lines)) => is_in_lines(lines, line),
            _ => false,
        }
    }
}

fn places_of(row: &Value, places: &[Place]) -> Vec<(String, Option<u32>)> {
    places
        .iter()
        .filter_map(|(file_key, line_key)| {
            let file = row.get(*file_key)?.as_str()?.to_string();
            let line = line_key
                .and_then(|key| row.get(key)?.as_u64())
                .map(|line| line as u32);
            Some((file, line))
        })
        .collect()
}

/// The facts of each file that a row names. The shared index gives them when
/// the analyzer built it. Without the index, the path gives the answer for a
/// file, and one parse of the file gives the lines for a row with a line.
fn collect(value: &Value, lists: &[RowList], ctx: &AnalysisContext<'_>) -> TestCode {
    let mut files: HashMap<String, bool> = HashMap::new();
    for list in lists {
        let Some(rows) = value.get(list.list).and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            for (file, line) in places_of(row, list.places) {
                // A child that has the flag got it from the analyzer, which
                // had the tree of the file.
                let has_children = list.children.is_some_and(|(children, _)| {
                    row.get(children)
                        .and_then(Value::as_array)
                        .is_some_and(|children| {
                            children.iter().any(|child| child.get(IS_TEST).is_none())
                        })
                });
                *files.entry(file).or_insert(false) |= line.is_some() || has_children;
            }
        }
    }
    let index = ctx.code_index_if_built();
    let facts = |file: &str| {
        let index = index.as_ref()?;
        let position = index
            .files
            .binary_search_by(|facts| facts.file.as_str().cmp(file))
            .ok()?;
        Some(&index.files[position])
    };
    let answers: Vec<FileAnswer> = files
        .into_par_iter()
        .map(|(file, needs_lines)| {
            if let Some(facts) = facts(&file) {
                let lines = (needs_lines && !facts.test_file).then(|| facts.test_lines.clone());
                return (file, facts.test_file, lines);
            }
            let path = Path::new(&file);
            let Some(language) = Language::detect(path) else {
                return (file.clone(), crate::core::is_test_file(&file), None);
            };
            if is_test_path(&file, language) {
                return (file, true, None);
            }
            let lines = needs_lines
                .then(|| {
                    let content = ctx.read_file(path).ok()?;
                    let parsed = crate::parser::Parser::new()
                        .parse(&content, language, path)
                        .ok()?;
                    Some(test_lines_of_tree(parsed.root_node(), &content, language))
                })
                .flatten();
            (file, false, lines)
        })
        .collect();
    let mut code = TestCode {
        whole: HashMap::new(),
        lines: HashMap::new(),
    };
    for (file, whole, lines) in answers {
        if let Some(lines) = lines.filter(|lines| !lines.is_empty()) {
            code.lines.insert(file.clone(), lines);
        }
        code.whole.insert(file, whole);
    }
    code
}

/// Sets `is_test` on a row. A row that the analyzer gave the value `true` keeps it.
fn flag(row: &mut Value, is_test: bool) -> bool {
    let Some(map) = row.as_object_mut() else {
        return false;
    };
    let is_test = is_test || map.get(IS_TEST).and_then(Value::as_bool) == Some(true);
    map.insert(IS_TEST.to_string(), Value::Bool(is_test));
    is_test
}

/// Adds `is_test` to each row of the report of `analyzer`. With
/// `exclude_tests`, removes the rows that are test code and adds
/// `tests_excluded`.
///
/// The count is the number of removed rows of the list that the command
/// pages: the functions for `complexity`, and the rows for the other
/// analyzers. The summary of a report does not change: it is for all the
/// code that the analyzer read.
pub fn mark(value: &mut Value, analyzer: &str, ctx: &AnalysisContext<'_>, exclude_tests: bool) {
    let lists = row_lists(analyzer);
    if lists.is_empty() {
        return;
    }
    let code = collect(value, lists, ctx);
    let mut excluded = 0usize;
    for list in lists {
        let Some(rows) = value.get_mut(list.list).and_then(Value::as_array_mut) else {
            continue;
        };
        for row in rows.iter_mut() {
            let places = places_of(row, list.places);
            let row_is_test = places.iter().any(|(file, line)| code.is_test(file, *line));
            let row_is_test = flag(row, row_is_test);
            let Some((children_key, line_key)) = list.children else {
                continue;
            };
            let file = places.first().map(|(file, _)| file.as_str()).unwrap_or("");
            let Some(children) = row.get_mut(children_key).and_then(Value::as_array_mut) else {
                continue;
            };
            for child in children.iter_mut() {
                let line = child
                    .get(line_key)
                    .and_then(Value::as_u64)
                    .map(|line| line as u32);
                flag(child, row_is_test || code.is_test(file, line));
            }
            if exclude_tests {
                let before = children.len();
                children.retain(|child| child.get(IS_TEST) != Some(&Value::Bool(true)));
                excluded += before - children.len();
            }
        }
        if exclude_tests {
            let before = rows.len();
            rows.retain(|row| row.get(IS_TEST) != Some(&Value::Bool(true)));
            if list.children.is_none() {
                excluded += before - rows.len();
            }
        }
    }
    if exclude_tests {
        if let Some(map) = value.as_object_mut() {
            map.insert(TESTS_EXCLUDED.to_string(), Value::Number(excluded.into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::Config;
    use crate::core::FileSet;

    const LIBRARY: &str = "pub fn production() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn checks() {\n        assert_eq!(production(), 1);\n    }\n}\n";

    fn with_context<T>(run: impl FnOnce(&AnalysisContext<'_>) -> T) -> T {
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("src/lib.rs", LIBRARY),
            ("tests/api.rs", "#[test]\nfn runs() {}\n"),
            ("src/service.test.ts", "export const value = 1;\n"),
        ] {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        run(&ctx)
    }

    #[test]
    fn test_file_rows_have_the_flag_of_the_path() {
        let mut report = json!({
            "files": [
                {"file": "src/lib.rs", "probability": 0.5},
                {"file": "tests/api.rs", "probability": 0.4},
                {"file": "src/service.test.ts", "probability": 0.3},
                {"file": "README.md", "probability": 0.1}
            ],
            "summary": {"total_files": 4}
        });
        with_context(|ctx| mark(&mut report, "defect", ctx, false));
        let flags: Vec<bool> = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[IS_TEST].as_bool().unwrap())
            .collect();
        assert_eq!(flags, [false, true, true, false]);
        assert!(report.get(TESTS_EXCLUDED).is_none());
    }

    #[test]
    fn test_exclude_tests_removes_the_rows_and_gives_their_number() {
        let mut report = json!({
            "hotspots": [
                {"file": "tests/api.rs", "score": 0.9},
                {"file": "src/lib.rs", "score": 0.5}
            ],
            "summary": {"total_hotspots": 2}
        });
        with_context(|ctx| mark(&mut report, "hotspot", ctx, true));
        assert_eq!(
            report["hotspots"],
            json!([{"file": "src/lib.rs", "score": 0.5, "is_test": false}])
        );
        assert_eq!(report[TESTS_EXCLUDED], 1);
        // The summary is for all the code that the analyzer read.
        assert_eq!(report["summary"]["total_hotspots"], 2);
    }

    #[test]
    fn test_row_with_a_line_has_the_flag_of_the_syntax() {
        let mut report = json!({"items": [
            {"file": "src/lib.rs", "line": 1, "marker": "NOTE"},
            {"file": "src/lib.rs", "line": 11, "marker": "NOTE"},
            {"file": "tests/api.rs", "line": 1, "marker": "NOTE"}
        ]});
        with_context(|ctx| mark(&mut report, "satd", ctx, false));
        let flags: Vec<bool> = report["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[IS_TEST].as_bool().unwrap())
            .collect();
        assert_eq!(flags, [false, true, true]);
    }

    #[test]
    fn test_index_gives_the_same_flags_as_the_tree() {
        let report = json!({"symbols": [
            {"file": "src/lib.rs", "line": 1, "name": "production"},
            {"file": "src/lib.rs", "line": 10, "name": "checks"}
        ]});
        let mut from_tree = report.clone();
        let mut from_index = report;
        with_context(|ctx| {
            mark(&mut from_tree, "repomap", ctx, false);
            ctx.code_index().unwrap();
            mark(&mut from_index, "repomap", ctx, false);
        });
        assert_eq!(from_tree, from_index);
        assert_eq!(from_tree["symbols"][0][IS_TEST], false);
        assert_eq!(from_tree["symbols"][1][IS_TEST], true);
    }

    #[test]
    fn test_functions_of_a_file_have_the_flag_and_the_count_is_for_the_functions() {
        let report = json!({"files": [
            {"file": "src/lib.rs", "functions": [
                {"name": "production", "start_line": 1},
                {"name": "checks", "start_line": 10}
            ]},
            {"file": "tests/api.rs", "functions": [{"name": "runs", "start_line": 2}]}
        ]});
        let mut marked = report.clone();
        with_context(|ctx| mark(&mut marked, "complexity", ctx, false));
        assert_eq!(marked["files"][0][IS_TEST], false);
        assert_eq!(marked["files"][0]["functions"][0][IS_TEST], false);
        assert_eq!(marked["files"][0]["functions"][1][IS_TEST], true);
        assert_eq!(marked["files"][1][IS_TEST], true);
        assert_eq!(marked["files"][1]["functions"][0][IS_TEST], true);

        let mut excluded = report;
        with_context(|ctx| mark(&mut excluded, "complexity", ctx, true));
        assert_eq!(excluded[TESTS_EXCLUDED], 2);
        let files = excluded["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["functions"].as_array().unwrap().len(), 1);
        assert_eq!(files[0]["functions"][0]["name"], "production");
    }

    #[test]
    fn test_pair_is_test_code_when_one_of_its_files_is() {
        let mut report = json!({"couplings": [
            {"file_a": "src/lib.rs", "file_b": "tests/api.rs"},
            {"file_a": "src/lib.rs", "file_b": "src/main.rs"}
        ]});
        with_context(|ctx| mark(&mut report, "temporal", ctx, true));
        assert_eq!(report["couplings"].as_array().unwrap().len(), 1);
        assert_eq!(report["couplings"][0]["file_b"], "src/main.rs");
        assert_eq!(report[TESTS_EXCLUDED], 1);
    }

    #[test]
    fn test_flag_of_the_analyzer_and_the_flag_of_the_path_combine() {
        // The analyzer gave each function a flag, thus the file is not parsed
        // again: line 10 of `src/lib.rs` is test code in the file on disk.
        let mut report = json!({"files": [
            {"file": "tests/api.rs", "functions": [
                {"name": "runs", "start_line": 2, "is_test": false}
            ]},
            {"file": "src/lib.rs", "functions": [
                {"name": "from_the_analyzer", "start_line": 1, "is_test": true},
                {"name": "not_parsed_again", "start_line": 10, "is_test": false}
            ]}
        ]});
        with_context(|ctx| mark(&mut report, "complexity", ctx, false));
        assert_eq!(report["files"][0]["functions"][0][IS_TEST], true);
        assert_eq!(report["files"][1]["functions"][0][IS_TEST], true);
        assert_eq!(report["files"][1]["functions"][1][IS_TEST], false);
    }

    #[test]
    fn test_row_that_has_the_flag_keeps_it() {
        let mut report = json!({"items": [
            {"file": "src/lib.rs", "line": 1, "name": "a", "is_test": true},
            {"file": "src/lib.rs", "line": 2, "name": "b", "is_test": false}
        ]});
        with_context(|ctx| mark(&mut report, "deadcode", ctx, true));
        assert_eq!(report["items"].as_array().unwrap().len(), 1);
        assert_eq!(report["items"][0]["name"], "b");
        assert_eq!(report[TESTS_EXCLUDED], 1);
    }

    #[test]
    fn test_analyzer_without_a_list_of_files_does_not_change() {
        let report = json!({"commits": [{"commit_hash": "abc"}]});
        let mut marked = report.clone();
        with_context(|ctx| mark(&mut marked, "changes", ctx, true));
        assert_eq!(marked, report);
    }
}
