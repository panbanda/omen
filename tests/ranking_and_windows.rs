//! The order of each list that a command pages, the history window that each
//! history analyzer reports, and the rows that are test code.

#[path = "support/fixture.rs"]
mod fixture;

use std::path::Path;

use fixture::{cli_json, fixture_repository, mcp_responses, omen};
use serde_json::{json, Value};
use tempfile::TempDir;

fn numbers(rows: &Value, key: &str) -> Vec<f64> {
    rows.as_array()
        .expect("a list")
        .iter()
        .map(|row| row[key].as_f64().expect("a number"))
        .collect()
}

fn is_descending(values: &[f64]) -> bool {
    values.windows(2).all(|pair| pair[0] >= pair[1])
}

fn tool_call(tool: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    })
}

fn envelope(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no tool text in {response}"));
    serde_json::from_str(text).expect("the envelope is JSON")
}

#[test]
fn test_defect_files_are_in_the_order_of_the_probability() {
    let repository = fixture_repository();
    let all = cli_json(repository.path(), &["defect", "--since", "all"]);
    let probabilities = numbers(&all["files"], "probability");
    assert!(probabilities.len() > 2);
    assert!(is_descending(&probabilities), "{probabilities:?}");

    let top = cli_json(
        repository.path(),
        &["defect", "--since", "all", "--top", "2"],
    );
    assert_eq!(numbers(&top["files"], "probability"), probabilities[..2]);
    assert_eq!(
        top["files_omitted"].as_u64(),
        Some(probabilities.len() as u64 - 2)
    );
}

#[test]
fn test_defect_json_has_no_noise_of_a_32_bit_float() {
    let repository = fixture_repository();
    let output = omen(repository.path())
        .args(["defect", "--since", "all"])
        .output()
        .expect("omen starts");
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let weights = &serde_json::from_str::<Value>(&text).expect("JSON")["weights"];
    for (name, weight) in weights.as_object().expect("weights") {
        assert!(weight.to_string().len() <= 4, "{name}: {weight}");
    }
    for noise in ["0000000", "9999999"] {
        let line = text.lines().find(|line| line.contains(noise));
        assert_eq!(line, None, "a number has the noise of a 32-bit float");
    }
}

#[test]
fn test_ownership_files_are_in_the_order_of_the_concentration_and_then_of_the_size() {
    let repository = fixture_repository();
    let result = cli_json(repository.path(), &["ownership"]);
    let files = result["files"].as_array().expect("files");
    assert!(files.len() > 2);
    let keys: Vec<(f64, u64, &str)> = files
        .iter()
        .map(|file| {
            (
                file["concentration"].as_f64().expect("concentration"),
                file["total_lines"].as_u64().expect("lines"),
                file["file"].as_str().expect("file"),
            )
        })
        .collect();
    for pair in keys.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        let in_order = first.0 > second.0
            || (first.0 == second.0 && first.1 > second.1)
            || (first.0 == second.0 && first.1 == second.1 && first.2 < second.2);
        assert!(in_order, "{first:?} is before {second:?}");
    }
    assert_eq!(result["history_complete"], true);
    assert!(result.get("history_note").is_none());
}

#[test]
fn test_complexity_top_gives_the_most_complex_functions_and_only_their_files() {
    let repository = fixture_repository();
    let all = cli_json(repository.path(), &["complexity"]);
    let mut every: Vec<u64> = all["files"]
        .as_array()
        .expect("files")
        .iter()
        .flat_map(|file| file["functions"].as_array().expect("functions"))
        .map(|function| function["metrics"]["cyclomatic"].as_u64().expect("number"))
        .collect();
    every.sort_unstable_by(|left, right| right.cmp(left));
    let all_files = all["files"].as_array().expect("files").len();
    assert!(all_files > 2 && every.len() > 4);

    let top = cli_json(repository.path(), &["complexity", "--top", "3"]);
    let files = top["files"].as_array().expect("files");
    let kept: Vec<u64> = files
        .iter()
        .flat_map(|file| file["functions"].as_array().expect("functions"))
        .map(|function| function["metrics"]["cyclomatic"].as_u64().expect("number"))
        .collect();
    assert_eq!(
        kept,
        every[..3],
        "the three most complex functions, in order"
    );
    assert!(
        files
            .iter()
            .all(|file| !file["functions"].as_array().expect("functions").is_empty()),
        "a file without a function on the page is not in the list"
    );
    assert_eq!(
        top["functions_omitted"].as_u64(),
        Some(every.len() as u64 - 3)
    );
    assert_eq!(
        top["files_omitted"].as_u64(),
        Some((all_files - files.len()) as u64)
    );

    let next = cli_json(
        repository.path(),
        &["complexity", "--top", "3", "--offset", "3"],
    );
    let next_kept: Vec<u64> = next["files"]
        .as_array()
        .expect("files")
        .iter()
        .flat_map(|file| file["functions"].as_array().expect("functions"))
        .map(|function| function["metrics"]["cyclomatic"].as_u64().expect("number"))
        .collect();
    assert_eq!(next_kept, every[3..6.min(every.len())]);
}

