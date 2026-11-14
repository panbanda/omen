//! Tests that the CLI and the MCP server give the same answer.
//!
//! The behavioural test compares the JSON of each CLI command with the result
//! of the related MCP tool. The structural test compares the parameters of
//! each tool with the flags of the related command.

#[path = "support/fixture.rs"]
mod fixture;

use std::collections::BTreeSet;
use std::path::Path;

use clap::CommandFactory;
use fixture::{cli_json, fixture_repository, mcp_responses, omen};
use serde_json::{json, Value};

/// One MCP tool and the CLI command that gives the same data.
struct Mirror {
    tool: &'static str,
    /// The command and its positional arguments.
    command: &'static [&'static str],
    /// The arguments of the tool that are equal to the positional arguments.
    arguments: &'static str,
    /// True when the test gives `--top 50` to the command. That is the
    /// default `limit` of a tool. False when the command has no `--top`
    /// flag, or when its default is the default of the tool.
    paged: bool,
    /// One option that the two sides have: the CLI flags and the arguments.
    option: Option<(&'static [&'static str], &'static str)>,
}

const fn mirror(tool: &'static str, command: &'static [&'static str]) -> Mirror {
    Mirror {
        tool,
        command,
        arguments: "{}",
        paged: true,
        option: None,
    }
}

impl Mirror {
    const fn arguments(mut self, arguments: &'static str) -> Self {
        self.arguments = arguments;
        self
    }

    const fn not_paged(mut self) -> Self {
        self.paged = false;
        self
    }

    const fn option(mut self, flags: &'static [&'static str], arguments: &'static str) -> Self {
        self.option = Some((flags, arguments));
        self
    }
}

const WIDE: &[&str] = &["--days", "36500"];
const WIDE_ARGUMENTS: &str = r#"{"days": 36500}"#;

