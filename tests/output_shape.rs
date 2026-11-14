//! Snapshot tests that record the output shape of each command and MCP tool.
//!
//! A shape is the tree of keys and value types. It does not contain values.
//! Thus paths, hashes, and dates do not appear in a snapshot.

use std::collections::{BTreeMap, BTreeSet};
#[path = "support/fixture.rs"]
mod fixture;

use fixture::{cli_json, fixture_repository, mcp_responses, omen};
use std::path::Path;

use clap::CommandFactory;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

/// The key that replaces all keys of an object that has data as its keys.
const DYNAMIC_KEY: &str = "<key>";

/// The key that holds the alternatives when one position has different types.
const ONE_OF_KEY: &str = "<one of>";

/// Converts a JSON value into its shape.
///
/// `dynamic_key_objects` lists the objects that have data as their keys. Each
/// entry is a path of keys from the root, such as `/result/files`. An array
/// does not add a segment to the path.
fn shape(value: &Value, dynamic_key_objects: &[&str]) -> Value {
    shape_at(value, "", dynamic_key_objects)
}

fn shape_at(value: &Value, path: &str, dynamic_key_objects: &[&str]) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("bool"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| shape_at(item, path, dynamic_key_objects))
                .reduce(merge_shapes)
                .into_iter()
                .collect(),
        ),
        Value::Object(entries) if dynamic_key_objects.contains(&path) => {
            let value_path = format!("{path}/{DYNAMIC_KEY}");
            let merged = entries
                .values()
                .map(|entry| shape_at(entry, &value_path, dynamic_key_objects))
                .reduce(merge_shapes);
            Value::Object(
                merged
                    .into_iter()
                    .map(|merged| (DYNAMIC_KEY.to_string(), merged))
                    .collect(),
            )
        }
        Value::Object(entries) => {
            let sorted: BTreeMap<String, Value> = entries
                .iter()
                .map(|(key, entry)| {
                    let entry_path = format!("{path}/{key}");
                    (
                        key.clone(),
                        shape_at(entry, &entry_path, dynamic_key_objects),
                    )
                })
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
    }
}

fn is_one_of(shape: &Value) -> bool {
    shape
        .as_object()
        .is_some_and(|entries| entries.len() == 1 && entries.contains_key(ONE_OF_KEY))
}

fn alternatives(shape: Value) -> Vec<Value> {
    if is_one_of(&shape) {
        shape[ONE_OF_KEY].as_array().cloned().unwrap_or_default()
    } else {
        vec![shape]
    }
}

fn merge_shapes(left: Value, right: Value) -> Value {
    if left == right {
        return left;
    }
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Value::Array(
            left.into_iter()
                .chain(right)
                .reduce(merge_shapes)
                .into_iter()
                .collect(),
        ),
        (Value::Object(left), Value::Object(right))
            if !left.contains_key(ONE_OF_KEY) && !right.contains_key(ONE_OF_KEY) =>
        {
            merge_object_shapes(left, right)
        }
        (left, right) => {
            let mut merged: Vec<Value> = Vec::new();
            for alternative in alternatives(left).into_iter().chain(alternatives(right)) {
                let same_kind = merged.iter().position(|existing| {
                    (existing.is_object() && alternative.is_object())
                        || (existing.is_array() && alternative.is_array())
                        || *existing == alternative
                });
                match same_kind {
                    Some(index) => {
                        let existing = merged.remove(index);
                        merged.push(merge_shapes(existing, alternative));
                    }
                    None => merged.push(alternative),
                }
            }
            merged.sort_by_key(Value::to_string);
            json!({ ONE_OF_KEY: merged })
        }
    }
}

/// Merges two object shapes. A key that one side does not have gets a `?`
/// suffix. The suffix stays when a later merge has the key on both sides.
fn merge_object_shapes(left: Map<String, Value>, right: Map<String, Value>) -> Value {
    fn split(entries: Map<String, Value>) -> BTreeMap<String, (bool, Value)> {
        entries
            .into_iter()
            .map(|(key, entry)| match key.strip_suffix('?') {
                Some(name) => (name.to_string(), (true, entry)),
                None => (key, (false, entry)),
            })
            .collect()
    }

    let mut left = split(left);
    let mut right = split(right);
    let names: BTreeSet<String> = left.keys().chain(right.keys()).cloned().collect();
    let mut merged = BTreeMap::new();
    for name in names {
        let (optional, entry) = match (left.remove(&name), right.remove(&name)) {
            (Some((left_optional, left)), Some((right_optional, right))) => {
                (left_optional || right_optional, merge_shapes(left, right))
            }
            (Some((_, entry)), None) | (None, Some((_, entry))) => (name != DYNAMIC_KEY, entry),
            (None, None) => continue,
        };
        let key = if optional { format!("{name}?") } else { name };
        merged.insert(key, entry);
    }
    Value::Object(merged.into_iter().collect())
}