#[test]
fn test_mcp_complexity_limit_gives_the_most_complex_functions() {
    let repository = fixture_repository();
    let responses = mcp_responses(
        repository.path(),
        &[tool_call("complexity", json!({"limit": 2}))],
    );
    let envelope = envelope(&responses[0]);
    let files = envelope["result"]["files"].as_array().expect("files");
    let kept: Vec<u64> = files
        .iter()
        .flat_map(|file| file["functions"].as_array().expect("functions"))
        .map(|function| function["metrics"]["cyclomatic"].as_u64().expect("number"))
        .collect();
    assert_eq!(kept.len(), 2);
    assert!(kept[0] >= kept[1]);
    assert!(files
        .iter()
        .all(|file| !file["functions"].as_array().expect("functions").is_empty()));
    assert_eq!(envelope["returned"], 2);
    assert_eq!(envelope["has_more"], true);

    let all = cli_json(repository.path(), &["complexity"]);
    let highest = all["files"]
        .as_array()
        .expect("files")
        .iter()
        .flat_map(|file| file["functions"].as_array().expect("functions"))
        .map(|function| function["metrics"]["cyclomatic"].as_u64().expect("number"))
        .max();
    assert_eq!(Some(kept[0]), highest);
}

#[test]
fn test_each_history_analyzer_reports_the_same_window_block() {
    let repository = fixture_repository();
    for (command, days) in [
        ("churn", 180),
        ("hotspot", 90),
        ("defect", 30),
        ("temporal", 30),
        ("changes", 30),
    ] {
        let result = cli_json(repository.path(), &[command]);
        let window = &result["churn_window"];
        assert_eq!(window["days"], days, "{command}");
        assert_eq!(
            window["anchor_date"], "2024-01-22T12:00:00+00:00",
            "{command}"
        );
        assert_eq!(window["history_complete"], true, "{command}");
        assert_eq!(window["commits_matched"], 4, "{command}");
        assert!(window.get("history_note").is_none(), "{command}");
    }
}

#[test]
fn test_window_of_all_the_history_has_no_number_of_days() {
    let repository = fixture_repository();
    for command in ["churn", "temporal"] {
        let result = cli_json(repository.path(), &[command, "--since", "all"]);
        assert!(result["period_days"].is_null(), "{command}");
        assert!(result["churn_window"]["days"].is_null(), "{command}");
        assert_eq!(result["churn_window"]["commits_matched"], 4, "{command}");
    }
}

#[test]
fn test_empty_result_of_a_narrow_window_gives_the_next_window() {
    let repository = fixture_repository();
    let narrow = cli_json(repository.path(), &["temporal", "--since", "1d"]);
    assert_eq!(narrow["couplings"], json!([]));
    assert_eq!(narrow["churn_window"]["commits_matched"], 1);
    assert_eq!(narrow["churn_window"]["next_since"], "90d");

    let responses = mcp_responses(
        repository.path(),
        &[tool_call("temporal", json!({"since": "1d"}))],
    );
    let envelope = envelope(&responses[0]);
    assert_eq!(envelope["result"]["churn_window"]["next_since"], "90d");
    let hints = envelope["hints"].as_array().expect("hints");
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(hints[0]["tool"], "temporal");
    assert_eq!(hints[0]["arguments"]["since"], "90d");
    assert!(hints[0]["reason"]
        .as_str()
        .expect("reason")
        .contains("1 commit"));

    let all = cli_json(repository.path(), &["temporal", "--since", "all"]);
    assert!(all["churn_window"].get("next_since").is_none());
}

