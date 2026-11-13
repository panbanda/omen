#[cfg(feature = "mutation")]
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
            "--gate",
            "error",
            "--fail-under",
            "99",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("\"overall_score\""))
        .stderr(predicate::str::contains("is below minimum"));
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
        .args(["-p", fixtures_dir(), "-f", "json", "all"])
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
    assert!(files[0]["file"].as_str().unwrap().ends_with("src/b.rs"));
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

#[cfg(feature = "mutation")]
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
#[cfg(feature = "mutation")]
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
        let path = file["file"].as_str().expect("file path should be a string");
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
        let path = file["file"].as_str().expect("file path should be a string");
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
            "--gate",
            "error",
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
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "complexity",
            "--gate",
            "error",
        ])
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
        let path = file["file"].as_str().expect("file path should be a string");
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
            "--gate",
            "error",
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
        .args(["-p", fixtures_dir(), "-f", "json", "all"])
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
// Gate: --gate off|warn|error, exit codes
// ---------------------------------------------------------------------------
//
// Convention under test: off = report only (exit 0); warn = report + one-line
// stderr summary (exit 0); error = report; on violation return
// Error::ThresholdViolation, which main() maps to exit code 2.

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
        .stdout(predicate::str::contains("\"overall_score\""))
        .stderr(predicate::str::contains("is below minimum"));
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
fn test_score_default_gate_off_exits_zero_on_violation() {
    // No --gate: default GateMode::Off must report only and
    // exit 0, even though the score is well below --fail-under.
    omen()
        .args(["-p", fixtures_dir(), "score", "--fail-under", "99"])
        .assert()
        .success();
}

#[cfg(feature = "mutation")]
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

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_gate_error_exits_two_and_emits_report_before_failing() {
    // Regression guard: a naive gate implementation can return Err from
    // inside the analyzer itself (before the report is ever written), which
    // would make this test pass for the wrong reason -- exit 2 with no
    // report at all. Requesting JSON and asserting the stdout is valid,
    // non-empty mutation output proves the report reaches stdout *before*
    // the gate fails, matching complexity/score/stubs.
    let temp = write_mutation_gate_fixture();

    let output = omen()
        .args([
            "-p",
            temp.path().to_str().unwrap(),
            "-f",
            "json",
            "mutation",
            "--dry-run",
            "--gate",
            "error",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("mutation report should be valid JSON on stdout");
    assert!(
        parsed.get("summary").is_some(),
        "mutation report should include a summary: {parsed:#?}"
    );
    assert_eq!(
        parsed["summary"]["mutation_score"], 0.0,
        "dry-run mutation score should be 0.0: {parsed:#?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Mutation score"),
        "expected gate failure message on stderr: {stderr}"
    );
}

#[cfg(feature = "mutation")]
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

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_gate_off_default_exits_zero_on_dry_run() {
    let temp = write_mutation_gate_fixture();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "mutation", "--dry-run"])
        .assert()
        .success();
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

#[cfg(feature = "mutation")]
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
            "--offset",
            "0",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Filtering: mutation routed through filtered_file_set (glob/exclude/changed-since)
// ---------------------------------------------------------------------------

#[cfg(feature = "mutation")]
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

// ---------------------------------------------------------------------------
// Paired coding-agent evaluation gate
// ---------------------------------------------------------------------------

/// A complete, internally consistent evaluation in which every candidate
/// succeeds and every baseline fails.
fn eval_fixture(count: usize) -> (serde_json::Value, serde_json::Value) {
    use serde_json::json;

    let languages = ["rust", "go", "ruby", "typescript"];
    let cases: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            json!({
                "id": i.to_string(),
                "repository": format!("repo-{}", i % 5),
                "revision": "b".repeat(40),
                "language": languages[i % 4],
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
    for case in registration["cases"].as_array().expect("cases") {
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
    (registration, serde_json::Value::Array(runs))
}

fn write_eval_fixture(dir: &TempDir, count: usize) -> (String, String, String) {
    let (registration, runs) = eval_fixture(count);
    let registration_path = dir.path().join("registration.json");
    let runs_path = dir.path().join("runs.json");
    let output_path = dir.path().join("report.json");
    std::fs::write(
        &registration_path,
        serde_json::to_string(&registration).expect("registration"),
    )
    .expect("write registration");
    std::fs::write(&runs_path, serde_json::to_string(&runs).expect("runs")).expect("write runs");
    (
        registration_path.to_string_lossy().into_owned(),
        runs_path.to_string_lossy().into_owned(),
        output_path.to_string_lossy().into_owned(),
    )
}

#[test]
fn test_eval_passing_gate_writes_report_and_exits_zero() {
    let dir = TempDir::new().expect("temp dir");
    let (registration, runs, output) = write_eval_fixture(&dir, 30);

    omen()
        .args(["-f", "json", "eval"])
        .args(["--registration", &registration])
        .args(["--runs", &runs])
        .args(["--output", &output])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"reported_outcome_gate\": true"));

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output).expect("report written"))
            .expect("valid report json");
    assert_eq!(written["pairs"], 30);
    assert_eq!(written["outcomes_independently_verified"], false);
}

#[test]
fn test_eval_failing_gate_exits_two_after_writing_report() {
    let dir = TempDir::new().expect("temp dir");
    let (registration, runs, output) = write_eval_fixture(&dir, 4);

    omen()
        .args(["-f", "json", "eval"])
        .args(["--registration", &registration])
        .args(["--runs", &runs])
        .args(["--output", &output])
        .assert()
        .code(2);

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output).expect("report written"))
            .expect("valid report json");
    assert_eq!(written["adequate_held_out_sample"], false);
    assert_eq!(written["reported_outcome_gate"], false);
}

