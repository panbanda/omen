use assert_cmd::cargo::CommandCargoExt;
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

// ---------------------------------------------------------------------------
// stubs analyzer
// ---------------------------------------------------------------------------

#[test]
fn test_stubs_runs_successfully() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "stubs"])
        .assert()
        .success();
}

#[test]
fn test_stubs_json_shape() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "stubs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"stubs\""))
        .stdout(predicate::str::contains("\"by_category\""))
        .stdout(predicate::str::contains("\"total_stubs\""));
}

/// Writes a small multi-language fixture with one stub per pattern type
/// (not_implemented, elision, empty_body) and returns the temp dir.
fn write_mixed_stub_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("todo.rs"),
        "fn f() -> i32 {\n    todo!()\n}\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("throw.ts"),
        "function f() {\n  throw new Error('not implemented');\n}\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("elided.go"),
        "package p\n\nfunc f() {\n\t// ... rest of the implementation\n}\n",
    )
    .unwrap();
    temp
}

#[test]
fn test_stubs_end_to_end_detects_all_pattern_types() {
    let temp = write_mixed_stub_fixture();

    let output = omen()
        .args(["-p", temp.path().to_str().unwrap(), "-f", "json", "stubs"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    // todo.rs and throw.ts each yield one not_implemented finding (2 unique
    // sites). elided.go's function body is *only* the elision comment, so
    // its elision and empty_body signals describe the SAME unfinished site
    // and are merged into a single finding rather than reported twice --
    // total is 3 unique sites, not 4 raw category matches.
    let stubs = parsed["stubs"].as_array().unwrap();
    assert_eq!(stubs.len(), 3, "expected 3 unique sites: {stubs:#?}");
    assert_eq!(parsed["summary"]["total_stubs"], 3);
    assert_eq!(parsed["by_category"]["not_implemented"], 2);
    assert_eq!(parsed["by_category"]["empty_body"], 1);
    assert!(parsed["by_category"].get("elision").is_none());

    let go_finding = stubs
        .iter()
        .find(|s| s["file"] == "elided.go")
        .expect("elided.go finding");
    assert_eq!(go_finding["category"], "empty_body");
    assert_eq!(
        go_finding["categories"],
        serde_json::json!(["elision", "empty_body"])
    );

    // Paths are repo-relative, matching the other analyzers' convention.
    assert!(!parsed.to_string().contains(temp.path().to_str().unwrap()));
}

#[test]
fn test_stubs_gate_off_by_default_exits_zero_even_with_stubs() {
    let temp = write_mixed_stub_fixture();
    omen()
        .args(["-p", temp.path().to_str().unwrap(), "-f", "json", "stubs"])
        .assert()
        .success();
}

#[test]
fn test_stubs_gate_error_exits_two_on_fixture_with_stubs() {
    let temp = write_mixed_stub_fixture();
    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "-f",
            "json",
            "stubs",
            "--gate",
            "error",
        ])
        .assert()
        .code(2)
        // JSON report must still reach stdout even when the gate fails.
        .stdout(predicate::str::contains("\"stubs\""))
        .stderr(predicate::str::contains("stub"));
}

#[test]
fn test_stubs_gate_error_exits_zero_on_clean_fixture() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("clean.rs"),
        "fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "-f",
            "json",
            "stubs",
            "--gate",
            "error",
        ])
        .assert()
        .success();
}

#[test]
fn test_stubs_gate_warn_exits_zero_even_with_stubs() {
    let temp = write_mixed_stub_fixture();
    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "stubs",
            "--gate",
            "warn",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("stub"));
}