#[test]
fn shape_names_the_type_of_each_scalar() {
    assert_eq!(shape(&json!(null), &[]), json!("null"));
    assert_eq!(shape(&json!(true), &[]), json!("bool"));
    assert_eq!(shape(&json!(1.5), &[]), json!("number"));
    assert_eq!(shape(&json!(7), &[]), json!("number"));
    assert_eq!(shape(&json!("text"), &[]), json!("string"));
}

#[test]
fn shape_keeps_the_keys_of_an_object_and_drops_the_values() {
    let value = json!({"path": "src/main.rs", "lines": 12, "inner": {"ok": false}});
    assert_eq!(
        shape(&value, &[]),
        json!({"inner": {"ok": "bool"}, "lines": "number", "path": "string"})
    );
}

#[test]
fn shape_sorts_the_keys_of_an_object() {
    let value = json!({"zebra": 1, "apple": 2, "mango": 3});
    let keys: Vec<String> = shape(&value, &[])
        .as_object()
        .expect("object shape")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["apple", "mango", "zebra"]);
}

#[test]
fn shape_of_an_array_is_one_merged_element_shape() {
    assert_eq!(shape(&json!([1, 2, 3]), &[]), json!(["number"]));
    assert_eq!(shape(&json!([]), &[]), json!([]));
    assert_eq!(
        shape(&json!([{"name": "a"}, {"name": "b"}]), &[]),
        json!([{"name": "string"}])
    );
}

#[test]
fn shape_marks_a_key_that_some_array_elements_do_not_have() {
    let value = json!([{"name": "a", "line": 1}, {"name": "b"}, {"name": "c", "line": 3}]);
    assert_eq!(
        shape(&value, &[]),
        json!([{"line?": "number", "name": "string"}])
    );
}

#[test]
fn shape_lists_the_alternatives_when_array_elements_have_different_types() {
    assert_eq!(
        shape(&json!(["a", null, 3, "b"]), &[]),
        json!([{ONE_OF_KEY: ["null", "number", "string"]}])
    );
    assert_eq!(
        shape(&json!([{"value": null}, {"value": {"x": 1}}]), &[]),
        json!([{"value": {ONE_OF_KEY: ["null", {"x": "number"}]}}])
    );
}

#[test]
fn shape_merges_nested_arrays_and_ignores_empty_ones() {
    let value = json!([{"items": []}, {"items": [{"id": 1}]}, {"items": [{"id": 2, "tag": "t"}]}]);
    assert_eq!(
        shape(&value, &[]),
        json!([{"items": [{"id": "number", "tag?": "string"}]}])
    );
}

#[test]
fn shape_replaces_data_keys_with_a_placeholder() {
    let value = json!({
        "files": {"src/a.rs": {"lines": 1}, "src/b.rs": {"lines": 2, "note": "n"}},
        "total": 2
    });
    assert_eq!(
        shape(&value, &["/files"]),
        json!({
            "files": {DYNAMIC_KEY: {"lines": "number", "note?": "string"}},
            "total": "number"
        })
    );
}

#[test]
fn shape_finds_a_data_key_object_below_an_array() {
    let value = json!({"groups": [{"by_path": {"a": 1}}, {"by_path": {"b": 2, "c": 3}}]});
    assert_eq!(
        shape(&value, &["/groups/by_path"]),
        json!({"groups": [{"by_path": {DYNAMIC_KEY: "number"}}]})
    );
}

/// One CLI command that the test runs with `-f json`.
struct CliCase {
    snapshot: &'static str,
    arguments: &'static [&'static str],
    dynamic_key_objects: &'static [&'static str],
}