#[test]
fn test_eval_rejects_non_array_runs_document() {
    let dir = TempDir::new().expect("temp dir");
    let (registration, _, _) = write_eval_fixture(&dir, 30);
    let runs_path = dir.path().join("object-runs.json");
    std::fs::write(&runs_path, "{}").expect("write runs");

    omen()
        .args(["eval"])
        .args(["--registration", &registration])
        .args(["--runs", &runs_path.to_string_lossy()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("runs file must be a JSON array"));
}

// ---------------------------------------------------------------------------
// Paired quality benchmark
// ---------------------------------------------------------------------------

#[test]
fn test_benchmark_help_lists_paired_binaries() {
    omen()
        .args(["benchmark", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--baseline"))
        .stdout(predicate::str::contains("--candidate"));
}

#[test]
fn test_benchmark_rejects_too_few_repetitions_before_touching_binaries() {
    omen()
        .args(["benchmark"])
        .args(["--baseline", "/nonexistent/baseline"])
        .args(["--candidate", "/nonexistent/candidate"])
        .args(["--baseline-label", "a", "--candidate-label", "b"])
        .args(["--repetitions", "1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("repetitions >= 2"));
}

#[test]
fn test_benchmark_rejects_wrong_corpus_version() {
    let dir = TempDir::new().expect("temp dir");
    let corpus = dir.path().join("corpus.json");
    std::fs::write(&corpus, r#"{"version": 2, "cases": []}"#).expect("write corpus");

    omen()
        .args(["benchmark"])
        .args(["--baseline", "/nonexistent/baseline"])
        .args(["--candidate", "/nonexistent/candidate"])
        .args(["--baseline-label", "a", "--candidate-label", "b"])
        .args(["--corpus", &corpus.to_string_lossy()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("version 1 and nonempty cases"));
}

#[test]
fn test_benchmark_rejects_non_finite_timeout() {
    omen()
        .args(["benchmark"])
        .args(["--baseline", "/nonexistent/baseline"])
        .args(["--candidate", "/nonexistent/candidate"])
        .args(["--baseline-label", "a", "--candidate-label", "b"])
        .args(["--timeout", "inf"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("timeout"));
}

// ---------------------------------------------------------------------------
// Output schema: command names as keys, one `file` field
// ---------------------------------------------------------------------------

fn collect_keys(value: &serde_json::Value, keys: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::Object(entries) => {
            for (key, entry) in entries {
                keys.insert(key.clone());
                collect_keys(entry, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keys(item, keys);
            }
        }
        _ => {}
    }
}

#[test]
fn test_all_payload_names_the_clones_analyzer_by_its_command() {
    let output = omen()
        .args(["-p", fixtures_dir(), "-f", "json", "all"])
        .output()
        .expect("command runs");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<&str> = parsed["analyzers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["analyzer"].as_str())
        .collect();

    assert!(names.contains(&"clones"), "{names:?}");
    assert!(!names.contains(&"duplicates"), "{names:?}");
}

#[test]
fn test_file_records_use_only_the_file_key() {
    for command in [
        "complexity",
        "churn",
        "cohesion",
        "ownership",
        "graph",
        "defect",
        "tdg",
    ] {
        let output = omen()
            .args(["-p", ".", "-f", "json", command, "--glob", "src/cli/**"])
            .output()
            .expect("command runs");
        assert!(output.status.success(), "{command} failed");
        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut keys = std::collections::BTreeSet::new();
        collect_keys(&parsed, &mut keys);

        assert!(
            keys.contains("file"),
            "{command} has no `file` key: {keys:?}"
        );
        for removed in ["path", "file_path", "relative_path"] {
            assert!(!keys.contains(removed), "{command} still has `{removed}`");
        }
    }
}

// ---------------------------------------------------------------------------
// One CLI shape: gate from configuration, global flags, pagination, --output
// ---------------------------------------------------------------------------

#[test]
fn test_gate_mode_comes_from_configuration_and_the_flag_wins() {
    let temp = TempDir::new().unwrap();
    write_complexity_exclude_fixture(&temp);
    std::fs::write(
        temp.path().join("omen.toml"),
        "[complexity]\ncyclomatic_error = 3\n\n[gate]\ncomplexity = \"error\"\n",
    )
    .unwrap();
    let path = temp.path().to_str().unwrap();

    omen().args(["-p", path, "complexity"]).assert().code(2);
    omen()
        .args(["-p", path, "complexity", "--gate", "off"])
        .assert()
        .success();
}

#[test]
fn test_stubs_gate_mode_comes_from_configuration() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn later() -> u32 { todo!() }\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("omen.toml"), "[gate]\nstubs = \"error\"\n").unwrap();

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "stubs"])
        .assert()
        .code(2);
}

#[test]
fn test_global_flags_work_after_the_subcommand() {
    let output = omen()
        .args(["complexity", "-p", fixtures_dir(), "-f", "json", "-j", "2"])
        .output()
        .expect("command runs");

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed["files"].is_array());
}

#[test]
fn test_search_query_offset_skips_results() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        concat!(
            "pub fn alpha() { parse(); }\npub fn beta() { parse(); parse(); }\n",
            "pub fn gamma() { parse(); parse(); parse(); }\n",
            "pub fn render_one() {}\npub fn render_two() {}\npub fn write_one() {}\n",
            "pub fn write_two() {}\npub fn load_one() {}\n",
        ),
    )
    .unwrap();
    let path = temp.path().to_str().unwrap();
    omen()
        .args(["-p", path, "search", "index"])
        .assert()
        .success();

    let names = |extra: &[&str]| -> Vec<String> {
        let output = omen()
            .args(["-p", path, "-f", "json", "search", "query", "parse"])
            .args(["--min-score", "0"])
            .args(extra)
            .output()
            .expect("command runs");
        assert!(output.status.success());
        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        parsed["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["symbol_name"].as_str().unwrap().to_string())
            .collect()
    };

    let all = names(&[]);
    assert!(all.len() >= 2, "{all:?}");
    assert_eq!(names(&["--top", "1"]), all[..1]);
    assert_eq!(names(&["--top", "1", "--offset", "1"]), all[1..2]);
    assert_eq!(names(&["--offset", "1"]), all[1..]);
}

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_output_writes_the_report_file() {
    let temp = write_mutation_gate_fixture();
    let report = temp.path().join("mutation.json");

    omen()
        .args(["-p", temp.path().to_str().unwrap(), "mutation", "--dry-run"])
        .args(["--output", report.to_str().unwrap()])
        .assert()
        .success();

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).expect("report written")).unwrap();
    assert!(written.get("summary").is_some());
}

#[test]
fn test_report_generate_names_artifacts_by_command() {
    let source = TempDir::new().unwrap();
    std::fs::write(source.path().join("lib.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
    let data = TempDir::new().unwrap();

    omen()
        .args([
            "-p",
            source.path().to_str().unwrap(),
            "report",
            "generate",
            "--skip",
            "trend",
            "--data",
            data.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    for present in ["clones.json", "hotspot.json"] {
        assert!(data.path().join(present).exists(), "{present} is absent");
    }
    for absent in ["duplicates.json", "hotspots.json"] {
        assert!(!data.path().join(absent).exists(), "{absent} is present");
    }
}

// ---------------------------------------------------------------------------
// Review fixes: repository path check, gate scope, pagination by format
// ---------------------------------------------------------------------------

#[test]
fn test_absent_repository_path_gives_one_error_on_each_command() {
    for command in [
        vec!["complexity"],
        vec!["score"],
        vec!["churn"],
        vec!["search", "query", "helper"],
        vec!["all"],
        vec!["outline"],
        vec!["context"],
    ] {
        omen()
            .args(&command)
            .args(["-p", "/no/such/directory"])
            .timeout(std::time::Duration::from_secs(30))
            .assert()
            .code(1)
            .stderr(predicate::str::contains(
                "The repository path `/no/such/directory` is not a directory.",
            ));
    }
}

#[test]
fn test_repository_path_that_is_a_file_is_an_error() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("lib.rs");
    std::fs::write(&file, "fn one() {}\n").unwrap();

    omen()
        .args(["-p", file.to_str().unwrap(), "complexity"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("is not a directory"));
}

#[test]
fn test_score_trend_short_p_names_the_period_flag() {
    omen()
        .args(["score", "trend", "-p", "monthly"])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "The repository path `monthly` is not a directory.",
        ))
        .stderr(predicate::str::contains("--period"));
}

#[test]
fn test_report_serve_short_p_names_the_port_flag() {
    omen()
        .args(["report", "serve", "-p", "3000"])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "The repository path `3000` is not a directory.",
        ))
        .stderr(predicate::str::contains("--port"));
}

fn write_committed_repository() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
    for arguments in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(&arguments)
            .current_dir(temp.path())
            .output()
            .unwrap()
            .status
            .success());
    }
    temp
}