const MIRRORS: &[Mirror] = &[
    mirror("context", &["context"])
        .not_paged()
        .option(&["--max-tokens", "500"], r#"{"max_tokens": 500}"#),
    mirror("outline", &["outline"]).option(&["--file", "main.rs"], r#"{"file": "main.rs"}"#),
    mirror("complexity", &["complexity"]),
    mirror("satd", &["satd"]),
    mirror("deadcode", &["deadcode"]),
    mirror("stubs", &["stubs"]),
    mirror("churn", &["churn"]).option(WIDE, WIDE_ARGUMENTS),
    mirror("clones", &["clones"]).option(&["--include-tests"], r#"{"include_tests": true}"#),
    mirror("defect", &["defect"]).option(WIDE, WIDE_ARGUMENTS),
    mirror("changes", &["changes"]),
    mirror("diff", &["diff"]).option(&["--target", "main"], r#"{"base": "main"}"#),
    mirror("tdg", &["tdg"]),
    mirror("graph", &["graph"]),
    mirror("hotspot", &["hotspot"]).option(WIDE, WIDE_ARGUMENTS),
    mirror("temporal", &["temporal"]),
    mirror("ownership", &["ownership"]),
    mirror("cohesion", &["cohesion"]),
    mirror("repomap", &["repomap"]),
    mirror("smells", &["smells"]),
    mirror("flags", &["flags"]).option(
        &["--provider", "unleash", "--stale-days", "7"],
        r#"{"provider": "unleash", "stale_days": 7}"#,
    ),
    mirror("score", &["score"]).not_paged(),
    mirror("security", &["security"]),
    // `--top` of `search query` is `top_k`, and the two have the default 10.
    mirror("search", &["search", "query", "helper"])
        .not_paged()
        .arguments(r#"{"query": "helper"}"#)
        .option(&["--min-score", "0"], r#"{"min_score": 0}"#),
    // `same_file` removes the imported callees of `run`.
    mirror("get_symbol", &["symbol", "run"])
        .arguments(r#"{"name": "run"}"#)
        .option(
            &["--max-source-lines", "2", "--min-basis", "same_file"],
            r#"{"max_source_lines": 2, "min_basis": "same_file"}"#,
        ),
    mirror("impact", &["impact", "run"])
        .arguments(r#"{"symbol": "run"}"#)
        .option(
            &["--depth", "1", "--min-basis", "same_file"],
            r#"{"depth": 1, "min_basis": "same_file"}"#,
        ),
];

/// The tools that have no CLI command, each with the reason.
const TOOLS_WITHOUT_COMMAND: &[(&str, &str)] = &[(
    "task_context",
    "The CLI has no command for bounded context.",
)];

/// A part of the output that is permitted to be different, with the reason.
/// The path is a JSON pointer in the output. The segment `*` is each element
/// of an array or each entry of an object.
struct Exemption {
    tool: &'static str,
    path: &'static str,
    reason: &'static str,
}

const fn exempt(tool: &'static str, path: &'static str, reason: &'static str) -> Exemption {
    Exemption { tool, path, reason }
}

const EXEMPTIONS: &[Exemption] = &[
    exempt(
        "*",
        "/generated_at",
        "The two sides run at different times.",
    ),
    exempt(
        "churn",
        "/repository_root",
        "The MCP server makes the path canonical for its path check. The CLI keeps the path that the user gave.",
    ),
];

/// A configuration in which each value is different from its default.
const CONFIGURATION: &str = r#"[churn]
since = "50y"

[duplicates]
min_tokens = 10
min_similarity = 0.8

[hotspot]
since = "36000d"

[defect]
since = "36000d"

[changes]
since = "36000d"

[feature_flags]
providers = ["unleash"]
stale_days = 7

[satd]
markers = ["SHORTCUT"]
"#;

fn remove_path(value: &mut Value, segments: &[&str]) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    match value {
        Value::Object(entries) if rest.is_empty() && *first != "*" => {
            entries.remove(*first);
        }
        Value::Object(entries) if *first == "*" => entries
            .values_mut()
            .for_each(|entry| remove_path(entry, rest)),
        Value::Object(entries) => {
            if let Some(entry) = entries.get_mut(*first) {
                remove_path(entry, rest);
            }
        }
        Value::Array(items) if *first == "*" => {
            items.iter_mut().for_each(|item| remove_path(item, rest));
        }
        _ => {}
    }
}

fn without_exemptions(tool: &str, mut value: Value) -> Value {
    for exemption in EXEMPTIONS
        .iter()
        .filter(|exemption| exemption.tool == "*" || exemption.tool == tool)
    {
        let segments: Vec<&str> = exemption.path.split('/').skip(1).collect();
        remove_path(&mut value, &segments);
    }
    value
}

/// Collects the JSON pointers at which the two values are different.
fn differences(path: &str, cli: &Value, mcp: &Value, found: &mut Vec<String>) {
    match (cli, mcp) {
        (Value::Object(cli), Value::Object(mcp)) => {
            let keys: BTreeSet<&String> = cli.keys().chain(mcp.keys()).collect();
            for key in keys {
                let child = format!("{path}/{key}");
                match (cli.get(key), mcp.get(key)) {
                    (Some(cli), Some(mcp)) => differences(&child, cli, mcp, found),
                    (Some(_), None) => found.push(format!("{child}: only in the CLI output")),
                    (None, Some(_)) => found.push(format!("{child}: only in the MCP result")),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(cli), Value::Array(mcp)) if cli.len() == mcp.len() => {
            for (index, (cli, mcp)) in cli.iter().zip(mcp).enumerate() {
                differences(&format!("{path}/{index}"), cli, mcp, found);
            }
        }
        (Value::Array(cli), Value::Array(mcp)) => found.push(format!(
            "{path}: {} items in the CLI output, {} in the MCP result",
            cli.len(),
            mcp.len()
        )),
        (cli, mcp) if cli == mcp => {}
        (cli, mcp) => found.push(format!("{path}: CLI {cli}, MCP {mcp}")),
    }
}

#[test]
fn remove_path_removes_a_key_below_arrays_and_objects() {
    let mut value = json!({"a": 1, "items": [{"t": 1, "k": 2}, {"t": 3}], "by": {"x": {"t": 4}}});
    remove_path(&mut value, &["a"]);
    remove_path(&mut value, &["items", "*", "t"]);
    remove_path(&mut value, &["by", "*", "t"]);
    remove_path(&mut value, &["absent", "t"]);
    assert_eq!(value, json!({"items": [{"k": 2}, {}], "by": {"x": {}}}));
}

#[test]
fn differences_names_each_place_that_is_not_equal() {
    let mut found = Vec::new();
    differences(
        "",
        &json!({"a": 1, "b": [1, 2], "c": {"d": true}, "e": 1}),
        &json!({"a": 2, "b": [1], "c": {"d": true}, "f": 1}),
        &mut found,
    );
    assert_eq!(
        found,
        [
            "/a: CLI 1, MCP 2",
            "/b: 2 items in the CLI output, 1 in the MCP result",
            "/e: only in the CLI output",
            "/f: only in the MCP result"
        ]
    );
}

#[test]
fn each_tool_is_a_mirror_or_has_a_reason() {
    let covered: BTreeSet<&str> = MIRRORS
        .iter()
        .map(|mirror| mirror.tool)
        .chain(TOOLS_WITHOUT_COMMAND.iter().map(|(tool, _)| *tool))
        .collect();
    let tools: BTreeSet<&str> = omen::mcp::McpServer::tool_names().iter().copied().collect();
    assert_eq!(covered, tools);
    let with_list: BTreeSet<&str> = PRIMARY_LISTS.iter().map(|(tool, _)| *tool).collect();
    let mirrored: BTreeSet<&str> = MIRRORS.iter().map(|mirror| mirror.tool).collect();
    assert_eq!(with_list, mirrored, "each mirror has a primary list");
    assert_eq!(incorrect_exemptions(&tools), Vec::<&str>::new());
}

/// The paths of the exemptions that name no tool, have no pointer, or have no
/// reason.
fn incorrect_exemptions(tools: &BTreeSet<&str>) -> Vec<&'static str> {
    EXEMPTIONS
        .iter()
        .filter(|exemption| {
            let known_tool = exemption.tool == "*" || tools.contains(exemption.tool);
            !known_tool || !exemption.path.starts_with('/') || exemption.reason.is_empty()
        })
        .map(|exemption| exemption.path)
        .collect()
}

fn merged(base: &str, extra: &str) -> Value {
    let mut arguments: Value = serde_json::from_str(base).expect("JSON arguments");
    let extra: Value = serde_json::from_str(extra).expect("JSON arguments");
    arguments
        .as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    arguments
}

/// Runs each mirror on the two sides and gives the differences.
fn compare_all(repository: &Path, label: &str) -> Vec<String> {
    // `search query` reads the index that `search index` writes.
    let index = omen(repository)
        .args(["search", "index"])
        .output()
        .expect("omen starts");
    assert!(index.status.success(), "search index failed");

    // (name of the call, tool, CLI arguments, MCP arguments)
    let mut calls: Vec<(String, &Mirror, Vec<&str>, Value)> = Vec::new();
    for mirror in MIRRORS {
        let mut variants = vec![("no arguments", &[][..], "{}")];
        if let Some((flags, arguments)) = mirror.option {
            variants.push(("one option", flags, arguments));
        }
        for (variant, flags, arguments) in variants {
            let mut cli: Vec<&str> = mirror.command.to_vec();
            cli.extend(flags);
            if mirror.paged {
                cli.extend(["--top", "50"]);
            }
            calls.push((
                format!("{label}, {}, {variant}", mirror.tool),
                mirror,
                cli,
                merged(mirror.arguments, arguments),
            ));
        }
    }

    let requests: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(id, (_, mirror, _, arguments))| {
            json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": mirror.tool, "arguments": arguments}
            })
        })
        .collect();
    let responses = mcp_responses(repository, &requests);

    let mut found = Vec::new();
    for ((name, mirror, cli, _), response) in calls.iter().zip(responses) {
        let result = &response["result"];
        if result["isError"] == json!(true) {
            found.push(format!("{name}: the MCP tool gave an error: {result}"));
            continue;
        }
        let text = result["content"][0]["text"].as_str().expect("text");
        let envelope: Value = serde_json::from_str(text).expect("JSON envelope");
        let mcp = without_exemptions(mirror.tool, envelope["result"].clone());
        let cli = without_exemptions(mirror.tool, cli_json(repository, cli));
        let mut places = Vec::new();
        differences("", &cli, &mcp, &mut places);
        found.extend(places.into_iter().map(|place| format!("{name}: {place}")));
        // Two empty lists are equal, but they compare nothing.
        if name.ends_with("no arguments") {
            if let Some(problem) = empty_list_problem(label, mirror.tool, &cli, &mcp) {
                found.push(format!("{name}: {problem}"));
            }
        }
    }
    found
}

/// The primary list of each mirrored tool, as a JSON pointer in the result.
const PRIMARY_LISTS: &[(&str, &str)] = &[
    ("context", "/top_symbols"),
    ("outline", "/files"),
    ("complexity", "/files"),
    ("satd", "/items"),
    ("deadcode", "/items"),
    ("stubs", "/stubs"),
    ("churn", "/files"),
    ("clones", "/clones"),
    ("defect", "/files"),
    ("changes", "/commits"),
    ("diff", "/recommendations"),
    ("tdg", "/files"),
    ("graph", "/nodes"),
    ("hotspot", "/hotspots"),
    ("temporal", "/couplings"),
    ("ownership", "/files"),
    ("cohesion", "/classes"),
    ("repomap", "/symbols"),
    ("smells", "/smells"),
    ("flags", "/flags"),
    ("score", "/components"),
    ("search", "/results"),
    ("security", "/sinks"),
    ("get_symbol", "/callees"),
    ("impact", "/callers"),
];

/// The tools whose primary list is permitted to be empty on the fixture,
/// each with the configuration label and the reason.
const ALLOWED_EMPTY: &[((&str, &str), &str)] = &[
    (
        ("defaults", "flags"),
        "No flag provider is on by default. The configuration test and the option of `flags` give items.",
    ),
    (
        ("defaults", "security"),
        "The fixture has no sink, and the snapshots of other commands read the fixture. The file tests/security_cli.rs gives sinks to the command.",
    ),
    (
        ("configuration", "security"),
        "The fixture has no sink, and the snapshots of other commands read the fixture. The file tests/security_cli.rs gives sinks to the command.",
    ),
];

/// Gives a problem when the primary list of the tool is empty on the two
/// sides and the table does not permit that.
fn empty_list_problem(label: &str, tool: &str, cli: &Value, mcp: &Value) -> Option<String> {
    let (_, pointer) = PRIMARY_LISTS.iter().find(|(name, _)| *name == tool)?;
    let is_empty = |value: &Value| match value.pointer(pointer) {
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(entries)) => entries.is_empty(),
        _ => true,
    };
    let allowed = ALLOWED_EMPTY
        .iter()
        .any(|((allowed_label, allowed_tool), _)| *allowed_label == label && *allowed_tool == tool);
    match (is_empty(cli) && is_empty(mcp), allowed) {
        (true, false) => Some(format!(
            "the list `{pointer}` is empty on the two sides. Make the fixture give items, \
             or add the tool to ALLOWED_EMPTY with a reason"
        )),
        (false, true) => Some(format!(
            "the list `{pointer}` has items, thus remove the tool from ALLOWED_EMPTY"
        )),
        _ => None,
    }
}

fn assert_no_differences(found: Vec<String>) {
    assert!(
        found.is_empty(),
        "{} differences between the CLI and the MCP server. Correct each one, or add it to \
         EXEMPTIONS with a reason:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn cli_and_mcp_give_the_same_result_with_the_default_configuration() {
    let directory = fixture_repository();
    assert_no_differences(compare_all(directory.path(), "defaults"));
}

#[test]
fn cli_and_mcp_give_the_same_result_with_a_configuration_file() {
    let directory = fixture_repository();
    std::fs::write(directory.path().join("omen.toml"), CONFIGURATION).expect("write configuration");
    assert_no_differences(compare_all(directory.path(), "configuration"));
}

/// One call with a limit that cuts the paged list on the fixture.
struct CutCase {
    tool: &'static str,
    command: &'static [&'static str],
    arguments: &'static str,
    /// True when the limit cuts the paged list.
    cuts: bool,
}

/// `run` has one caller and nine callees. `helper` in `util.rs` has three
/// callers. The limit cuts only the paged list: `callers` of `get_symbol`,
/// and the levels of `impact`.
const CUT_CASES: &[CutCase] = &[
    CutCase {
        tool: "get_symbol",
        command: &["symbol", "run", "--top", "1"],
        arguments: r#"{"name": "run", "limit": 1}"#,
        cuts: false,
    },
    CutCase {
        tool: "get_symbol",
        command: &["symbol", "util.rs:helper", "--top", "1", "--offset", "1"],
        arguments: r#"{"name": "util.rs:helper", "limit": 1, "offset": 1}"#,
        cuts: true,
    },
    CutCase {
        tool: "impact",
        command: &["impact", "run", "--top", "1", "--offset", "1"],
        arguments: r#"{"symbol": "run", "limit": 1, "offset": 1}"#,
        cuts: true,
    },
];

/// The CLI tells the number of omitted items in `<list>_omitted`. The MCP
/// envelope tells it in `truncated`. The other content must be equal.
#[test]
fn a_limit_cuts_the_same_lists_on_the_two_sides() {
    let directory = fixture_repository();
    let repository = directory.path();
    let requests: Vec<Value> = CUT_CASES
        .iter()
        .enumerate()
        .map(|(id, case)| {
            let arguments: Value = serde_json::from_str(case.arguments).expect("arguments");
            json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": case.tool, "arguments": arguments}
            })
        })
        .collect();
    let responses = mcp_responses(repository, &requests);
    let mut found = Vec::new();
    for (case, response) in CUT_CASES.iter().zip(responses) {
        let name = case.command.join(" ");
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        let envelope: Value = serde_json::from_str(text).expect("JSON envelope");
        let cut = envelope["truncated"]
            .as_array()
            .is_some_and(|cuts| !cuts.is_empty());
        if cut != case.cuts {
            found.push(format!(
                "{name}: the limit cuts a list: {cut}, expected: {}",
                case.cuts
            ));
        }
        let mut cli = cli_json(repository, case.command);
        if let Some(map) = cli.as_object_mut() {
            map.retain(|key, _| !key.ends_with("_omitted"));
        }
        let mut places = Vec::new();
        differences("", &cli, &envelope["result"], &mut places);
        found.extend(places.into_iter().map(|place| format!("{name}: {place}")));
    }
    assert_no_differences(found);
}

/// One tool parameter and the CLI flag that has the same effect. The flag is
/// the identifier of the clap argument.
const PARAMETER_FLAGS: &[((&str, &str), &str)] = &[
    (("context", "max_tokens"), "max_tokens"),
    (("outline", "file"), "file"),
    (("churn", "days"), "days"),
    (("clones", "include_tests"), "include_tests"),
    (("defect", "days"), "days"),
    (("diff", "base"), "target"),
    (("hotspot", "days"), "days"),
    (("flags", "provider"), "provider"),
    (("flags", "stale_days"), "stale_days"),
    (("search", "query"), "query"),
    (("search", "top_k"), "top"),
    (("search", "min_score"), "min_score"),
    (("search", "files"), "files"),
    (("search", "include_projects"), "include_project"),
    (("get_symbol", "name"), "name"),
    // The flag is the opposite of the parameter.
    (("get_symbol", "include_source"), "no_source"),
    (("get_symbol", "max_source_lines"), "max_source_lines"),
    (("get_symbol", "min_basis"), "min_basis"),
    (("impact", "symbol"), "symbol"),
    (("impact", "depth"), "depth"),
    (("impact", "direction"), "direction"),
    (("impact", "min_basis"), "min_basis"),
    (("security", "section"), "section"),
    (("security", "entry"), "entry"),
    (("security", "sink"), "sink"),
    (("security", "category"), "category"),
    (("security", "min_basis"), "min_basis"),
];

/// The parameters that each paged tool has, with the flag of the command.
const COMMON_PARAMETER_FLAGS: &[(&str, &str)] =
    &[("path", "path"), ("limit", "top"), ("offset", "offset")];

/// The tool parameters that have no CLI flag, each with the reason.
const MCP_ONLY: &[((&str, &str), &str)] = &[
    (("context", "max_symbols"), NO_FLAG_FIXED_VALUE),
    (("context", "max_risks"), NO_FLAG_FIXED_VALUE),
    (
        ("context", "format"),
        "The CLI uses the global `--format` flag.",
    ),
    (("context", "compact"), TABLE_ENCODING),
    (
        ("context", "limit"),
        "The `context` command has no `--top` flag.",
    ),
    (
        ("context", "offset"),
        "The `context` command has no `--offset` flag.",
    ),
    (("clones", "min_tokens"), CONFIGURATION_ONLY),
    (("clones", "similarity"), CONFIGURATION_ONLY),
    (
        ("diff", "head"),
        "An alias of `base` that the tool keeps for old clients.",
    ),
    (("temporal", "days"), NO_FLAG_FIXED_VALUE),
    (("repomap", "max_symbols"), NO_FLAG_FIXED_VALUE),
    (("repomap", "compact"), TABLE_ENCODING),
    (
        ("score", "limit"),
        "The `score` command has no `--top` flag.",
    ),
    (
        ("score", "offset"),
        "The `score` command has no `--offset` flag.",
    ),
    (
        ("search", "max_complexity"),
        "The `search query` command has no complexity filter.",
    ),
    (
        ("get_symbol", "start_line"),
        "The `symbol` command selects the first definition and has no line selector.",
    ),
    (("get_symbol", "compact"), TABLE_ENCODING),
    (("impact", "compact"), TABLE_ENCODING),
];

const NO_FLAG_FIXED_VALUE: &str =
    "The command has no flag for this value. It uses the default of the tool.";
const CONFIGURATION_ONLY: &str =
    "The command reads this value only from the configuration, which is the default of the tool.";
const TABLE_ENCODING: &str =
    "The table encoding is only for MCP results. The CLI flag `--compact` has a different meaning.";
const FILE_FILTER: &str = "The tools have no file filter. They read each file below `path`.";
const GATE: &str = "A tool gives data and does not apply a gate.";
const PULL_REQUEST_COMMENT: &str =
    "Only the Markdown comment of the CLI uses the flag. A tool gives JSON and does not write a comment.";
const SINCE: &str = "The tool accepts the window only as `days`.";
const PROCESS: &str = "The flag controls the CLI process, not the analysis.";

/// The CLI flags that have no tool parameter, each with the reason. The
/// command `*` is each mirrored command.
const CLI_ONLY: &[((&str, &str), &str)] = &[
    (
        ("*", "format"),
        "A tool always gives JSON. `context` has its own `format` parameter.",
    ),
    (("*", "compact"), "A tool always gives JSON on one line."),
    (
        ("*", "config"),
        "The server reads the configuration one time, when it starts.",
    ),
    (("*", "verbose"), PROCESS),
    (("*", "jobs"), PROCESS),
    (
        ("*", "git_ref"),
        "The server analyzes the working tree only.",
    ),
    (
        ("*", "shallow"),
        "The flag applies to a clone of a remote repository, which the server does not make.",
    ),
    (("*", "glob"), FILE_FILTER),
    (("*", "exclude"), FILE_FILTER),
    (("*", "changed_since"), FILE_FILTER),
    (("diff", "max_bytes"), PULL_REQUEST_COMMENT),
    (("diff", "max_nodes"), PULL_REQUEST_COMMENT),
    (("complexity", "gate"), GATE),
    (("complexity", "max_cyclomatic"), GATE),
    (("complexity", "max_cognitive"), GATE),
    (("stubs", "gate"), GATE),
    (("stubs", "gate_severity"), GATE),
    (("security", "gate"), GATE),
    (("security", "target"), GATE),
    (("security", "gate_category"), GATE),
    (("security", "gate_basis"), GATE),
    (("score", "gate"), GATE),
    (("score", "fail_under"), GATE),
    (
        ("deadcode", "cargo_check"),
        "The flag runs build scripts, thus the server does not offer it.",
    ),
    (
        ("search query", "path"),
        "The search tools read the index of the MCP root and have no `path` parameter.",
    ),
    (("churn", "since"), SINCE),
    (("defect", "since"), SINCE),
    (("hotspot", "since"), SINCE),
];

/// The identifiers of the arguments of a command, with the global arguments.
fn flags_of(path: &[&str]) -> BTreeSet<String> {
    let root = omen::cli::Cli::command();
    let mut command = &root;
    let mut flags: BTreeSet<String> = root
        .get_arguments()
        .filter(|argument| argument.is_global_set())
        .map(|argument| argument.get_id().to_string())
        .collect();
    for name in path {
        command = command
            .find_subcommand(name)
            .unwrap_or_else(|| panic!("the CLI has no command {path:?}"));
    }
    flags.extend(
        command
            .get_arguments()
            .map(|argument| argument.get_id().to_string()),
    );
    flags
}

/// The parameters of each tool, read from `tools/list`.
fn tool_parameters() -> Vec<(String, Vec<String>)> {
    let directory = tempfile::TempDir::new().expect("temporary directory");
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let responses = mcp_responses(directory.path(), &[request]);
    responses[0]["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            let name = tool["name"].as_str().expect("name").to_string();
            let parameters = tool["inputSchema"]["properties"]
                .as_object()
                .expect("properties")
                .keys()
                .cloned()
                .collect();
            (name, parameters)
        })
        .collect()
}

/// The command path of a mirror: the words before the positional arguments.
fn command_path(mirror: &Mirror) -> &'static [&'static str] {
    match mirror.command {
        ["search", "query", ..] => &["search", "query"],
        [command, ..] => std::slice::from_ref(command),
        [] => &[],
    }
}

