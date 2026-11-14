//! The command `omen security`: the sections, the filters, the formats, and the gate.

#[path = "support/fixture.rs"]
mod fixture;

use std::path::Path;

use serde_json::{json, Value};
use tempfile::TempDir;

const BASE: &str = "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\", methods=[\"POST\"])\ndef run_command():\n    return helper()\n\ndef helper():\n    return 1\n";
const HEAD: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\", methods=[\"POST\"])\ndef run_command():\n    return helper()\n\ndef helper():\n    os.system(\"x\" + os.environ[\"A\"])\n\ndef dead(expression):\n    eval(expression)\n";

fn omen(directory: &Path, arguments: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_omen"))
        .arg("-p")
        .arg(directory)
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("omen starts")
}

fn json(directory: &Path, arguments: &[&str]) -> Value {
    let mut all = vec!["-f", "json"];
    all.extend_from_slice(arguments);
    let output = omen(directory, &all);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON")
}

fn project(source: &str) -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    std::fs::write(directory.path().join("app.py"), source).expect("write");
    directory
}

fn two_commits() -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path();
    let first = "2024-02-01T12:00:00Z";
    fixture::git(path, "ada", first, &["init", "--initial-branch", "main"]);
    std::fs::write(path.join("app.py"), BASE).expect("write");
    fixture::commit(path, "ada", first, "feat: add the app");
    let second = "2024-02-08T12:00:00Z";
    fixture::git(path, "grace", second, &["checkout", "-b", "feature"]);
    std::fs::write(path.join("app.py"), HEAD).expect("write");
    fixture::commit(path, "grace", second, "feat: add a command");
    directory
}

#[test]
fn test_the_report_has_the_sinks_the_surface_and_the_paths() {
    let directory = project(HEAD);
    let report = json(directory.path(), &["security"]);
    assert_eq!(report["summary"]["sinks"], 2);
    assert_eq!(report["summary"]["entry_points"], 1);
    assert_eq!(report["summary"]["paths"], 1);
    assert_eq!(report["summary"]["sinks_with_no_known_entry"], 1);
    assert!(report["coverage"]["statement"]
        .as_str()
        .unwrap()
        .contains("does not mean that the code is safe"));
}

#[test]
fn test_each_section_gives_only_its_fields() {
    let directory = project(HEAD);
    let sinks = json(directory.path(), &["security", "sinks"]);
    assert!(sinks.get("sinks").is_some() && sinks.get("paths").is_none());
    let surface = json(directory.path(), &["security", "surface"]);
    assert!(surface.get("entry_points").is_some() && surface.get("sinks").is_none());
    let paths = json(directory.path(), &["security", "paths"]);
    assert!(paths.get("paths").is_some() && paths.get("entry_points").is_none());
}

#[test]
fn test_paths_can_be_filtered_by_sink_and_by_entry_point() {
    let directory = project(HEAD);
    let all = json(directory.path(), &["security", "paths"]);
    let entry = all["paths"][0]["entry"].as_str().unwrap().to_string();
    let sink = all["paths"][0]["sink"].as_str().unwrap().to_string();
    let by_entry = json(directory.path(), &["security", "paths", "--entry", &entry]);
    assert_eq!(by_entry["paths"].as_array().unwrap().len(), 1);
    let none = json(
        directory.path(),
        &["security", "paths", "--entry", "nobody"],
    );
    assert!(none["paths"].as_array().unwrap().is_empty());
    let by_sink = json(directory.path(), &["security", "paths", "--sink", &sink]);
    assert_eq!(by_sink["paths"].as_array().unwrap().len(), 1);
    let rejected = omen(directory.path(), &["security", "sinks", "--entry", &entry]);
    assert!(!rejected.status.success());
}