const fn cli(snapshot: &'static str, arguments: &'static [&'static str]) -> CliCase {
    cli_keyed(snapshot, arguments, &[])
}

/// A case whose output has objects with data as their keys.
const fn cli_keyed(
    snapshot: &'static str,
    arguments: &'static [&'static str],
    dynamic_key_objects: &'static [&'static str],
) -> CliCase {
    CliCase {
        snapshot,
        arguments,
        dynamic_key_objects,
    }
}

/// `all` holds the result of each analyzer, thus it has all their data keys.
/// The paths apply after `with_analyzers_by_name`.
const ALL_DYNAMIC_KEY_OBJECTS: &[&str] = &[
    "/analyzers/churn/summary/author_contributions",
    "/analyzers/deadcode/summary/by_kind",
    "/analyzers/flags/summary/by_provider",
    "/analyzers/satd/by_category",
    "/analyzers/stubs/by_category",
    "/analyzers/tdg/grade_distribution",
    "/analyzers/tdg/language_distribution",
];

const CLI_CASES: &[CliCase] = &[
    cli("complexity", &["complexity"]),
    cli_keyed("satd", &["satd"], &["/by_category"]),
    cli_keyed("stubs", &["stubs"], &["/by_category"]),
    cli_keyed("deadcode", &["deadcode"], &["/summary/by_kind"]),
    cli_keyed(
        "churn",
        &["churn", "--since", "all"],
        &["/summary/author_contributions"],
    ),
    cli("clones", &["clones"]),
    cli("defect", &["defect"]),
    cli("changes", &["changes"]),
    cli("diff", &["diff", "--target", "main"]),
    cli_keyed(
        "tdg",
        &["tdg"],
        &["/grade_distribution", "/language_distribution"],
    ),
    cli("graph", &["graph"]),
    cli("hotspot", &["hotspot"]),
    cli("temporal", &["temporal"]),
    cli("ownership", &["ownership"]),
    cli("cohesion", &["cohesion"]),
    cli("repomap", &["repomap"]),
    cli("smells", &["smells"]),
    // No provider is on by default, and without one the output has no flags.
    cli_keyed(
        "flags",
        &["flags", "--provider", "unleash"],
        &["/summary/by_provider"],
    ),
    cli("score", &["score"]),
    cli("score_trend", &["score", "trend"]),
    cli_keyed("all", &["all"], ALL_DYNAMIC_KEY_OBJECTS),
    cli("context", &["context"]),
    cli("outline", &["outline"]),
    cli("impact", &["impact", "run"]),
    cli("symbol", &["symbol", "run"]),
    cli("security", &["security"]),
    cli("search_query", &["search", "query", "helper"]),
    cli("mcp_manifest", &["mcp", "manifest"]),
    cli_keyed(
        "eval",
        &[
            "eval",
            "--registration",
            EVAL_REGISTRATION,
            "--runs",
            EVAL_RUNS,
        ],
        &["/per_language_success_delta"],
    ),
];

const EVAL_REGISTRATION: &str = ".omen/eval-registration.json";
const EVAL_RUNS: &str = ".omen/eval-runs.json";

/// The top-level commands that have no snapshot, each with the reason.
const CLI_SKIPPED: &[(&str, &str)] = &[
    ("mutation", "It runs the test suite of the repository."),
    (
        "report",
        "It writes HTML and data files, not JSON to stdout.",
    ),
    ("benchmark", "It needs two omen binaries and a corpus."),
];

/// One MCP tool call. `arguments` is a JSON object. The paths in
/// `dynamic_key_objects` start at the envelope, not at the JSON-RPC response.
struct McpCase {
    tool: &'static str,
    arguments: &'static str,
    dynamic_key_objects: &'static [&'static str],
}

const fn mcp(tool: &'static str, arguments: &'static str) -> McpCase {
    mcp_keyed(tool, arguments, &[])
}

/// A case whose envelope has objects with data as their keys.
const fn mcp_keyed(
    tool: &'static str,
    arguments: &'static str,
    dynamic_key_objects: &'static [&'static str],
) -> McpCase {
    McpCase {
        tool,
        arguments,
        dynamic_key_objects,
    }
}

/// The path from the JSON-RPC response to the envelope of a tool result.
const ENVELOPE_PATH: &str = "/result/content/text";