#[test]
fn test_changes_accepts_since_on_the_cli_and_in_mcp() {
    let repository = fixture_repository();
    let narrow = cli_json(repository.path(), &["changes", "--since", "1d"]);
    assert_eq!(narrow["commits"].as_array().expect("commits").len(), 1);
    assert_eq!(narrow["period_days"], 1);
    let wide = cli_json(repository.path(), &["changes", "--since", "1y"]);
    assert_eq!(wide["commits"].as_array().expect("commits").len(), 4);
    assert_eq!(wide["churn_window"]["days"], 365);

    let responses = mcp_responses(
        repository.path(),
        &[
            tool_call("changes", json!({"since": "1d"})),
            tool_call("changes", json!({"since": "all"})),
        ],
    );
    let narrow = envelope(&responses[0]);
    assert_eq!(narrow["total_items"], 1);
    assert_eq!(narrow["result"]["churn_window"]["days"], 1);
    let all = envelope(&responses[1]);
    assert_eq!(all["error"]["code"], "invalid_argument", "{all}");
    assert_eq!(all["error"]["param"], "since");

    let rejected = omen(repository.path())
        .args(["changes", "--since", "all"])
        .output()
        .expect("omen starts");
    assert!(!rejected.status.success());
    let message = String::from_utf8_lossy(&rejected.stderr);
    assert!(message.contains("not a window of `changes`"), "{message}");
}

#[test]
fn test_changes_of_a_window_with_few_commits_has_absolute_levels() {
    let repository = fixture_repository();
    let narrow = cli_json(repository.path(), &["changes", "--since", "1d"]);
    let commit = &narrow["commits"][0];
    assert_eq!(narrow["risk_thresholds"]["basis"], "absolute");
    assert_eq!(narrow["risk_thresholds"]["high_threshold"], 0.6);
    assert_eq!(narrow["risk_thresholds"]["medium_threshold"], 0.3);
    let score = commit["risk_score"].as_f64().expect("score");
    let expected = if score >= 0.6 {
        "high"
    } else if score >= 0.3 {
        "medium"
    } else {
        "low"
    };
    assert_eq!(commit["risk_level"], expected, "score {score}");

    let scores = numbers(
        &cli_json(repository.path(), &["changes", "--since", "1y"])["commits"],
        "risk_score",
    );
    assert!(is_descending(&scores), "{scores:?}");
}

/// A clone of the fixture that has only the last `depth` commits.
fn shallow_clone(source: &Path, depth: &str) -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    let url = format!("file://{}", source.display());
    fixture::git(
        directory.path(),
        "ada",
        "2024-02-01T12:00:00Z",
        &["clone", "--depth", depth, &url, "."],
    );
    directory
}

#[test]
fn test_shallow_clone_has_a_note_and_no_owner_for_the_lines_before_the_boundary() {
    let source = fixture_repository();
    let shallow = shallow_clone(source.path(), "1");

    let churn = cli_json(shallow.path(), &["churn", "--since", "all"]);
    assert_eq!(churn["churn_window"]["history_complete"], false);
    assert!(churn["churn_window"]["history_note"]
        .as_str()
        .expect("note")
        .contains("shallow"));

    // Each line is as old as the boundary commit, or older. Git gives each
    // line to the author of that commit, who wrote one function.
    let ownership = cli_json(shallow.path(), &["ownership"]);
    assert_eq!(ownership["history_complete"], false);
    assert!(ownership["history_note"]
        .as_str()
        .expect("note")
        .contains("shallow"));
    assert_eq!(ownership["files"], json!([]));
    assert_eq!(ownership["summary"]["bus_factor"], 0);
    assert!(ownership["summary"]["files_without_history"].as_u64() > Some(3));
    assert!(ownership["summary"]["unattributed_lines"].as_u64() > Some(10));
}

#[test]
fn test_shallow_clone_gives_an_owner_only_to_the_lines_after_the_boundary() {
    let source = fixture_repository();
    let shallow = shallow_clone(source.path(), "2");
    let ownership = cli_json(shallow.path(), &["ownership"]);
    let files = ownership["files"].as_array().expect("files");
    // The last commit, by bob, adds a function to `other.rs`. The commit
    // before it is the boundary.
    let names: Vec<&str> = files
        .iter()
        .map(|file| file["file"].as_str().expect("file"))
        .collect();
    assert_eq!(names, ["other.rs"]);
    assert_eq!(files[0]["primary_owner"], "bob");
    assert_eq!(files[0]["contributors"].as_array().expect("list").len(), 1);
    assert!(files[0]["unattributed_lines"].as_u64() > Some(0));
    let attributed = files[0]["contributors"][0]["lines_owned"]
        .as_u64()
        .expect("lines");
    assert_eq!(
        attributed + files[0]["unattributed_lines"].as_u64().expect("lines"),
        files[0]["total_lines"].as_u64().expect("lines")
    );
}