#[test]
fn each_parameter_and_each_flag_has_a_decision() {
    let problems = decision_problems(&tool_parameters());
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// The parameters and the flags that are in no table, and the table entries
/// that are out of date or have no reason.
fn decision_problems(parameters: &[(String, Vec<String>)]) -> Vec<String> {
    let mut problems = Vec::new();

    for mirror in MIRRORS {
        let path = command_path(mirror);
        let command = path.join(" ");
        let flags = flags_of(path);
        let (_, tool_parameters) = parameters
            .iter()
            .find(|(name, _)| name == mirror.tool)
            .expect("tool in tools/list");

        let mut used_flags = BTreeSet::new();
        for parameter in tool_parameters {
            let key = (mirror.tool, parameter.as_str());
            let mcp_only = MCP_ONLY.iter().any(|(entry, _)| *entry == key);
            let flag = PARAMETER_FLAGS
                .iter()
                .find(|(entry, _)| *entry == key)
                .map(|(_, flag)| *flag)
                .or_else(|| {
                    COMMON_PARAMETER_FLAGS
                        .iter()
                        .find(|(common, _)| common == parameter)
                        .map(|(_, flag)| *flag)
                        .filter(|_| !mcp_only)
                });
            match (flag, mcp_only) {
                (Some(_), true) => problems.push(format!(
                    "{}.{parameter} is in PARAMETER_FLAGS and in MCP_ONLY",
                    mirror.tool
                )),
                (Some(flag), false) if !flags.contains(flag) => problems.push(format!(
                    "{}.{parameter}: the command `{command}` has no flag `{flag}`",
                    mirror.tool
                )),
                (Some(flag), false) => {
                    used_flags.insert(flag.to_string());
                }
                (None, true) => {}
                (None, false) => problems.push(format!(
                    "{}.{parameter}: add it to PARAMETER_FLAGS or to MCP_ONLY",
                    mirror.tool
                )),
            }
        }

        for flag in flags.difference(&used_flags) {
            let cli_only = CLI_ONLY.iter().any(|((entry_command, entry_flag), _)| {
                (*entry_command == "*" || *entry_command == command) && entry_flag == flag
            });
            if !cli_only {
                problems.push(format!(
                    "`{command}` --{flag}: add it to PARAMETER_FLAGS or to CLI_ONLY"
                ));
            }
        }
    }

    // An entry that names no parameter or no flag is out of date.
    for ((tool, parameter), _) in PARAMETER_FLAGS.iter().chain(MCP_ONLY) {
        let known = parameters
            .iter()
            .any(|(name, list)| name == tool && list.iter().any(|entry| entry == parameter));
        if !known {
            problems.push(format!("the tool `{tool}` has no parameter `{parameter}`"));
        }
    }
    for ((command, flag), _) in CLI_ONLY {
        let known = MIRRORS.iter().any(|mirror| {
            let path = command_path(mirror);
            (*command == "*" || *command == path.join(" ")) && flags_of(path).contains(*flag)
        });
        if !known {
            problems.push(format!(
                "no mirrored command `{command}` has the flag `{flag}`"
            ));
        }
    }
    for ((name, entry), reason) in MCP_ONLY.iter().chain(CLI_ONLY) {
        if reason.is_empty() {
            problems.push(format!("`{name}` `{entry}` has no reason"));
        }
    }
    problems
}

/// Runs `flags` on the two sides and gives the number of stale flags of each.
fn stale_flags(repository: &Path, flags: &[&str], arguments: Value) -> (u64, u64) {
    let mut command = vec!["flags"];
    command.extend(flags);
    let cli = cli_json(repository, &command);
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "flags", "arguments": arguments}
    });
    let response = &mcp_responses(repository, &[request])[0];
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    let envelope: Value = serde_json::from_str(text).expect("JSON envelope");
    assert_eq!(cli["summary"]["total_flags"], json!(1), "CLI: {cli}");
    assert_eq!(
        envelope["result"]["summary"]["total_flags"],
        json!(1),
        "MCP: {envelope}"
    );
    (
        cli["summary"]["stale_flags"].as_u64().expect("CLI count"),
        envelope["result"]["summary"]["stale_flags"]
            .as_u64()
            .expect("MCP count"),
    )
}