#[test]
fn test_mermaid_applies_to_paths_only() {
    let directory = project(HEAD);
    let output = omen(directory.path(), &["-f", "mermaid", "security", "paths"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("flowchart LR"), "{text}");
    assert!(text.contains("command_execution (CWE-78)"));
    let rejected = omen(directory.path(), &["-f", "mermaid", "security", "sinks"]);
    assert!(!rejected.status.success());
}

#[test]
fn test_sarif_has_one_result_for_each_sink() {
    let directory = project(HEAD);
    let output = omen(directory.path(), &["-f", "sarif", "security"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("SARIF");
    assert_eq!(value["version"], "2.1.0");
    assert_eq!(value["runs"][0]["results"].as_array().unwrap().len(), 2);
}

#[test]
fn test_a_suppression_with_a_reason_is_counted_and_removes_the_sink() {
    let source = HEAD.replace(
        "    os.system(",
        "    # omen:allow command_execution: reviewed\n    os.system(",
    );
    let directory = project(&source);
    let report = json(directory.path(), &["security"]);
    assert_eq!(report["summary"]["sinks"], 1);
    assert_eq!(report["summary"]["suppressed"], 1);
    assert_eq!(report["suppressed"][0]["reason"], "reviewed");
}

#[test]
fn test_the_gate_is_off_by_default_and_the_modes_follow_the_new_sinks() {
    let directory = two_commits();
    let path = directory.path();
    let off = omen(path, &["security", "-t", "main"]);
    assert!(off.status.success());
    let warn = omen(path, &["security", "-t", "main", "--gate", "warn"]);
    assert!(warn.status.success());
    assert!(String::from_utf8_lossy(&warn.stderr).contains("command_execution"));
    let error = omen(path, &["security", "-t", "main", "--gate", "error"]);
    assert_eq!(error.status.code(), Some(2));
    let other = omen(
        path,
        &[
            "security",
            "-t",
            "main",
            "--gate",
            "error",
            "--gate-category",
            "weak_hash",
        ],
    );
    assert!(
        other.status.success(),
        "a category that the change does not add"
    );
    let strong = omen(
        path,
        &[
            "security",
            "-t",
            "main",
            "--gate",
            "error",
            "--gate-basis",
            "imported_symbol",
            "--gate-category",
            "code_evaluation",
        ],
    );
    assert!(strong.status.success(), "the builtin `eval` has no import");
}

#[test]
fn test_the_configuration_rejects_an_unknown_key_in_the_security_section() {
    let directory = project(HEAD);
    std::fs::write(
        directory.path().join("omen.toml"),
        "[security]\nunknown_key = 1\n",
    )
    .expect("write");
    let output = omen(directory.path(), &["security"]);
    assert!(!output.status.success());
}

#[test]
fn test_the_mcp_tool_gives_the_section_and_rejects_entry_outside_paths() {
    let directory = project(HEAD);
    let call = |arguments: Value| {
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "security", "arguments": arguments}
        });
        let responses = fixture::mcp_responses(directory.path(), &[request]);
        responses[0].to_string()
    };
    let sinks = call(json!({"section": "sinks"}));
    assert!(sinks.contains("command_execution"), "{sinks}");
    assert!(!sinks.contains("choke_points"), "{sinks}");
    let paths = call(json!({"section": "paths"}));
    assert!(paths.contains("choke_points"), "{paths}");
    let wrong = call(json!({"section": "sinks", "entry": "x"}));
    assert!(wrong.contains("invalid_argument"), "{wrong}");
}

#[test]
fn test_symbol_and_impact_have_the_security_field() {
    let directory = project(HEAD);
    let symbol = json(directory.path(), &["symbol", "helper"]);
    assert_eq!(
        symbol["security"]["sinks_in_body"][0]["category"],
        "command_execution"
    );
    assert_eq!(
        symbol["security"]["reachable_from"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let impact = json(directory.path(), &["impact", "run_command"]);
    assert_eq!(impact["security"]["entry_point"]["framework"], "flask");
    assert_eq!(impact["security"]["reaches"].as_array().unwrap().len(), 1);
}