#[test]
fn test_hotspot_row_names_the_values_that_it_ranks() {
    let repository = fixture_repository();
    let result = cli_json(repository.path(), &["hotspot", "--since", "all"]);
    let rows = result["hotspots"].as_array().expect("hotspots");
    assert!(!rows.is_empty());
    for row in rows {
        assert!(row["total_complexity"].as_u64().is_some(), "{row}");
        assert!(row["churn_score"].as_f64().is_some(), "{row}");
    }
    assert!(is_descending(&numbers(&result["hotspots"], "score")));
}

const LIBRARY_WITH_TESTS: &str = r#"use std::process::Command;

pub fn production(value: u32) -> u32 {
    if value > 3 {
        1
    } else {
        2
    }
}

pub fn run(program: &str) {
    // TODO: check the program name
    Command::new(program).status().ok();
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod checks {
    use super::*;

    #[test]
    /// The documentation is between the attribute and the function.
    fn branches() {
        // TODO: add the boundary value
        for value in 0..9 {
            if value > 3 && value < 7 {
                assert_eq!(production(value), 1);
            } else if value > 7 {
                assert_eq!(production(value), 1);
            }
        }
        Command::new("true").status().unwrap();
    }
}
"#;

/// A repository with production code and test code, and one commit.
fn repository_with_tests() -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path();
    let date = "2024-03-01T12:00:00Z";
    fixture::git(path, "ada", date, &["init", "--initial-branch", "main"]);
    for (file, text) in [
        ("src/lib.rs", LIBRARY_WITH_TESTS),
        (
            "tests/api.rs",
            "// TODO: add more cases\n#[test]\nfn runs() {\n    std::process::Command::new(\"true\").status().unwrap();\n}\n",
        ),
        ("src/other.rs", "pub fn other() -> u32 {\n    3\n}\n"),
    ] {
        let target = path.join(file);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("directory");
        std::fs::write(target, text).expect("write");
    }
    fixture::commit(path, "ada", date, "feat: add the library");
    directory
}

fn flags(rows: &Value, key: &str) -> Vec<(String, bool)> {
    let mut found: Vec<(String, bool)> = rows
        .as_array()
        .expect("a list")
        .iter()
        .map(|row| {
            (
                row[key].as_str().expect("a name").to_string(),
                row["is_test"].as_bool().expect("is_test"),
            )
        })
        .collect();
    found.sort();
    found
}

fn row(name: &str, is_test: bool) -> (String, bool) {
    (name.to_string(), is_test)
}

#[test]
fn test_each_file_row_has_is_test_and_exclude_tests_removes_the_test_rows() {
    let repository = repository_with_tests();
    let expected = [
        row("src/lib.rs", false),
        row("src/other.rs", false),
        row("tests/api.rs", true),
    ];
    for (command, list) in [
        (vec!["defect", "--since", "all"], "files"),
        (vec!["churn", "--since", "all"], "files"),
        (vec!["tdg"], "files"),
        (vec!["ownership"], "files"),
        (vec!["complexity"], "files"),
    ] {
        let result = cli_json(repository.path(), &command);
        assert_eq!(flags(&result[list], "file"), expected, "{command:?}");
        assert!(result.get("tests_excluded").is_none(), "{command:?}");

        let mut excluding = command.clone();
        excluding.push("--exclude-tests");
        let result = cli_json(repository.path(), &excluding);
        assert_eq!(flags(&result[list], "file"), expected[..2], "{excluding:?}");
        assert!(
            result["tests_excluded"].as_u64() >= Some(1),
            "{excluding:?}"
        );
    }
}

#[test]
fn test_function_and_comment_rows_have_is_test_by_the_syntax() {
    let repository = repository_with_tests();
    let complexity = cli_json(repository.path(), &["complexity"]);
    let library = complexity["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|file| file["file"] == "src/lib.rs")
        .expect("the library");
    assert_eq!(
        flags(&library["functions"], "name"),
        [
            row("branches", true),
            row("production", false),
            row("run", false)
        ]
    );

    // The most complex function is a test. Without the tests, it is not on the page.
    let top = cli_json(repository.path(), &["complexity", "--top", "1"]);
    assert_eq!(top["files"][0]["functions"][0]["name"], "branches");
    let top = cli_json(
        repository.path(),
        &["complexity", "--top", "1", "--exclude-tests"],
    );
    assert_eq!(top["files"][0]["functions"][0]["name"], "production");
    assert_eq!(top["tests_excluded"], 2);

    let satd = cli_json(repository.path(), &["satd"]);
    let mut lines: Vec<(String, u64, bool)> = satd["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| {
            (
                item["file"].as_str().expect("file").to_string(),
                item["line"].as_u64().expect("line"),
                item["is_test"].as_bool().expect("is_test"),
            )
        })
        .collect();
    lines.sort();
    assert_eq!(
        lines,
        [
            ("src/lib.rs".to_string(), 12, false),
            ("src/lib.rs".to_string(), 24, true),
            ("tests/api.rs".to_string(), 1, true),
        ]
    );
    let satd = cli_json(repository.path(), &["satd", "--exclude-tests"]);
    assert_eq!(satd["items"].as_array().expect("items").len(), 1);
    assert_eq!(satd["tests_excluded"], 2);
}