/// The flag of the fixture is from 2024. With an age limit of 100 years it is
/// not stale, and with each smaller limit in this test it is stale.
#[test]
fn stale_days_comes_from_the_flag_then_the_configuration_then_the_default() {
    let directory = fixture_repository();
    let repository = directory.path();
    let unleash = "[feature_flags]\nproviders = [\"unleash\"]\n";

    // No flag and no configured age: the default of 90 days.
    std::fs::write(repository.join("omen.toml"), unleash).expect("write configuration");
    assert_eq!(stale_flags(repository, &[], json!({})), (1, 1));

    // No flag: the configured age applies.
    let configured = format!("{unleash}stale_days = 36500\n");
    std::fs::write(repository.join("omen.toml"), configured).expect("write configuration");
    assert_eq!(stale_flags(repository, &[], json!({})), (0, 0));

    // The flag has priority over the configured age.
    assert_eq!(
        stale_flags(
            repository,
            &["--stale-days", "30"],
            json!({"stale_days": 30})
        ),
        (1, 1)
    );
}

/// A call for which the two sides give different answers by design. The test
/// makes sure that the difference continues to exist, thus a person removes
/// the entry when one side changes.
struct KnownDifference {
    tool: &'static str,
    command: &'static [&'static str],
    arguments: &'static str,
    /// The `error.code` that the tool gives. The command gives a result.
    tool_error: &'static str,
    reason: &'static str,
}