/// Acceptance test: the stubs analyzer must report zero findings on omen's
/// own `src/` tree. This is the project's own real, complete codebase --
/// full of legitimate doc comments, explanatory "placeholder"/"stub"
/// wording, and panics/exceptions whose messages happen to share
/// substrings with trigger words (e.g. `panic!("Expected Stubs command")`).
/// Any finding here is a precision regression, not a real stub.
#[test]
fn test_stubs_on_own_source_reports_zero_false_positives() {
    let repo_root = env!("CARGO_MANIFEST_DIR");
    let output = omen()
        .args(["-p", repo_root, "-f", "json", "stubs"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let stubs = parsed["stubs"].as_array().cloned().unwrap_or_default();
    assert!(
        stubs.is_empty(),
        "expected zero stubs on omen's own source, found {}: {:#?}",
        stubs.len(),
        stubs
    );
}

#[test]
fn test_stubs_gate_severity_high_ignores_medium_findings() {
    // This fixture yields a single Medium-severity finding (the empty
    // function whose only content is an elision comment, merged into one
    // `empty_body` site); gating on "high" alone must not trip on it.
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("elided.go"),
        "package p\n\nfunc f() {\n\t// ... rest of the implementation\n}\n",
    )
    .unwrap();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "stubs",
            "--gate",
            "error",
            "--gate-severity",
            "high",
        ])
        .assert()
        .success();
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
fn test_analyzer_json_paths_are_repo_relative() {
    for analyzer in ["complexity", "satd", "deadcode"] {
        let output = omen()
            .args(["-p", fixtures_dir(), "-f", "json", analyzer])
            .output()
            .unwrap();
        assert!(output.status.success(), "{analyzer}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(fixtures_dir()), "{analyzer}: {stdout}");
        if analyzer == "complexity" {
            let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            assert!(value["files"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|file| file["functions"].as_array().unwrap())
                .all(|function| function.get("file").is_none()));
        }
    }
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
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("sample.rs"), "pub fn sample() {}\n").unwrap();
    omen()
        .args(["-p", temp.path().to_str().unwrap(), "-f", "json", "defect"])
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
fn test_threshold_violation_exits_two_and_emits_json() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "score",
            "--check",
            "--fail-under",
            "99",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("\"violations\""));
}

#[test]
fn test_operational_failure_exits_one() {
    omen()
        .args(["-p", "/definitely/not/a/repository", "complexity"])
        .assert()
        .code(1);
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

#[test]
fn test_changed_since_rejects_option_like_ref() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "complexity",
            "--changed-since=--output=stolen",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must not start with '-'"));
}

#[test]
fn test_mutation_refuses_dirty_targets_unless_allowed() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(temp.path().join("lib.rs"), "pub fn value() -> i32 { 2 }\n").unwrap();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "mutation", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "mutation",
            "--dry-run",
            "--allow-dirty",
        ])
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn test_mutation_sigint_restores_active_source_file() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("lib.rs");
    let marker = temp.path().join("test-started");
    let original = "pub fn value() -> i32 { 1 }\n";
    std::fs::write(&source, original).unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    let test_command = format!("touch '{}' && sleep 2", marker.display());
    #[allow(deprecated)]
    let mut command = std::process::Command::cargo_bin("omen").unwrap();
    command.args([
        "-p",
        temp.path().to_str().unwrap(),
        "mutation",
        "--test-command",
        &test_command,
    ]);
    let mut child = command
        .current_dir(temp.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    for _ in 0..250 {
        if marker.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(marker.exists(), "mutation test command did not start");
    assert_ne!(std::fs::read_to_string(&source).unwrap(), original);

    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let status = child.wait().unwrap();

    assert_eq!(status.code(), Some(130));
    assert_eq!(std::fs::read_to_string(source).unwrap(), original);
}

#[test]
fn test_deadcode_only_runs_cargo_check_when_opted_in() {
    let temp = TempDir::new().unwrap();
    let marker = temp.path().join("build-script-ran");
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"deadcode-opt-in\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "fn unused() {}\n").unwrap();
    std::fs::write(
        temp.path().join("build.rs"),
        format!(
            "fn main() {{ std::fs::write(r#\"{}\"#, b\"ran\").unwrap(); }}\n",
            marker.display()
        ),
    )
    .unwrap();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "deadcode"])
        .assert()
        .success();
    assert!(!marker.exists());

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "deadcode",
            "--cargo-check",
        ])
        .assert()
        .success();
    assert!(marker.exists());
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