#[test]
fn test_security_exclude_tests_removes_the_sinks_in_test_code() {
    let repository = repository_with_tests();
    let all = cli_json(repository.path(), &["security", "sinks"]);
    let in_test: Vec<bool> = all["sinks"]
        .as_array()
        .expect("sinks")
        .iter()
        .map(|sink| sink["flags"]["in_test"].as_bool().expect("in_test"))
        .collect();
    let tests = in_test.iter().filter(|flag| **flag).count();
    assert_eq!(tests, 2, "{in_test:?}");
    assert_eq!(in_test.len(), 3);

    let production = cli_json(repository.path(), &["security", "sinks", "--exclude-tests"]);
    let sinks = production["sinks"].as_array().expect("sinks");
    assert_eq!(sinks.len(), 1);
    assert_eq!(sinks[0]["flags"]["in_test"], false);
    assert_eq!(production["tests_excluded"], 2);
}

#[test]
fn test_mcp_exclude_tests_gives_the_same_rows_as_the_cli() {
    let repository = repository_with_tests();
    let responses = mcp_responses(
        repository.path(),
        &[
            tool_call("defect", json!({"since": "all"})),
            tool_call("defect", json!({"since": "all", "exclude_tests": true})),
            tool_call(
                "security",
                json!({"section": "sinks", "exclude_tests": true}),
            ),
            tool_call("complexity", json!({"exclude_tests": true, "limit": 1})),
        ],
    );
    let all = envelope(&responses[0]);
    assert_eq!(all["total_items"], 3);
    assert_eq!(
        flags(&all["result"]["files"], "file")[2],
        row("tests/api.rs", true)
    );
    let production = envelope(&responses[1]);
    assert_eq!(production["total_items"], 2);
    assert_eq!(production["result"]["tests_excluded"], 1);
    let security = envelope(&responses[2]);
    assert_eq!(security["total_items"], 1);
    assert_eq!(security["result"]["tests_excluded"], 2);
    let complexity = envelope(&responses[3]);
    assert_eq!(
        complexity["result"]["files"][0]["functions"][0]["name"],
        "production"
    );
    assert_eq!(complexity["total_items"], 3);
}

#[test]
fn test_exclude_tests_is_an_error_for_a_command_without_a_list_of_files() {
    let repository = repository_with_tests();
    for command in ["changes", "flags"] {
        let output = omen(repository.path())
            .args([command, "--exclude-tests"])
            .output()
            .expect("omen starts");
        assert!(!output.status.success(), "{command}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(
            message.contains("`--exclude-tests` does not apply"),
            "{command}: {message}"
        );
    }
}

#[test]
fn test_all_gives_the_same_rows_as_each_single_command() {
    let repository = repository_with_tests();
    let section = |all: &Value, name: &str| -> Value {
        all["analyzers"]
            .as_array()
            .expect("analyzers")
            .iter()
            .find(|entry| entry["analyzer"] == name)
            .unwrap_or_else(|| panic!("no section {name}"))["result"]
            .clone()
    };
    let all = cli_json(repository.path(), &["all"]);
    assert_eq!(
        flags(&section(&all, "satd")["items"], "file"),
        [
            row("src/lib.rs", false),
            row("src/lib.rs", true),
            row("tests/api.rs", true)
        ]
    );
    let production = cli_json(repository.path(), &["all", "--exclude-tests"]);
    let satd = section(&production, "satd");
    assert_eq!(satd["items"].as_array().expect("items").len(), 1);
    assert_eq!(satd["tests_excluded"], 2);
    let security = section(&production, "security");
    assert_eq!(security["sinks"].as_array().expect("sinks").len(), 1);
    assert_eq!(security["tests_excluded"], 2);
    // An analyzer reads the full result of a different one: the score does not change.
    assert_eq!(
        section(&all, "score")["score"],
        section(&production, "score")["score"]
    );
}