const KNOWN_DIFFERENCES: &[KnownDifference] = &[KnownDifference {
    tool: "get_symbol",
    // The fixture has `helper` in `util.rs` and in `other.rs`.
    command: &["symbol", "helper"],
    arguments: r#"{"name": "helper"}"#,
    tool_error: "ambiguous",
    reason: "The `symbol` command selects the first definition of an ambiguous name. \
             The tool gives an `ambiguous` error. The 5.0 plan changes the command later: \
             it will give the choices and a non-zero exit status.",
}];

#[test]
fn each_known_difference_continues_to_exist() {
    let directory = fixture_repository();
    let problems = known_difference_problems(directory.path());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The known differences that do not exist now, or that have no reason.
fn known_difference_problems(repository: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for difference in KNOWN_DIFFERENCES {
        if difference.reason.is_empty() {
            problems.push(format!("{}: the entry has no reason", difference.tool));
        }
        let cli = cli_json(repository, difference.command);
        if cli.get("error").is_some() || !cli["name"].is_string() {
            problems.push(format!(
                "{}: the command gives no result now: {cli}",
                difference.tool
            ));
        }
        let arguments: Value = serde_json::from_str(difference.arguments).expect("arguments");
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": difference.tool, "arguments": arguments}
        });
        let response = &mcp_responses(repository, &[request])[0];
        let code = &response["result"]["structuredContent"]["error"]["code"];
        if *code != json!(difference.tool_error) {
            problems.push(format!(
                "{}: the tool gives the code {code}, not `{}`. {}",
                difference.tool, difference.tool_error, difference.reason
            ));
        }
    }
    problems
}