fn write_complexity_exclude_fixture(temp: &TempDir) {
    let branchy = r#"
export function branchy(a: boolean, b: boolean, c: boolean, d: boolean, e: boolean, f: boolean) {
  if (a) return 1;
  if (b) return 2;
  if (c) return 3;
  if (d) return 4;
  if (e) return 5;
  if (f) return 6;
  return 0;
}
"#;
    for path in [
        "src/branchy.ts",
        "packages/alpha/src/branchy.ts",
        "packages/alpha/test/setup.ts",
        "packages/alpha/test/helper.test.ts",
    ] {
        let path = temp.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, branchy).unwrap();
    }
    std::fs::write(
        temp.path().join("omen.toml"),
        "[complexity]\ncyclomatic_error = 3\n",
    )
    .unwrap();
}

#[test]
fn test_complexity_check_excludes_nested_test_directory() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "complexity",
            "--check",
            "-e",
            "packages/alpha/test/**",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Complexity threshold exceeded in 2 function(s)",
        ));
}

#[test]
fn test_complexity_check_honors_config_excludes() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);
    std::fs::write(
        temp.path().join("omen.toml"),
        "exclude = [\"packages/alpha/test/**\"]\n\n[complexity]\ncyclomatic_error = 3\n",
    )
    .unwrap();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "complexity", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Complexity threshold exceeded in 2 function(s)",
        ));
}

#[test]
fn test_config_excludes_apply_to_satd_results() {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join("excluded")).unwrap();
    std::fs::write(temp.path().join("keep.ts"), "// TODO: keep\n").unwrap();
    std::fs::write(temp.path().join("excluded/debt.ts"), "// TODO: exclude\n").unwrap();
    std::fs::write(
        temp.path().join("omen.toml"),
        "exclude = [\"excluded/**\"]\n",
    )
    .unwrap();

    let output = omen()
        .args(["-p", temp.path().to_str().unwrap(), "-f", "json", "satd"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let findings = parsed["items"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["file"], "keep.ts");
}

#[test]
fn test_score_records_git_components_skipped_outside_repository() {
    use omen::core::{AnalysisContext, Analyzer as _, FileSet};

    let temp = TempDir::new().unwrap();
    let config = omen::config::Config::default();
    let files = FileSet::from_path(temp.path(), &config).unwrap();
    let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));
    let weights = omen::score::ScoreWeights {
        churn: 1.0,
        ownership: 1.0,
        defect: 1.0,
        complexity: 0.0,
        satd: 0.0,
        deadcode: 0.0,
        duplicates: 0.0,
        cohesion: 0.0,
        tdg: 0.0,
        coupling: 0.0,
        smells: 0.0,
    };

    let result = omen::score::Analyzer::with_weights(weights)
        .analyze(&ctx)
        .unwrap();
    let names: Vec<&str> = result
        .skipped_components
        .iter()
        .map(|component| component.name.as_str())
        .collect();

    assert!(names.contains(&"churn"));
    assert!(names.contains(&"ownership"));
    assert!(names.contains(&"defect"));
    assert!(result
        .skipped_components
        .iter()
        .all(|component| !component.reason.is_empty()));
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
            "*.xyz",
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

// ---------------------------------------------------------------------------
// Gate/check: --gate off|warn|error, --check deprecated alias, exit codes
// ---------------------------------------------------------------------------
//
// Convention under test: off = report only (exit 0); warn = report + one-line
// stderr summary (exit 0); error = report; on violation return
// Error::ThresholdViolation, which main() maps to exit code 2. --check is a
// deprecated alias for --gate error; --gate wins if both are given.

#[test]
fn test_complexity_gate_error_exits_two_on_violation() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "complexity",
            "--gate",
            "error",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Complexity threshold exceeded"));
}

#[test]
fn test_complexity_gate_warn_exits_zero_on_violation() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "complexity",
            "--gate",
            "warn",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Gate mode 'warn'"));
}

#[test]
fn test_complexity_gate_off_default_exits_zero_on_violation() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "complexity"])
        .assert()
        .success();
}

#[test]
fn test_complexity_check_alias_still_exits_two() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "complexity", "--check"])
        .assert()
        .code(2);
}

#[test]
fn test_score_gate_error_exits_two_and_emits_json() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "score",
            "--gate",
            "error",
            "--fail-under",
            "99",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("\"violations\""));
}