#[test]
fn test_configured_score_gate_does_not_replace_score_trend() {
    let temp = write_committed_repository();
    let config = temp.path().join("gate.toml");
    std::fs::write(
        &config,
        "[score]\nfail_under = 100.0\n\n[gate]\nscore = \"error\"\n",
    )
    .unwrap();
    let path = temp.path().to_str().unwrap();
    let config = config.to_str().unwrap();

    let output = omen()
        .args(["-p", path, "-c", config, "-f", "json", "score", "trend"])
        .args(["--since", "1y"])
        .output()
        .expect("command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed.get("points").is_some(), "{parsed:#?}");
    assert!(parsed.get("violations").is_none(), "{parsed:#?}");
}

#[test]
fn test_score_trend_refuses_the_gate_flag() {
    omen()
        .args(["-p", ".", "score", "--gate", "error", "trend"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("`score trend` has no gate"));
}

fn write_search_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        concat!(
            "pub fn alpha() { parse(); }\npub fn beta() { parse(); parse(); }\n",
            "pub fn gamma() { parse(); parse(); parse(); }\n",
            "pub fn render_one() {}\npub fn render_two() {}\npub fn write_one() {}\n",
            "pub fn write_two() {}\npub fn load_one() {}\n",
        ),
    )
    .unwrap();
    omen()
        .args(["-p", temp.path().to_str().unwrap(), "search", "index"])
        .assert()
        .success();
    temp
}

#[test]
fn test_search_query_sarif_keeps_each_finding() {
    let temp = write_search_fixture();
    let results = |extra: &[&str]| -> usize {
        let output = omen()
            .args(["-p", temp.path().to_str().unwrap(), "-f", "sarif"])
            .args(["search", "query", "parse", "--min-score", "0"])
            .args(extra)
            .output()
            .expect("command runs");
        assert!(output.status.success());
        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        parsed["runs"][0]["results"].as_array().unwrap().len()
    };

    let all = results(&["--top", "0"]);
    assert!(all >= 2, "{all}");
    assert_eq!(results(&["--top", "1"]), all);
    assert_eq!(results(&["--offset", "1"]), all);
}

#[test]
fn test_outline_markdown_applies_top_and_offset() {
    let temp = TempDir::new().unwrap();
    for name in ["alpha.rs", "beta.rs", "gamma.rs"] {
        std::fs::write(temp.path().join(name), "pub fn one() {}\n").unwrap();
    }
    for format in ["markdown", "text"] {
        let output = omen()
            .args(["-p", temp.path().to_str().unwrap(), "-f", format])
            .args(["outline", "--top", "1", "--offset", "1"])
            .output()
            .expect("command runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("alpha.rs"), "{format}: {stdout}");
        assert!(stdout.contains("beta.rs"), "{format}: {stdout}");
        assert!(!stdout.contains("gamma.rs"), "{format}: {stdout}");
    }
}

// ---------------------------------------------------------------------------
// Second review: path check scope, one meaning of --top in each format
// ---------------------------------------------------------------------------

#[test]
fn test_commands_without_a_repository_ignore_the_repository_path() {
    omen()
        .args(["-p", "/no/such/path", "mcp", "manifest"])
        .assert()
        .success();

    let dir = TempDir::new().expect("temp dir");
    let (registration, runs, _) = write_eval_fixture(&dir, 30);
    omen()
        .args(["-p", "/no/such/path", "-f", "json", "eval"])
        .args(["--registration", &registration])
        .args(["--runs", &runs])
        .assert()
        .success();

    omen()
        .args(["-p", "/no/such/path", "benchmark"])
        .args(["--baseline", "/nonexistent/baseline"])
        .args(["--candidate", "/nonexistent/candidate"])
        .args(["--baseline-label", "a", "--candidate-label", "b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("repository path").not());
}

/// The candidates that the output of `omen` contains for one format. The command
/// runs in the repository, because `mutation` reads its files from there.
fn listed<'a>(
    repository: &TempDir,
    format: &str,
    arguments: &[&str],
    candidates: &[&'a str],
) -> Vec<&'a str> {
    let output = omen()
        .current_dir(repository.path())
        .args(["-p", ".", "-f", format])
        .args(arguments)
        .output()
        .expect("command runs");
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    candidates
        .iter()
        .copied()
        .filter(|candidate| stdout.contains(candidate))
        .collect()
}

#[test]
fn test_outline_top_pages_the_files_list_in_each_format() {
    let temp = TempDir::new().unwrap();
    for name in ["alpha.rs", "beta.rs", "gamma.rs"] {
        std::fs::write(temp.path().join(name), "pub fn one() {}\n").unwrap();
    }
    let files = ["alpha.rs", "beta.rs", "gamma.rs"];

    for format in ["json", "markdown", "text"] {
        assert_eq!(
            listed(&temp, format, &["outline", "--top", "1"], &files),
            ["alpha.rs"],
            "{format}"
        );
        assert_eq!(
            listed(
                &temp,
                format,
                &["outline", "--top", "1", "--offset", "1"],
                &files
            ),
            ["beta.rs"],
            "{format}"
        );
    }

    let output = omen()
        .args(["-p", temp.path().to_str().unwrap(), "-f", "json"])
        .args(["outline", "--top", "1"])
        .output()
        .expect("command runs");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["files"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["files_omitted"], 2);
}

fn write_callers_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        concat!(
            "pub fn target() -> u32 { 1 }\n",
            "pub fn first_caller() -> u32 { target() }\n",
            "pub fn second_caller() -> u32 { target() }\n",
            "pub fn third_caller() -> u32 { target() }\n",
        ),
    )
    .unwrap();
    temp
}

#[test]
fn test_top_lists_the_same_items_in_json_and_markdown() {
    let callers = ["first_caller", "second_caller", "third_caller"];
    let repository = write_callers_fixture();
    for arguments in [
        vec!["symbol", "target", "--no-source", "--top", "1"],
        vec!["impact", "target", "--top", "1"],
    ] {
        let json = listed(&repository, "json", &arguments, &callers);
        assert_eq!(json.len(), 1, "{arguments:?}: {json:?}");
        assert_eq!(
            listed(&repository, "markdown", &arguments, &callers),
            json,
            "{arguments:?}"
        );
    }

    let repository = write_search_fixture();
    let names = ["alpha", "beta", "gamma"];
    let arguments = ["search", "query", "parse", "--min-score", "0", "--top", "1"];
    let json = listed(&repository, "json", &arguments, &names);
    assert_eq!(json.len(), 1, "{json:?}");
    assert_eq!(listed(&repository, "markdown", &arguments, &names), json);
}

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_top_lists_the_same_files_in_json_and_markdown() {
    let repository = TempDir::new().unwrap();
    for name in ["one.rs", "two.rs"] {
        std::fs::write(
            repository.path().join(name),
            "pub fn is_positive(x: i32) -> bool { x > 0 }\n",
        )
        .unwrap();
    }
    let files = ["one.rs", "two.rs"];
    let arguments = ["mutation", "--test-command", "true", "--top", "1"];

    let json = listed(&repository, "json", &arguments, &files);
    assert_eq!(json.len(), 1, "{json:?}");
    assert_eq!(listed(&repository, "markdown", &arguments, &files), json);
}

// ---------------------------------------------------------------------------
// The working directory does not change the output
// ---------------------------------------------------------------------------

/// A committed repository with calls, a class, and a stub.
fn write_path_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        concat!(
            "pub struct Counter { count: u32 }\n",
            "impl Counter {\n",
            "    pub fn bump(&mut self) -> u32 { self.count += 1; self.count }\n",
            "}\n",
            "pub fn target(x: i32) -> bool { x > 0 }\n",
            "pub fn first_caller() -> bool { target(1) }\n",
            "// TODO: remove this function\n",
            "pub fn later() -> u32 { todo!() }\n",
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/other.rs"),
        "pub fn second_caller(y: i32) -> bool { if y > 2 { true } else { y + 1 > 0 } }\n",
    )
    .unwrap();
    for arguments in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(&arguments)
            .current_dir(temp.path())
            .output()
            .unwrap()
            .status
            .success());
    }
    temp
}