/// One configuration key that a tool reads when its argument is absent.
struct ConfiguredKey {
    key: &'static str,
    /// The configuration without the key. Some tools need other keys.
    base: &'static str,
    /// The lines that set the key. They follow `base`.
    configured: &'static str,
    tool: &'static str,
    command: &'static [&'static str],
    /// A JSON pointer in the result. The value at the pointer must be
    /// different with the key and without it.
    probe: &'static str,
}

const UNLEASH: &str = "[feature_flags]\nproviders = [\"unleash\"]\n";

/// Each key has a value that changes the result on the fixture. The history
/// of the fixture has 21 days, and the last commit is the anchor of a window.
const CONFIGURED_KEYS: &[ConfiguredKey] = &[
    ConfiguredKey {
        key: "churn.since",
        base: "",
        configured: "[churn]\nsince = \"3d\"\n",
        tool: "churn",
        command: &["churn"],
        probe: "/files",
    },
    ConfiguredKey {
        key: "duplicates.min_similarity",
        base: "",
        configured: "[duplicates]\nmin_similarity = 0.7\n",
        tool: "clones",
        command: &["clones"],
        probe: "/clones",
    },
    ConfiguredKey {
        key: "duplicates.min_tokens",
        base: "",
        configured: "[duplicates]\nmin_tokens = 10\n",
        tool: "clones",
        command: &["clones"],
        probe: "/clones",
    },
    ConfiguredKey {
        key: "exclude_tests",
        base: "",
        configured: "exclude_tests = false\n",
        tool: "clones",
        command: &["clones"],
        probe: "/clones",
    },
    ConfiguredKey {
        key: "defect.since",
        base: "",
        configured: "[defect]\nsince = \"3d\"\n",
        tool: "defect",
        command: &["defect"],
        probe: "/files",
    },
    ConfiguredKey {
        key: "hotspot.since",
        base: "",
        configured: "[hotspot]\nsince = \"3d\"\n",
        tool: "hotspot",
        command: &["hotspot"],
        probe: "/hotspots",
    },
    ConfiguredKey {
        key: "changes.since",
        base: "",
        configured: "[changes]\nsince = \"3d\"\n",
        tool: "changes",
        command: &["changes"],
        probe: "/commits",
    },
    ConfiguredKey {
        key: "satd.markers",
        base: "",
        configured: "[satd]\nmarkers = [\"REVISIT\"]\n",
        tool: "satd",
        command: &["satd"],
        probe: "/items",
    },
    ConfiguredKey {
        key: "feature_flags.providers",
        base: "",
        configured: UNLEASH,
        tool: "flags",
        command: &["flags"],
        probe: "/flags",
    },
    ConfiguredKey {
        key: "feature_flags.stale_days",
        base: UNLEASH,
        configured: "stale_days = 36500\n",
        tool: "flags",
        command: &["flags"],
        probe: "/summary/stale_flags",
    },
];