const MCP_CASES: &[McpCase] = &[
    mcp("context", "{}"),
    mcp("outline", "{}"),
    mcp("complexity", "{}"),
    mcp_keyed("satd", "{}", &["/result/by_category"]),
    mcp_keyed("stubs", "{}", &["/result/by_category"]),
    mcp_keyed("deadcode", "{}", &["/result/summary/by_kind"]),
    mcp_keyed(
        "churn",
        r#"{"days": 36500}"#,
        &["/result/summary/author_contributions"],
    ),
    mcp("clones", "{}"),
    mcp("defect", r#"{"days": 36500}"#),
    mcp("changes", "{}"),
    mcp("diff", r#"{"base": "main"}"#),
    mcp_keyed(
        "tdg",
        "{}",
        &[
            "/result/grade_distribution",
            "/result/language_distribution",
        ],
    ),
    mcp("graph", "{}"),
    mcp("hotspot", r#"{"days": 36500}"#),
    mcp("temporal", r#"{"days": 36500}"#),
    mcp("ownership", "{}"),
    mcp("cohesion", "{}"),
    mcp("repomap", "{}"),
    mcp("smells", "{}"),
    mcp_keyed(
        "flags",
        r#"{"provider": "unleash"}"#,
        &["/result/summary/by_provider"],
    ),
    mcp("score", "{}"),
    mcp("search", r#"{"query": "helper"}"#),
    mcp("get_symbol", r#"{"name": "run"}"#),
    mcp("task_context", r#"{"name": "run"}"#),
    mcp("impact", r#"{"symbol": "run"}"#),
    mcp("security", "{}"),
];

/// The MCP tools that have no snapshot, each with the reason.
const MCP_SKIPPED: &[(&str, &str)] = &[];

/// One MCP tool call that gives an error. The snapshot holds the values of
/// the error, not only its shape, because the values are the contract.
struct McpErrorCase {
    snapshot: &'static str,
    /// The value of `error.code` that the call must give.
    code: &'static str,
    tool: &'static str,
    arguments: &'static str,
}

const fn mcp_error(
    snapshot: &'static str,
    code: &'static str,
    tool: &'static str,
    arguments: &'static str,
) -> McpErrorCase {
    McpErrorCase {
        snapshot,
        code,
        tool,
        arguments,
    }
}

/// One case for each error code that the fixture can cause.
const MCP_ERROR_CASES: &[McpErrorCase] = &[
    mcp_error("missing_argument", "missing_argument", "impact", "{}"),
    mcp_error(
        "invalid_type",
        "invalid_argument",
        "churn",
        r#"{"days": "30"}"#,
    ),
    mcp_error(
        "invalid_enum",
        "invalid_argument",
        "impact",
        r#"{"symbol": "run", "direction": "caller"}"#,
    ),
    mcp_error(
        "unknown_argument",
        "unknown_argument",
        "complexity",
        r#"{"pth": "."}"#,
    ),
    mcp_error(
        "many_problems",
        "missing_argument",
        "impact",
        r#"{"depth": "2", "symbl": "run"}"#,
    ),
    mcp_error(
        "not_found",
        "not_found",
        "get_symbol",
        r#"{"name": "helpr"}"#,
    ),
    // The fixture has `helper` in `util.rs` and in `other.rs`.
    mcp_error(
        "ambiguous",
        "ambiguous",
        "get_symbol",
        r#"{"name": "helper"}"#,
    ),
    mcp_error(
        "path_outside_root",
        "path_outside_root",
        "complexity",
        r#"{"path": "/"}"#,
    ),
];

/// The error codes that have no snapshot, each with the reason.
const MCP_ERROR_CODES_SKIPPED: &[(&str, &str)] = &[
    (
        "git_unavailable",
        "The fixture is a git repository. A unit test in `src/mcp/mod.rs` has this code.",
    ),
    (
        "internal",
        "No call on the fixture causes a failure of the server.",
    ),
];

/// and each baseline fails. With fewer pairs the gate fails and omen exits 2.
fn write_eval_records(repository: &Path) {
    let languages = ["rust", "go", "ruby", "typescript"];
    let cases: Vec<Value> = (0..30)
        .map(|index| {
            json!({
                "id": index.to_string(),
                "repository": format!("repo-{}", index % 5),
                "revision": "b".repeat(40),
                "language": languages[index % 4],
            })
        })
        .collect();
    let registration = json!({
        "version": 1,
        "split": "held_out",
        "development_repositories": [],
        "model_snapshot": "fixed-test-snapshot",
        "prompt_sha256": "a".repeat(64),
        "token_budget": 2000,
        "cases": cases,
    });
    let hash = omen::eval::digest(&registration);
    let mut runs = Vec::new();
    for case in &cases {
        for variant in ["baseline", "candidate"] {
            runs.push(json!({
                "repository": case["repository"],
                "revision": case["revision"],
                "language": case["language"],
                "case_id": case["id"],
                "variant": variant,
                "registration_sha256": hash,
                "model_snapshot": "fixed-test-snapshot",
                "prompt_sha256": "a".repeat(64),
                "token_budget": 2000,
                "seed": 0,
                "patch_sha256": "c".repeat(64),
                "test_log_sha256": "d".repeat(64),
                "tool_snapshot_sha256": "e".repeat(64),
                "status": "completed",
                "tests_pass": variant == "candidate",
                "patch_applies": true,
                "input_tokens": 500,
                "output_tokens": 200,
                "elapsed_ms": 1000,
                "cost_usd": 0.01,
                "unrelated_edits": 0,
            }));
        }
    }
    std::fs::write(repository.join(EVAL_REGISTRATION), registration.to_string())
        .expect("write registration");
    std::fs::write(repository.join(EVAL_RUNS), Value::Array(runs).to_string()).expect("write runs");
}

/// Changes the `analyzers` array of the `all` output into an object.
///
/// The output of `all` is `{"analyzers": [{"analyzer": name, "result": ...}]}`.
/// One merged element shape cannot show which analyzer has which keys. The
/// object has one entry for each analyzer: the name, then the result.
fn with_analyzers_by_name(mut output: Value) -> Value {
    let entries = output["analyzers"]
        .as_array()
        .expect("analyzers array")
        .iter()
        .map(|entry| {
            let keys: Vec<&String> = entry.as_object().expect("entry object").keys().collect();
            assert_eq!(keys, ["analyzer", "result"], "keys of an `all` entry");
            let name = entry["analyzer"].as_str().expect("analyzer name");
            (name.to_string(), entry["result"].clone())
        })
        .collect();
    output["analyzers"] = Value::Object(entries);
    output
}

#[test]
fn with_analyzers_by_name_makes_one_entry_for_each_analyzer() {
    let output = json!({"analyzers": [
        {"analyzer": "satd", "result": {"items": []}},
        {"analyzer": "graph", "result": {"nodes": [1]}}
    ]});
    assert_eq!(
        with_analyzers_by_name(output),
        json!({"analyzers": {"graph": {"nodes": [1]}, "satd": {"items": []}}})
    );
}

#[test]
fn cli_table_has_each_command() {
    let covered: BTreeSet<&str> = CLI_CASES
        .iter()
        .map(|case| case.arguments[0])
        .chain(CLI_SKIPPED.iter().map(|(command, _)| *command))
        .collect();
    let command = omen::cli::Cli::command();
    let missing: Vec<&str> = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name())
        .filter(|name| !covered.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "add a snapshot or a skip reason for: {missing:?}"
    );
}

#[test]
fn mcp_table_has_each_tool() {
    let covered: BTreeSet<&str> = MCP_CASES
        .iter()
        .map(|case| case.tool)
        .chain(MCP_SKIPPED.iter().map(|(tool, _)| *tool))
        .collect();
    let missing: Vec<&str> = omen::mcp::McpServer::tool_names()
        .iter()
        .copied()
        .filter(|tool| !covered.contains(tool))
        .collect();
    assert!(
        missing.is_empty(),
        "add a snapshot or a skip reason for: {missing:?}"
    );
}

#[test]
fn cli_json_shapes() {
    let directory = fixture_repository();
    let repository = directory.path();
    // `search query` reads the index that `search index` writes.
    let index = omen(repository)
        .args(["search", "index"])
        .output()
        .expect("omen starts");
    assert!(index.status.success(), "search index failed");
    // `search index` made the `.omen` directory.
    write_eval_records(repository);

    for case in CLI_CASES {
        let mut output = cli_json(repository, case.arguments);
        if case.snapshot == "all" {
            output = with_analyzers_by_name(output);
        }
        insta::assert_json_snapshot!(
            format!("cli_{}", case.snapshot),
            shape(&output, case.dynamic_key_objects)
        );
    }
}

#[test]
fn mcp_tools_list() {
    let directory = TempDir::new().expect("temporary directory");
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let responses = mcp_responses(directory.path(), &[request]);
    insta::assert_json_snapshot!("mcp_tools_list", responses[0]);
}

#[test]
fn mcp_tool_result_shapes() {
    let directory = fixture_repository();
    let requests: Vec<Value> = MCP_CASES
        .iter()
        .enumerate()
        .map(|(id, case)| {
            let arguments: Value = serde_json::from_str(case.arguments).expect("JSON arguments");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": case.tool, "arguments": arguments}
            })
        })
        .collect();
    let responses = mcp_responses(directory.path(), &requests);

    for (id, (case, mut response)) in MCP_CASES.iter().zip(responses).enumerate() {
        assert_eq!(
            response["id"],
            json!(id),
            "responses keep the request sequence"
        );
        assert!(
            response["result"]["isError"].is_null(),
            "tool {} failed: {response}",
            case.tool
        );
        // The envelope is JSON text in a string. Parse it, or the snapshot
        // records only `string`.
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool {} gave no text", case.tool));
        let envelope: Value = serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("tool {} gave no JSON: {error}", case.tool));
        response["result"]["content"][0]["text"] = envelope;
        let dynamic_key_objects: Vec<String> = case
            .dynamic_key_objects
            .iter()
            .map(|path| format!("{ENVELOPE_PATH}{path}"))
            .collect();
        let dynamic_key_objects: Vec<&str> =
            dynamic_key_objects.iter().map(String::as_str).collect();
        insta::assert_json_snapshot!(
            format!("mcp_{}", case.tool),
            shape(&response, &dynamic_key_objects)
        );
    }
}

#[test]
fn mcp_error_table_has_each_error_code() {
    let covered: BTreeSet<&str> = MCP_ERROR_CASES
        .iter()
        .map(|case| case.code)
        .chain(MCP_ERROR_CODES_SKIPPED.iter().map(|(code, _)| *code))
        .collect();
    let codes = [
        "missing_argument",
        "invalid_argument",
        "unknown_argument",
        "not_found",
        "ambiguous",
        "path_outside_root",
        "git_unavailable",
        "internal",
    ];
    assert_eq!(covered, codes.into_iter().collect::<BTreeSet<&str>>());
}

/// Replaces the path of the temporary repository in each string of the value.
fn without_repository_path(value: &mut Value, repository: &str) {
    match value {
        Value::String(text) => *text = text.replace(repository, "<root>"),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| without_repository_path(item, repository)),
        Value::Object(entries) => entries
            .values_mut()
            .for_each(|entry| without_repository_path(entry, repository)),
        _ => {}
    }
}

#[test]
fn without_repository_path_changes_each_string_in_the_value() {
    let mut value = json!({"message": "in /tmp/x now", "list": ["/tmp/x/a", 3]});
    without_repository_path(&mut value, "/tmp/x");
    assert_eq!(
        value,
        json!({"message": "in <root> now", "list": ["<root>/a", 3]})
    );
}

#[test]
fn mcp_tool_errors() {
    let directory = fixture_repository();
    let requests: Vec<Value> = MCP_ERROR_CASES
        .iter()
        .enumerate()
        .map(|(id, case)| {
            let arguments: Value = serde_json::from_str(case.arguments).expect("JSON arguments");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": case.tool, "arguments": arguments}
            })
        })
        .collect();
    let responses = mcp_responses(directory.path(), &requests);
    let repository = directory
        .path()
        .canonicalize()
        .expect("repository path")
        .to_string_lossy()
        .into_owned();

    for (case, response) in MCP_ERROR_CASES.iter().zip(responses) {
        assert!(
            response.get("error").is_none(),
            "{}: a tool error is not a JSON-RPC error: {response}",
            case.snapshot
        );
        let result = &response["result"];
        assert_eq!(result["isError"], json!(true), "{}", case.snapshot);
        let text = result["content"][0]["text"].as_str().expect("error text");
        let mut body: Value = serde_json::from_str(text).expect("JSON error text");
        assert_eq!(
            body, result["structuredContent"],
            "{}: the text and the structured content agree",
            case.snapshot
        );
        assert_eq!(body["error"]["code"], json!(case.code), "{}", case.snapshot);
        without_repository_path(&mut body, &repository);
        insta::assert_json_snapshot!(format!("mcp_error_{}", case.snapshot), body);
    }
}
