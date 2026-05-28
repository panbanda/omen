use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn omen() -> Command {
    #[allow(deprecated)]
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
fn test_satd_sarif_output() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "sarif", "satd"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\": \"2.1.0\""))
        .stdout(predicate::str::contains("\"runs\""));
}

#[test]
fn test_context_json_outputs_context_pack() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "context"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"hints\""))
        .stdout(predicate::str::contains("\"top_symbols\""))
        .stdout(predicate::str::contains("\"languages\""));
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

#[test]
fn test_changed_since_filters_analyzer_files() {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/a.rs"), "fn unchanged() {}\n").unwrap();
    std::fs::write(temp.path().join("src/b.rs"), "fn changed() {}\n").unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    std::fs::write(
        temp.path().join("src/b.rs"),
        "fn changed() { if true {} }\n",
    )
    .unwrap();

    let output = omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "-f",
            "json",
            "complexity",
            "--changed-since",
            "HEAD",
        ])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let files = parsed["files"].as_array().unwrap();

    assert_eq!(files.len(), 1, "expected only changed file: {stdout}");
    assert!(files[0]["path"].as_str().unwrap().ends_with("src/b.rs"));
    assert!(!stdout.contains("src/a.rs"));
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
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("complexity output should be valid JSON");
    let files = parsed["files"]
        .as_array()
        .expect("files should be an array");
    assert!(
        !files.is_empty(),
        "expected filtered output to include files"
    );
    for file in files {
        let path = file["path"].as_str().expect("file path should be a string");
        assert!(
            path.ends_with(".py"),
            "expected only Python files in filtered output, got {path}"
        );
    }
}

#[test]
fn test_exclude_filter() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-e",
            "*.py",
        ])
        .output()
        .expect("command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("complexity output should be valid JSON");
    let files = parsed["files"]
        .as_array()
        .expect("files should be an array");
    assert!(
        !files.is_empty(),
        "expected output to include non-Python files"
    );
    for file in files {
        let path = file["path"].as_str().expect("file path should be a string");
        assert!(
            !path.ends_with(".py"),
            "expected Python files to be excluded, got {path}"
        );
    }
}

// ---------------------------------------------------------------------------
// Additional glob and exclude filter tests (filtered_file_set paths)
// ---------------------------------------------------------------------------

#[test]
fn test_satd_glob_filter_limits_to_python() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "satd", "-g", "*.py"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("satd output should be valid JSON");

    // All reported file paths should end with .py
    if let Some(findings) = parsed["findings"].as_array() {
        for finding in findings {
            let path = finding["file"].as_str().unwrap_or("");
            assert!(
                path.ends_with(".py"),
                "satd glob filter should only report Python files, got: {path}"
            );
        }
    }
}

#[test]
fn test_satd_exclude_filter_removes_rust_files() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "satd", "-e", "*.rs"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("satd output should be valid JSON");

    // No reported file paths should end with .rs
    if let Some(findings) = parsed["findings"].as_array() {
        for finding in findings {
            let path = finding["file"].as_str().unwrap_or("");
            assert!(
                !path.ends_with(".rs"),
                "satd exclude filter should not report Rust files, got: {path}"
            );
        }
    }
}

#[test]
fn test_deadcode_exclude_filter_removes_python_files() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "deadcode", "-e", "*.py"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("deadcode output should be valid JSON");

    // No reported file paths should end with .py
    if let Some(findings) = parsed["findings"].as_array() {
        for finding in findings {
            let path = finding["file"].as_str().unwrap_or("");
            assert!(
                !path.ends_with(".py"),
                "deadcode exclude filter should not report Python files, got: {path}"
            );
        }
    }
}

#[test]
fn test_flags_glob_filter_limits_to_rust() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "flags", "-g", "*.rs"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    // flags analyzer succeeds with a glob-filtered file set
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
        "flags with glob filter should produce valid JSON"
    );
}

#[test]
fn test_complexity_glob_and_exclude_combined() {
    // Include all .rs files then exclude nothing matching Python
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.rs",
            "-e",
            "*.go",
        ])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("complexity output should be valid JSON");
    let files = parsed["files"]
        .as_array()
        .expect("files should be an array");

    // All files should be Rust (glob filters to .rs; exclude .go has no effect since glob already limits to .rs)
    for file in files {
        let path = file["path"].as_str().expect("file path should be a string");
        assert!(
            path.ends_with(".rs"),
            "combined glob+exclude should only include Rust files, got: {path}"
        );
    }
}

#[test]
fn test_complexity_glob_no_match_produces_empty_files() {
    let output = omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "complexity",
            "-g",
            "*.java",
        ])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("complexity output should be valid JSON");
    let files = parsed["files"]
        .as_array()
        .expect("files should be an array");

    assert!(
        files.is_empty(),
        "glob that matches no files should result in empty files array"
    );
}

#[test]
fn test_clones_glob_filter_limits_to_rust() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "clones", "-g", "*.rs"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
        "clones with glob filter should produce valid JSON"
    );
}

#[test]
fn test_repomap_glob_filter_limits_to_python() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "repomap", "-g", "*.py"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
        "repomap with glob filter should produce valid JSON"
    );
}

#[test]
fn test_complexity_check_with_glob_filter() {
    let tmp = TempDir::new().expect("create temp dir");
    // Write a simple function that won't exceed complexity thresholds
    std::fs::write(
        tmp.path().join("simple.rs"),
        "fn simple_function() { let x = 1; }",
    )
    .expect("write file");
    std::fs::write(tmp.path().join("ignored.py"), "def ignored(): pass\n").expect("write file");

    omen()
        .args([
            "-p",
            tmp.path().to_str().unwrap(),
            "complexity",
            "--check",
            "-g",
            "*.rs",
        ])
        .assert()
        .success();
}

#[test]
fn test_changes_glob_filter() {
    // changes command uses run_changes_analyzer which also calls filtered_file_set
    let output = omen()
        .args(["-p", ".", "-f", "json", "changes", "-g", "*.rs"])
        .output()
        .expect("command runs");

    // changes may fail if no git history, but should not panic
    // If it succeeds, verify the output is valid JSON
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
            "changes with glob filter should produce valid JSON when successful"
        );
    }
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