/// Removes the values that change between two runs, and makes each path
/// relative to the repository.
fn normalise(value: &mut serde_json::Value, roots: &[String]) {
    match value {
        serde_json::Value::Object(entries) => {
            // `repository_root` of `churn` repeats the `-p` value.
            for key in ["generated_at", "duration_ms", "repository_root"] {
                entries.remove(key);
            }
            for entry in entries.values_mut() {
                normalise(entry, roots);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                normalise(item, roots);
            }
        }
        serde_json::Value::String(text) => {
            for root in roots {
                *text = text.replace(&format!("{root}/"), "");
            }
            if let Some(stripped) = text.strip_prefix("./") {
                *text = stripped.to_string();
            }
        }
        _ => {}
    }
}

/// Runs one command in `working_directory` and gives its status and its
/// normalised JSON output.
fn run_normalised(
    working_directory: &std::path::Path,
    path: &str,
    arguments: &[&str],
    roots: &[String],
) -> (Option<i32>, serde_json::Value) {
    let output = omen()
        .current_dir(working_directory)
        .args(["-p", path, "-f", "json"])
        .args(arguments)
        .output()
        .expect("command runs");
    let mut parsed = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    normalise(&mut parsed, roots);
    (output.status.code(), parsed)
}

fn repository_roots(repository: &TempDir) -> Vec<String> {
    let plain = repository.path().to_str().unwrap().to_string();
    let canonical = repository
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    vec![canonical, plain]
}

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_gives_the_same_mutants_from_each_working_directory() {
    let repository = write_path_fixture();
    let elsewhere = TempDir::new().unwrap();
    let roots = repository_roots(&repository);
    let path = repository.path().to_str().unwrap();

    let inside = run_normalised(repository.path(), ".", &["mutation", "--dry-run"], &roots);
    let outside = run_normalised(elsewhere.path(), path, &["mutation", "--dry-run"], &roots);

    assert!(
        inside.1["summary"]["total_mutants"].as_u64().unwrap() > 0,
        "{:#?}",
        inside.1
    );
    assert_eq!(inside, outside);
}

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_runs_and_records_in_the_repository_from_another_directory() {
    let repository = write_path_fixture();
    let elsewhere = TempDir::new().unwrap();
    let marker = "working-directory-marker";

    let output = omen()
        .current_dir(elsewhere.path())
        .args(["-p", repository.path().to_str().unwrap(), "-f", "json"])
        .args(["mutation", "--record", "--test-command"])
        .arg(format!("touch {marker}"))
        .output()
        .expect("command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(
        parsed["summary"]["survived"].as_u64().unwrap() > 0,
        "{parsed:#?}"
    );
    assert!(repository.path().join(marker).exists());
    assert!(!elsewhere.path().join(marker).exists());
    let history =
        std::fs::read_to_string(repository.path().join(".omen/mutation-history.jsonl")).unwrap();
    assert!(history.contains("source_context"));
    assert!(
        history.contains("x > 0"),
        "the record has no source context"
    );
    assert!(!elsewhere.path().join(".omen").exists());
    // The run restores each file.
    assert!(std::process::Command::new("git")
        .args(["diff", "--quiet", "--", "src"])
        .current_dir(repository.path())
        .status()
        .unwrap()
        .success());
}

