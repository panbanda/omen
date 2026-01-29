use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn omen() -> Command {
    Command::cargo_bin("omen").expect("binary exists")
}

fn fixtures_dir() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")
}

// ---------------------------------------------------------------------------
// CLI smoke tests
// ---------------------------------------------------------------------------

#[test]
fn test_help_output() {
    omen()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("code analysis"));
}

#[test]
fn test_complexity_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "complexity"])
        .assert()
        .success();
}

#[test]
fn test_complexity_json_output() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "complexity"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cyclomatic"));
}

#[test]
fn test_satd_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "satd"])
        .assert()
        .success();
}

#[test]
fn test_deadcode_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "deadcode"])
        .assert()
        .success();
}

#[test]
fn test_cohesion_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "cohesion"])
        .assert()
        .success();
}

#[test]
fn test_flags_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "flags"])
        .assert()
        .success();
}

#[test]
fn test_clones_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "clones"])
        .assert()
        .success();
}

#[test]
fn test_defect_requires_git_repo() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "defect"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("git"));
}

#[test]
fn test_tdg_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "tdg"])
        .assert()
        .success();
}

#[test]
fn test_graph_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "graph"])
        .assert()
        .success();
}

#[test]
fn test_repomap_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "repomap"])
        .assert()
        .success();
}

#[test]
fn test_smells_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "smells"])
        .assert()
        .success();
}

#[test]
fn test_score_runs_successfully() {
    omen()
        .args(["-p", ".", "-f", "json", "score"])
        .assert()
        .success()
        .stdout(predicate::str::contains("overall_score"));
}

#[test]
fn test_all_analyzers_no_panic() {
    omen()
        .args(["-p", fixtures_dir(), "all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("analyzers"));
}

#[test]
fn test_json_output_is_valid_json() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "complexity"])
        .output()
        .expect("command runs");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(parsed.is_ok(), "stdout is not valid JSON: {}", stdout);
}

#[test]
fn test_markdown_output() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "markdown", "complexity"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# "));
}

#[test]
fn test_text_output() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "text", "complexity"])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Multi-language fixture tests
// ---------------------------------------------------------------------------

#[test]
fn test_complexity_rust_fixture() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.rs",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fibonacci"),
        "expected fibonacci in Rust output: {}",
        stdout
    );
    assert!(
        stdout.contains("validate"),
        "expected validate in Rust output: {}",
        stdout
    );
}

#[test]
fn test_complexity_python_fixture() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.py",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("get_user"),
        "expected get_user in Python output: {}",
        stdout
    );
    assert!(
        stdout.contains("calculate_discount"),
        "expected calculate_discount in Python output: {}",
        stdout
    );
}

#[test]
fn test_complexity_go_fixture() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.go",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("validate"),
        "expected validate in Go output: {}",
        stdout
    );
    assert!(
        stdout.contains("maxOf"),
        "expected maxOf in Go output: {}",
        stdout
    );
}

#[test]
fn test_complexity_ruby_fixture() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.rb",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("process"),
        "expected process in Ruby output: {}",
        stdout
    );
}

#[test]
fn test_complexity_typescript_fixture() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.ts",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("parseConfig"),
        "expected parseConfig in TypeScript output: {}",
        stdout
    );
}

#[test]
fn test_satd_detects_todo_in_python() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "satd", "-g", "*.py"])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TODO"),
        "expected SATD to detect TODO in Python fixture: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

#[test]
fn test_invalid_path_returns_error() {
    omen()
        .args(["-p", "/nonexistent/path/that/does/not/exist", "complexity"])
        .assert()
        .failure();
}

#[test]
fn test_nonexistent_path_error() {
    omen()
        .args(["-p", "/tmp/__omen_nonexistent_xyz__", "complexity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn test_empty_directory() {
    let tmp = TempDir::new().expect("create temp dir");
    omen()
        .args([
            "-p",
            tmp.path().to_str().unwrap(),
            "-f",
            "json",
            "complexity",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Glob and exclude filter tests
// ---------------------------------------------------------------------------

#[test]
fn test_glob_filter() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.py",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".py"),
        "expected .py files in filtered output"
    );
}

// ---------------------------------------------------------------------------
// Score analyzer tests
// ---------------------------------------------------------------------------

#[test]
fn test_score_json_structure() {
    let output = omen()
        .args(["-p", ".", "-f", "json", "score"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("score output should be valid JSON");

    assert!(
        parsed.get("overall_score").is_some(),
        "missing overall_score"
    );
    assert!(parsed.get("grade").is_some(), "missing grade");
    assert!(parsed.get("components").is_some(), "missing components");
}

#[test]
fn test_score_grade_is_valid() {
    let output = omen()
        .args(["-p", ".", "-f", "json", "score"])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let grade = parsed["grade"].as_str().unwrap();
    assert!(
        ["A", "B", "C", "D", "F"].contains(&grade),
        "unexpected grade: {}",
        grade,
    );
}

// ---------------------------------------------------------------------------
// All command output structure
// ---------------------------------------------------------------------------

#[test]
fn test_all_json_has_analyzers_array() {
    let output = omen()
        .args(["-p", fixtures_dir(), "all"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("all output should be valid JSON");

    let analyzers = parsed["analyzers"]
        .as_array()
        .expect("analyzers should be an array");
    assert!(!analyzers.is_empty(), "analyzers array should not be empty");

    for entry in analyzers {
        assert!(
            entry.get("analyzer").is_some(),
            "each entry needs an analyzer name"
        );
    }
}

// ---------------------------------------------------------------------------
// Output format consistency
// ---------------------------------------------------------------------------

#[test]
fn test_all_three_formats_succeed() {
    for format in &["json", "markdown", "text"] {
        omen()
            .args(["-p", fixtures_dir(), "-f", format, "complexity"])
            .assert()
            .success();
    }
}