/// A function that is almost a clone of `sum_first` in the fixture. Its
/// similarity is approximately 0.75: below the default threshold 0.9, and
/// above the configured threshold 0.7.
const NEAR_CLONE_SOURCE: &str = r#"pub fn sum_near(values: &[u32]) -> u32 {
    let mut total = 0;
    let mut count = 0;
    for value in values {
        if *value > 10 {
            total += value * 2;
            count += 1;
        } else if *value > 5 {
            total += value + 3;
            count += 2;
        } else {
            total += 1;
        }
    }
    if count > 4 {
        total / count
    } else {
        let scaled = total.saturating_add(count);
        scaled.saturating_sub(1)
    }
}
"#;

/// A marker that only `satd.markers` can find.
const MARKED_SOURCE: &str = "// REVISIT: make this faster\npub fn marked() -> u32 {\n    1\n}\n";

/// Adds the files that the configuration keys need to the fixture.
fn add_configuration_files(repository: &Path) {
    std::fs::write(repository.join("near.rs"), NEAR_CLONE_SOURCE).expect("write near clone");
    std::fs::write(repository.join("marked.rs"), MARKED_SOURCE).expect("write marked file");
    // A clone in test code: only `exclude_tests = false` reads it.
    let tests = repository.join("tests");
    std::fs::create_dir(&tests).expect("tests directory");
    let clone_in_tests = format!(
        "{NEAR_CLONE_SOURCE}\n{}",
        NEAR_CLONE_SOURCE.replace("sum_near", "sum_near_again")
    );
    std::fs::write(tests.join("sums.rs"), clone_in_tests).expect("write test clone");
}