#[test]
fn test_each_repository_command_gives_the_same_output_from_each_working_directory() {
    let repository = write_path_fixture();
    let elsewhere = TempDir::new().unwrap();
    let roots = repository_roots(&repository);
    let path = repository.path().to_str().unwrap();
    // `search query` reads the index that `search index` writes.
    omen()
        .args(["-p", path, "search", "index"])
        .assert()
        .success();

    let commands: &[&[&str]] = &[
        &["complexity"],
        &["satd"],
        &["stubs"],
        &["deadcode"],
        &["churn"],
        &["clones"],
        &["defect"],
        &["changes"],
        &["diff"],
        &["tdg"],
        &["graph"],
        &["hotspot"],
        &["temporal"],
        &["ownership"],
        &["cohesion"],
        &["repomap"],
        &["smells"],
        &["flags"],
        &["score"],
        &["score", "trend"],
        &["all"],
        &["context"],
        &["outline"],
        &["outline", "--file", "src/lib.rs"],
        &["impact", "target"],
        &["symbol", "target"],
        &["search", "query", "target", "--min-score", "0"],
        #[cfg(feature = "mutation")]
        &["mutation", "--dry-run"],
    ];

    let mut differences = Vec::new();
    for arguments in commands {
        let inside = run_normalised(repository.path(), ".", arguments, &roots);
        let outside = run_normalised(elsewhere.path(), path, arguments, &roots);
        if inside != outside {
            differences.push(format!(
                "{arguments:?}\n  inside:  {} {}\n  outside: {} {}",
                inside.0.unwrap_or(-1),
                inside.1,
                outside.0.unwrap_or(-1),
                outside.1
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn test_report_generate_gives_the_same_artifacts_from_each_working_directory() {
    let repository = write_path_fixture();
    let elsewhere = TempDir::new().unwrap();
    let roots = repository_roots(&repository);
    let inside_data = TempDir::new().unwrap();
    let outside_data = TempDir::new().unwrap();

    for (working_directory, path, data) in [
        (repository.path(), ".", &inside_data),
        (
            elsewhere.path(),
            repository.path().to_str().unwrap(),
            &outside_data,
        ),
    ] {
        omen()
            .current_dir(working_directory)
            .args(["-p", path, "report", "generate", "--skip", "trend"])
            .args(["--data", data.path().to_str().unwrap()])
            .assert()
            .success();
    }

    let mut names: Vec<_> = std::fs::read_dir(inside_data.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    let mut differences = Vec::new();
    for name in names {
        // `metadata.json` records the `-p` value and the time.
        if name == "metadata.json" {
            continue;
        }
        let read = |data: &TempDir| -> serde_json::Value {
            let mut parsed = std::fs::read_to_string(data.path().join(&name))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(serde_json::Value::Null);
            normalise(&mut parsed, &roots);
            parsed
        };
        if read(&inside_data) != read(&outside_data) {
            differences.push(name.to_string_lossy().into_owned());
        }
    }
    assert!(differences.is_empty(), "{differences:?}");
}

// ---------------------------------------------------------------------------
// `all` obeys the output format
// ---------------------------------------------------------------------------

fn all_output(arguments: &[&str]) -> String {
    let output = omen()
        .args(["-p", fixtures_dir()])
        .args(arguments)
        .output()
        .expect("command runs");
    assert!(output.status.success(), "{arguments:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn test_all_gives_markdown_and_text_through_the_shared_formats() {
    for format in ["markdown", "text"] {
        let stdout = all_output(&["-f", format, "all"]);
        assert!(
            serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
            "`all -f {format}` gave JSON"
        );
        for section in ["Complexity", "Satd", "Clones", "Score"] {
            assert!(
                stdout.contains(section),
                "{format} has no {section} section"
            );
        }
    }
    assert!(all_output(&["-f", "markdown", "all"]).contains("# Complexity"));
    // Markdown is the default format of each command.
    assert!(all_output(&["all"]).contains("# Complexity"));
}

#[test]
fn test_all_gives_sarif() {
    let parsed: serde_json::Value =
        serde_json::from_str(&all_output(&["-f", "sarif", "all"])).unwrap();
    assert_eq!(parsed["version"], "2.1.0");
    assert!(!parsed["runs"][0]["results"].as_array().unwrap().is_empty());
}

#[test]
fn test_all_applies_top_to_each_section_as_the_single_command_does() {
    let all: serde_json::Value =
        serde_json::from_str(&all_output(&["-f", "json", "all", "--top", "1"])).unwrap();
    let sections = all["analyzers"].as_array().unwrap();
    assert_eq!(sections.len(), 18, "`--top` must not remove a section");

    for command in ["satd", "stubs", "complexity"] {
        let single: serde_json::Value =
            serde_json::from_str(&all_output(&["-f", "json", command, "--top", "1"])).unwrap();
        let section = sections
            .iter()
            .find(|entry| entry["analyzer"] == command)
            .unwrap();
        assert_eq!(section["result"], single, "{command}");
    }

    let markdown = all_output(&["-f", "markdown", "all", "--top", "1"]);
    let full = all_output(&["-f", "markdown", "all"]);
    assert!(markdown.len() < full.len());
}

// ---------------------------------------------------------------------------
// A gate does not change the report on stdout
// ---------------------------------------------------------------------------

/// One gated command with a fixture that has a violation.
struct GateCase {
    command: &'static str,
    arguments: &'static [&'static str],
    /// The configuration that makes the fixture a violation.
    thresholds: &'static str,
    source: &'static str,
}

const GATE_CASES: &[GateCase] = &[
    GateCase {
        command: "complexity",
        arguments: &["complexity"],
        thresholds: "[complexity]\ncyclomatic_error = 1\n",
        source:
            "pub fn branchy(a: bool, b: bool) -> u32 { if a { 1 } else if b { 2 } else { 3 } }\n",
    },
    GateCase {
        command: "score",
        arguments: &["score"],
        thresholds: "[score]\nfail_under = 100.0\n",
        source:
            "// TODO: remove this\npub fn later(a: bool) -> u32 { if a { todo!() } else { 1 } }\n",
    },
    GateCase {
        command: "stubs",
        arguments: &["stubs"],
        thresholds: "",
        source: "pub fn later() -> u32 { todo!() }\n",
    },
    #[cfg(feature = "mutation")]
    GateCase {
        command: "mutation",
        arguments: &["mutation", "--dry-run"],
        thresholds: "",
        source: "pub fn is_positive(x: i32) -> bool { x > 0 }\n",
    },
];

/// Removes the one value of a report that changes between two runs.
fn without_duration(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| !line.contains("duration_ms") && !line.contains("Duration"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_gate_keeps_the_report_and_writes_the_violation_to_stderr() {
    for case in GATE_CASES {
        let repository = TempDir::new().unwrap();
        std::fs::write(repository.path().join("lib.rs"), case.source).unwrap();
        let configuration = TempDir::new().unwrap();
        let write_configuration = |name: &str, gate: &str| -> String {
            let file = configuration.path().join(name);
            std::fs::write(&file, format!("{}{gate}", case.thresholds)).unwrap();
            file.to_str().unwrap().to_string()
        };
        let plain = write_configuration("plain.toml", "");
        let run = |format: &str, config: &str, extra: &[&str]| {
            omen()
                .args(["-p", repository.path().to_str().unwrap()])
                .args(["-f", format, "-c", config])
                .args(case.arguments)
                .args(extra)
                .output()
                .expect("command runs")
        };

        for format in ["json", "markdown", "text"] {
            let off = run(format, &plain, &[]);
            assert_eq!(off.status.code(), Some(0), "{} off", case.command);
            let report = without_duration(&off.stdout);
            assert!(
                !report.is_empty(),
                "{} {format} has no report",
                case.command
            );

            for (mode, status) in [("warn", 0), ("error", 2)] {
                let from_config = write_configuration(
                    &format!("{mode}.toml"),
                    &format!("\n[gate]\n{} = \"{mode}\"\n", case.command),
                );
                for (source, output) in [
                    ("flag", run(format, &plain, &["--gate", mode])),
                    ("configuration", run(format, &from_config, &[])),
                ] {
                    let label = format!("{} {format} {mode} from the {source}", case.command);
                    assert_eq!(output.status.code(), Some(status), "{label}");
                    assert_eq!(without_duration(&output.stdout), report, "{label}");
                    assert!(
                        output.stderr.len() > off.stderr.len(),
                        "{label}: stderr has no violation"
                    );
                }
            }

            let off_flag = run(
                format,
                &write_configuration(
                    "error.toml",
                    &format!("\n[gate]\n{} = \"error\"\n", case.command),
                ),
                &["--gate", "off"],
            );
            assert_eq!(
                off_flag.status.code(),
                Some(0),
                "{} --gate off",
                case.command
            );
            assert_eq!(without_duration(&off_flag.stdout), report);
        }
    }
}

// ---------------------------------------------------------------------------
// `--top` lists the same items in each format
// ---------------------------------------------------------------------------

fn write_import_chain_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    for (name, source) in [
        (
            "alpha.ts",
            "import { two } from './beta';\nexport function one() { return two(); }\n",
        ),
        (
            "beta.ts",
            "import { three } from './gamma';\nexport function two() { return three(); }\n",
        ),
        ("gamma.ts", "export function three() { return 3; }\n"),
        ("delta.ts", "export function four() { return 4; }\n"),
    ] {
        std::fs::write(temp.path().join(name), source).unwrap();
    }
    temp
}

#[test]
fn test_graph_top_pages_the_nodes_in_each_format() {
    let repository = write_import_chain_fixture();
    let run = |format: &str, extra: &[&str]| -> String {
        let output = omen()
            .args(["-p", repository.path().to_str().unwrap()])
            .args(["-f", format, "graph"])
            .args(extra)
            .output()
            .expect("command runs");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    let full: serde_json::Value = serde_json::from_str(&run("json", &[])).unwrap();
    let nodes: Vec<String> = full["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["file"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(nodes.len(), 4, "{full:#?}");
    assert!(full["cycles"].as_array().unwrap().is_empty());
    assert_eq!(full["edges"].as_array().unwrap().len(), 2, "{full:#?}");

    let paged: serde_json::Value =
        serde_json::from_str(&run("json", &["--top", "2", "--offset", "1"])).unwrap();
    let kept: Vec<&str> = paged["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["file"].as_str().unwrap())
        .collect();
    assert_eq!(kept, [nodes[1].as_str(), nodes[2].as_str()]);
    assert_eq!(paged["nodes_omitted"], 1);
    let edges = paged["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1, "{paged:#?}");
    for edge in edges {
        assert!(kept.contains(&edge["from"].as_str().unwrap()), "{edge}");
        assert!(kept.contains(&edge["to"].as_str().unwrap()), "{edge}");
    }
    assert_eq!(paged["edges_omitted"], 1);

    for format in ["markdown", "text"] {
        let stdout = run(format, &["--top", "1"]);
        let listed: Vec<&String> = nodes.iter().filter(|node| stdout.contains(*node)).collect();
        assert_eq!(listed, [&nodes[0]], "{format}: {stdout}");
    }
}

/// Collects the string values of a report. With `key`, it collects only the
/// values of that key.
fn collect_strings(
    value: &serde_json::Value,
    key: Option<&str>,
    strings: &mut std::collections::BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(entries) => {
            for (name, entry) in entries {
                match (key, entry) {
                    (Some(key), serde_json::Value::String(text)) if key == name => {
                        strings.insert(text.clone());
                    }
                    (Some(_), serde_json::Value::String(_)) => {}
                    _ => collect_strings(entry, key, strings),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, key, strings);
            }
        }
        serde_json::Value::String(text) if key.is_none() => {
            strings.insert(text.clone());
        }
        _ => {}
    }
}

/// The words of a Markdown report, without the table and emphasis marks.
fn markdown_words(markdown: &str) -> std::collections::BTreeSet<String> {
    markdown
        .split(|character: char| character.is_whitespace() || character == '|')
        .map(|word| {
            word.trim_matches(|character| matches!(character, '*' | '`' | ',' | ':'))
                .to_string()
        })
        .collect()
}

#[test]
fn test_top_lists_the_same_items_in_json_and_markdown_for_each_command() {
    let repository = write_path_fixture();
    let path = repository.path().to_str().unwrap();
    omen()
        .args(["-p", path, "search", "index"])
        .assert()
        .success();

    // The second value is the key that names an item. A command with its own
    // Markdown renderer prints a summary of each item, so the test compares
    // that key only.
    let commands: &[(&[&str], Option<&str>)] = &[
        (&["complexity"], None),
        (&["satd"], None),
        (&["stubs"], None),
        (&["deadcode"], None),
        (&["churn"], None),
        (&["clones"], None),
        (&["defect"], None),
        (&["changes"], None),
        (&["tdg"], None),
        (&["graph"], None),
        (&["hotspot"], None),
        (&["temporal"], None),
        (&["ownership"], None),
        (&["cohesion"], None),
        (&["repomap"], None),
        (&["smells"], None),
        (&["flags"], None),
        (&["outline"], Some("file")),
        (&["impact", "target"], None),
        (&["symbol", "target", "--no-source"], None),
        (
            &["search", "query", "target", "--min-score", "0"],
            Some("symbol_name"),
        ),
        #[cfg(feature = "mutation")]
        (&["mutation", "--dry-run"], Some("file")),
    ];

    let run = |format: &str, arguments: &[&str], extra: &[&str]| -> String {
        let output = omen()
            .args(["-p", path, "-f", format])
            .args(arguments)
            .args(extra)
            .output()
            .expect("command runs");
        assert!(output.status.success(), "{arguments:?}");
        String::from_utf8(output.stdout).unwrap()
    };

    let mut differences = Vec::new();
    for (arguments, item_key) in commands {
        // A candidate is a value of the full report that is one word, such as
        // a file or a symbol name.
        let full: serde_json::Value = serde_json::from_str(&run("json", arguments, &[])).unwrap();
        let mut candidates = std::collections::BTreeSet::new();
        collect_strings(&full, *item_key, &mut candidates);
        candidates.retain(|text| {
            text.len() >= 4
                && !text.contains(|character: char| character.is_whitespace() || character == '|')
        });

        let paged: serde_json::Value =
            serde_json::from_str(&run("json", arguments, &["--top", "1"])).unwrap();
        let mut json_strings = std::collections::BTreeSet::new();
        collect_strings(&paged, *item_key, &mut json_strings);
        let words = markdown_words(&run("markdown", arguments, &["--top", "1"]));

        let in_json: Vec<&String> = candidates.intersection(&json_strings).collect();
        let in_markdown: Vec<&String> = candidates.intersection(&words).collect();
        if in_json != in_markdown {
            differences.push(format!(
                "{arguments:?}\n  json:     {in_json:?}\n  markdown: {in_markdown:?}"
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

// ---------------------------------------------------------------------------
// A test command cannot change a source file after the mutation run
// ---------------------------------------------------------------------------

/// Runs `mutation` with a test command that starts a late writer, and gives
/// the content of the source file after the writer had time to run.
#[cfg(unix)]
#[cfg(feature = "mutation")]
fn source_after_late_writer(test_command: &str, writer_delay: std::time::Duration) -> String {
    let repository = TempDir::new().unwrap();
    std::fs::create_dir(repository.path().join("src")).unwrap();
    let source = repository.path().join("src/lib.rs");
    std::fs::write(&source, LATE_WRITER_SOURCE).unwrap();

    omen()
        .args(["-p", repository.path().to_str().unwrap(), "mutation"])
        .args(["--allow-dirty", "--operators", "CRR", "--timeout", "1"])
        .args(["--test-command", test_command])
        .assert()
        .success();

    std::thread::sleep(writer_delay);
    std::fs::read_to_string(source).unwrap()
}

#[cfg(unix)]
#[cfg(feature = "mutation")]
const LATE_WRITER_SOURCE: &str = "pub fn value() -> i32 { 1 }\n";

#[cfg(unix)]
#[cfg(feature = "mutation")]
#[test]
fn test_mutation_timeout_stops_each_process_of_the_test_command() {
    let source = source_after_late_writer(
        "(sleep 2; printf corrupt > src/lib.rs) & wait",
        std::time::Duration::from_millis(2500),
    );
    assert_eq!(source, LATE_WRITER_SOURCE);
}

#[cfg(unix)]
#[cfg(feature = "mutation")]
#[test]
fn test_mutation_stops_a_writer_that_the_test_command_leaves() {
    let source = source_after_late_writer(
        "(sleep 1; printf corrupt > src/lib.rs) > /dev/null 2>&1 & exit 0",
        std::time::Duration::from_millis(1500),
    );
    assert_eq!(source, LATE_WRITER_SOURCE);
}

// ---------------------------------------------------------------------------
// Mutation path options: the command line is relative to the working
// directory, and a default is relative to the repository
// ---------------------------------------------------------------------------

#[cfg(feature = "mutation")]
#[test]
fn test_mutation_path_options_follow_one_rule_from_another_directory() {
    let repository = write_path_fixture();
    let elsewhere = TempDir::new().unwrap();
    let path = repository.path().to_str().unwrap();
    let run = |arguments: &[&str]| {
        let output = omen()
            .current_dir(elsewhere.path())
            .args(["-p", path, "-f", "json", "mutation"])
            .args(arguments)
            .output()
            .expect("command runs");
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    };

    // A default path is in the repository.
    run(&["--record", "--test-command", "true"]);
    let default_history = repository.path().join(".omen/mutation-history.jsonl");
    assert!(default_history.exists());
    run(&["train"]);
    assert!(repository.path().join(".omen/mutation-model.json").exists());
    assert!(!elsewhere.path().join(".omen").exists());

    // A relative path on the command line is in the working directory.
    std::fs::copy(&default_history, elsewhere.path().join("history.jsonl")).unwrap();
    run(&[
        "train",
        "--history",
        "history.jsonl",
        "--model",
        "model.json",
    ]);
    assert!(elsewhere.path().join("model.json").exists());
    assert!(!repository.path().join("model.json").exists());

    let stderr = run(&[
        "--dry-run",
        "--skip-predicted",
        "0.9",
        "--model",
        "model.json",
    ]);
    assert!(!stderr.contains("No trained model found"), "{stderr}");
    run(&["--dry-run", "--output", "report.json"]);
    assert!(elsewhere.path().join("report.json").exists());
    assert!(!repository.path().join("report.json").exists());
}

// ---------------------------------------------------------------------------
// Cycles and smells have one order
// ---------------------------------------------------------------------------

/// Two independent cycles: `a` and `b`, and `c` and `d`.
fn write_two_cycles_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    for (name, other) in [("a", "b"), ("b", "a"), ("c", "d"), ("d", "c")] {
        std::fs::write(
            temp.path().join(format!("{name}.ts")),
            format!(
                "import {{ {other} }} from './{other}';\nexport function {name}() {{ return {other}; }}\n"
            ),
        )
        .unwrap();
    }
    temp
}

fn two_cycles() -> serde_json::Value {
    serde_json::json!([["a.ts", "b.ts"], ["c.ts", "d.ts"]])
}

fn cyclic_smell_members(smells: &serde_json::Value) -> serde_json::Value {
    smells["smells"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|smell| smell["smell_type"] == "CyclicDependency")
        .map(|smell| smell["components"].clone())
        .collect()
}

fn json_report(repository: &TempDir, command: &str) -> serde_json::Value {
    let output = omen()
        .args(["-p", repository.path().to_str().unwrap()])
        .args(["-f", "json", command])
        .output()
        .expect("command runs");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn test_smells_of_equal_severity_have_one_order() {
    let repository = write_two_cycles_fixture();
    // The order came from the graph traversal, so one run is not proof.
    for _ in 0..3 {
        assert_eq!(
            cyclic_smell_members(&json_report(&repository, "smells")),
            two_cycles()
        );
    }
}

#[test]
fn test_cycles_have_one_order_in_graph_smells_and_the_report() {
    let repository = write_two_cycles_fixture();
    for _ in 0..3 {
        assert_eq!(json_report(&repository, "graph")["cycles"], two_cycles());
    }
    assert_eq!(
        cyclic_smell_members(&json_report(&repository, "smells")),
        two_cycles()
    );

    let data = TempDir::new().unwrap();
    omen()
        .args(["-p", repository.path().to_str().unwrap()])
        .args(["report", "generate", "--skip", "trend"])
        .args(["--data", data.path().to_str().unwrap()])
        .assert()
        .success();
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(data.path().join(name)).unwrap()).unwrap()
    };
    assert_eq!(read("graph.json")["cycles"], two_cycles());
    assert_eq!(cyclic_smell_members(&read("smells.json")), two_cycles());
}

#[test]
fn test_graph_keeps_the_node_order_with_cycles() {
    let repository = write_two_cycles_fixture();
    let nodes: Vec<String> = json_report(&repository, "graph")["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["file"].as_str().unwrap().to_string())
        .collect();
    // Each node has the same PageRank, so the file name gives the order.
    assert_eq!(nodes, ["a.ts", "b.ts", "c.ts", "d.ts"]);
}

// ---------------------------------------------------------------------------
// A time window that is not valid is an error, not all the history
// ---------------------------------------------------------------------------

#[test]
fn test_churn_rejects_an_invalid_since_flag() {
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "-f",
            "json",
            "churn",
            "--since",
            "nonsense",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--since"))
        .stderr(predicate::str::contains("\"nonsense\""));
}

#[test]
fn test_report_generate_rejects_an_invalid_since_flag() {
    let data = TempDir::new().unwrap();
    omen()
        .args([
            "-p",
            fixtures_dir(),
            "report",
            "generate",
            "--since",
            "nonsense",
            "--data",
        ])
        .arg(data.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--since"))
        .stderr(predicate::str::contains("\"nonsense\""));
}

#[test]
fn test_an_invalid_configured_churn_window_is_rejected() {
    let directory = TempDir::new().unwrap();
    std::fs::write(directory.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        directory.path().join("omen.toml"),
        "[churn]\nsince = \"nonsense\"\n",
    )
    .unwrap();
    for command in ["churn", "complexity"] {
        omen()
            .arg("-p")
            .arg(directory.path())
            .args(["-f", "json", command])
            .assert()
            .failure()
            .stderr(predicate::str::contains("churn.since"))
            .stderr(predicate::str::contains("\"nonsense\""));
    }
}