#[test]
fn test_score_gate_warn_exits_zero() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "score",
            "--gate",
            "warn",
            "--fail-under",
            "99",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Gate mode 'warn'"));
}

#[test]
fn test_score_check_alias_still_exits_two() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "score",
            "--check",
            "--fail-under",
            "99",
        ])
        .assert()
        .code(2);
}

/// `--dry-run` only generates mutants (no test execution), so the mutation
/// score is always 0.0 (0 killed / 0 scored) -- reliably below the default
/// `--min-score` of 0.8. This lets the gate be exercised deterministically
/// without running real tests.
fn write_mutation_gate_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn is_positive(x: i32) -> bool { x > 0 }\n",
    )
    .unwrap();
    temp
}

#[test]
fn test_mutation_gate_error_exits_two_on_dry_run() {
    let temp = write_mutation_gate_fixture();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "mutation",
            "--dry-run",
            "--gate",
            "error",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Mutation score"));
}

#[test]
fn test_mutation_gate_warn_exits_zero_on_dry_run() {
    let temp = write_mutation_gate_fixture();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "mutation",
            "--dry-run",
            "--gate",
            "warn",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Gate mode 'warn'"));
}

#[test]
fn test_mutation_gate_off_default_exits_zero_on_dry_run() {
    let temp = write_mutation_gate_fixture();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "mutation", "--dry-run"])
        .assert()
        .success();
}

#[test]
fn test_mutation_check_alias_still_exits_two_on_dry_run() {
    let temp = write_mutation_gate_fixture();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "mutation",
            "--dry-run",
            "--check",
        ])
        .assert()
        .code(2);
}

#[test]
fn test_operational_error_exits_one_not_two() {
    // Sanity check that op errors (bad path) are distinct from gate
    // violations: exit 1, not 2, and not success.
    omen()
        .args(["-p", "/definitely/not/a/repository", "-f", "json", "score"])
        .assert()
        .code(1);
}

// ---------------------------------------------------------------------------
// Pagination: diff/mutation routed through format_with_limits
// ---------------------------------------------------------------------------

#[test]
fn test_diff_top_offset_flags_parse_and_run() {
    // diff's git plumbing (target resolution, no-diff cases) is exercised
    // elsewhere; this only checks that --top/--offset reach format_with_limits
    // without panicking and that the output stays valid JSON either way.
    let output = omen()
        .args([
            "-p", ".", "-f", "json", "diff", "--top", "1", "--offset", "0",
        ])
        .output()
        .unwrap();
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
            "diff --top/--offset should produce valid JSON when successful"
        );
    }
}

#[test]
fn test_mutation_top_offset_flags_parse_and_run() {
    let temp = write_mutation_gate_fixture();
    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "-f",
            "json",
            "mutation",
            "--dry-run",
            "--top",
            "1",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Filtering: mutation routed through filtered_file_set (glob/exclude/changed-since)
// ---------------------------------------------------------------------------

#[test]
fn test_mutation_glob_and_exclude_still_work() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("keep.rs"),
        "pub fn keep(x: i32) -> bool { x > 0 }\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("skip.py"),
        "def skip(x):\n    return x > 0\n",
    )
    .unwrap();

    omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "mutation",
            "--dry-run",
            "-g",
            "*.rs",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Time window: --since / --days on churn
// ---------------------------------------------------------------------------

#[test]
fn test_churn_since_flag_runs_successfully() {
    omen()
        .args(["-p", ".", "-f", "json", "churn", "--since", "6m"])
        .assert()
        .success();
}

#[test]
fn test_churn_days_alias_still_runs_successfully() {
    omen()
        .args(["-p", ".", "-f", "json", "churn", "--days", "180"])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Command-name aliases: clones/duplicates, hotspot/hotspots
// ---------------------------------------------------------------------------

#[test]
fn test_duplicates_alias_runs_like_clones() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "duplicates"])
        .assert()
        .success();
}

#[test]
fn test_hotspots_alias_runs_like_hotspot() {
    omen()
        .args(["-p", fixtures_dir(), "-f", "json", "hotspots"])
        .assert()
        .success();
}