/// Runs the command and the tool, makes sure that they agree, and gives the
/// value at the probe.
fn probe_value(repository: &Path, key: &ConfiguredKey, configuration: &str) -> Value {
    std::fs::write(repository.join("omen.toml"), configuration).expect("write configuration");
    let mut command: Vec<&str> = key.command.to_vec();
    command.extend(["--top", "0"]);
    let cli = without_exemptions(key.tool, cli_json(repository, &command));
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": key.tool, "arguments": {"limit": 0}}
    });
    let response = &mcp_responses(repository, &[request])[0];
    assert_ne!(
        response["result"]["isError"],
        json!(true),
        "{}: {response}",
        key.key
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    let envelope: Value = serde_json::from_str(text).expect("JSON envelope");
    let mcp = without_exemptions(key.tool, envelope["result"].clone());

    let mut places = Vec::new();
    differences("", &cli, &mcp, &mut places);
    assert!(
        places.is_empty(),
        "{} with the configuration {configuration:?}: the CLI and the MCP tool are different:\n{}",
        key.key,
        places.join("\n")
    );
    cli.pointer(key.probe)
        .unwrap_or_else(|| panic!("{}: the result has no `{}`", key.key, key.probe))
        .clone()
}

/// The test above passes when a handler does not read its key and the two
/// values of the key give the same result. This test makes sure that each
/// key changes the result, on the CLI and on the MCP server.
#[test]
fn each_configuration_key_changes_the_result_of_its_tool() {
    let directory = fixture_repository();
    let repository = directory.path();
    add_configuration_files(repository);
    let problems = keys_without_effect(repository);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The keys for which the result is the same with the key and without it.
fn keys_without_effect(repository: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for key in CONFIGURED_KEYS {
        let without = probe_value(repository, key, key.base);
        let with = probe_value(repository, key, &format!("{}{}", key.base, key.configured));
        if without == with {
            problems.push(format!(
                "{}: `{}` of `{}` is the same with the key and without it: {with}",
                key.key, key.probe, key.tool
            ));
        }
    }
    problems
}

#[test]
fn each_configuration_key_of_the_tools_has_a_discriminating_case() {
    // The keys that a tool specification names, and the keys that a handler
    // reads without a parameter.
    let expected: BTreeSet<&str> = [
        "churn.since",
        "duplicates.min_tokens",
        "duplicates.min_similarity",
        "exclude_tests",
        "defect.since",
        "hotspot.since",
        "changes.since",
        "satd.markers",
        "feature_flags.providers",
        "feature_flags.stale_days",
    ]
    .into_iter()
    .collect();
    let covered: BTreeSet<&str> = CONFIGURED_KEYS.iter().map(|key| key.key).collect();
    assert_eq!(covered, expected);

    // Each key that a schema names is in the table.
    let named = keys_in_the_schemas();
    assert!(!named.is_empty());
    let absent: Vec<&String> = named
        .iter()
        .filter(|key| !covered.contains(key.as_str()))
        .collect();
    assert_eq!(absent, Vec::<&String>::new());
}

/// The configuration keys that the descriptions in `tools/list` name.
fn keys_in_the_schemas() -> BTreeSet<String> {
    let directory = tempfile::TempDir::new().expect("temporary directory");
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let response = &mcp_responses(directory.path(), &[request])[0];
    response
        .to_string()
        .split("The default is `")
        .skip(1)
        .filter_map(|part| part.split('`').next())
        .map(str::to_string)
        .collect()
}
