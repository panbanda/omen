//! The pull-request comment of `omen diff -f markdown`, on a fixture
//! repository that has two commits.

#[path = "support/fixture.rs"]
mod fixture;

use std::path::Path;

use omen::changeset::markdown::{render, CommentOptions};
use omen::changeset::model::{ChangeSet, ChangeState, FindingKind};
use omen::config::Config;
use tempfile::TempDir;

const SHAPES_BASE: &str = r#"pub fn area(width: u32, height: u32) -> u32 {
    width * height
}

pub fn perimeter(width: u32, height: u32) -> u32 {
    2 * (width + height)
}

pub fn legacy(value: u32) -> u32 {
    value
}
"#;

const SHAPES_HEAD: &str = r#"pub fn area(width: u32, height: u32, scale: u32) -> u32 {
    if scale == 0 {
        return 0;
    }
    if width > 100 && height > 100 {
        return width * height * scale / 2;
    }
    width * height * scale
}

pub fn perimeter(width: u32, height: u32) -> u32 {
    2 * (width + height)
}

// TODO: replace this estimate
pub fn volume(width: u32, height: u32, depth: u32) -> u32 {
    area(width, height, 1) * depth
}

pub fn unfinished() -> u32 {
    todo!()
}
"#;

const REPORT_BASE: &str = r#"use crate::shapes::{area, legacy, perimeter};

pub fn describe(width: u32, height: u32) -> u32 {
    area(width, height) + legacy(width)
}

pub fn outline(width: u32, height: u32) -> u32 {
    perimeter(width, height)
}
"#;

const REPORT_HEAD: &str = r#"use crate::shapes::{area, perimeter, volume};

pub fn describe(width: u32, height: u32) -> u32 {
    area(width, height, 2)
}

pub fn outline(width: u32, height: u32) -> u32 {
    perimeter(width, height)
}

pub fn solid(width: u32, height: u32) -> u32 {
    volume(width, height, 3)
}
"#;

const MANIFEST: &str = "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

fn tests_module(call: &str) -> String {
    format!(
        "\n#[cfg(test)]\nmod tests {{\n    use super::*;\n\n    #[test]\n    fn test_area() {{\n        assert_eq!({call}, 6);\n    }}\n}}\n"
    )
}

fn write(repository: &Path, file: &str, text: &str) {
    let path = repository.join(file);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, text).expect("write file");
}

/// A repository with `main` and a `feature` branch. `HEAD` is `feature`.
fn repository() -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path();
    let first = "2024-02-01T12:00:00Z";
    fixture::git(path, "ada", first, &["init", "--initial-branch", "main"]);
    write(path, "Cargo.toml", MANIFEST);
    write(path, "src/lib.rs", "pub mod report;\npub mod shapes;\n");
    write(
        path,
        "src/shapes.rs",
        &format!("{SHAPES_BASE}{}", tests_module("area(2, 3)")),
    );
    write(path, "src/report.rs", REPORT_BASE);
    fixture::commit(path, "ada", first, "feat: add the shapes");

    let second = "2024-02-08T12:00:00Z";
    fixture::git(path, "grace", second, &["checkout", "-b", "feature"]);
    write(
        path,
        "src/shapes.rs",
        &format!("{SHAPES_HEAD}{}", tests_module("area(2, 3, 1)")),
    );
    write(path, "src/report.rs", REPORT_HEAD);
    fixture::commit(path, "grace", second, "feat: scale the area");
    directory
}

fn markdown_omen(repository: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_omen"));
    command
        .arg("-p")
        .arg(repository)
        .args(["-f", "markdown"])
        .current_dir(repository);
    command
}

fn changes(repository: &Path) -> ChangeSet {
    omen::changeset::build::compute(repository, Some("main"), &Config::default())
        .expect("change set")
}

#[test]
fn test_symbols_have_the_state_that_the_two_indexes_give() {
    let directory = repository();
    let changes = changes(directory.path());
    let state = |key: &str| {
        changes
            .symbols
            .iter()
            .find(|symbol| symbol.key == key)
            .map(|symbol| symbol.state)
    };
    assert_eq!(state("src/shapes.rs#area"), Some(ChangeState::Changed));
    assert_eq!(state("src/shapes.rs#volume"), Some(ChangeState::Added));
    assert_eq!(state("src/shapes.rs#unfinished"), Some(ChangeState::Added));
    assert_eq!(state("src/shapes.rs#legacy"), Some(ChangeState::Removed));
    assert_eq!(state("src/report.rs#describe"), Some(ChangeState::Changed));
    assert_eq!(state("src/shapes.rs#perimeter"), None);
    assert_eq!(state("src/report.rs#outline"), None);
}

#[test]
fn test_changed_symbol_has_the_complexity_before_and_after() {
    let directory = repository();
    let changes = changes(directory.path());
    let area = changes
        .symbols
        .iter()
        .find(|symbol| symbol.key == "src/shapes.rs#area")
        .expect("area");
    assert_eq!(area.complexity_before.map(|c| c.cyclomatic), Some(1));
    assert_eq!(area.complexity_after.map(|c| c.cyclomatic), Some(4));
    assert!(area.signature_changed());
}

#[test]
fn test_changed_symbol_has_callers_and_tests() {
    let directory = repository();
    let changes = changes(directory.path());
    let area = changes
        .symbols
        .iter()
        .find(|symbol| symbol.key == "src/shapes.rs#area")
        .expect("area");
    let callers: Vec<&str> = area
        .direct_callers
        .iter()
        .map(|caller| caller.key.as_str())
        .collect();
    assert!(callers.contains(&"src/report.rs#describe"), "{callers:?}");
    assert!(callers.contains(&"src/shapes.rs#volume"), "{callers:?}");
    assert!(area.test_callers() >= 1, "{callers:?}");
    assert!(area.transitive_callers >= area.direct_callers.len());
    assert!(area.weakest_basis.is_some());
}

#[test]
fn test_graph_marks_an_added_call_and_a_removed_call() {
    let directory = repository();
    let changes = changes(directory.path());
    let edge_state = |from: &str, to: &str| {
        changes
            .graph
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .map(|edge| edge.state)
    };
    assert_eq!(
        edge_state("src/shapes.rs#volume", "src/shapes.rs#area"),
        Some(ChangeState::Added)
    );
    assert_eq!(
        edge_state("src/report.rs#describe", "src/shapes.rs#legacy"),
        Some(ChangeState::Removed)
    );
    assert_eq!(
        edge_state("src/report.rs#describe", "src/shapes.rs#area"),
        Some(ChangeState::Unchanged)
    );
}

#[test]
fn test_graph_has_the_callers_of_a_changed_symbol_as_context() {
    let directory = repository();
    let changes = changes(directory.path());
    let node = |key: &str| changes.graph.nodes.iter().find(|node| node.key == key);
    let test = node("src/shapes.rs#tests.test_area").expect("test node");
    assert_eq!(test.state, ChangeState::Changed);
    assert!(node("src/report.rs#outline").is_none());
}

#[test]
fn test_new_findings_have_the_stub_and_the_debt_comment() {
    let directory = repository();
    let changes = changes(directory.path());
    assert!(changes
        .new_findings
        .iter()
        .any(|finding| finding.kind == FindingKind::Stub && finding.file == "src/shapes.rs"));
    assert!(changes
        .new_findings
        .iter()
        .any(|finding| finding.kind == FindingKind::Satd && finding.file == "src/shapes.rs"));
    assert_eq!(changes.summary.new_stubs, 1);
    assert_eq!(changes.summary.new_satd, 1);
}

#[test]
fn test_files_have_the_owners_of_their_history() {
    let directory = repository();
    let changes = changes(directory.path());
    let shapes = changes
        .files
        .iter()
        .find(|file| file.file == "src/shapes.rs")
        .expect("shapes");
    assert_eq!(shapes.owners, ["ada", "grace"]);
    assert!(shapes.additions > 0 && shapes.deletions > 0);
}

#[test]
fn test_summary_counts_match_the_symbols() {
    let directory = repository();
    let summary = changes(directory.path()).summary;
    assert_eq!(summary.symbols_added, 3);
    assert_eq!(summary.symbols_removed, 1);
    assert!(summary.symbols_changed >= 2);
    assert_eq!(summary.files_changed, 2);
    assert!(summary.cyclomatic_after > summary.cyclomatic_before);
}

#[test]
fn test_comment_is_the_same_for_two_runs() {
    let directory = repository();
    let first = render(&changes(directory.path()), &CommentOptions::default(), &[]);
    let second = render(&changes(directory.path()), &CommentOptions::default(), &[]);
    assert_eq!(first, second);
}

#[test]
fn test_comment_snapshot() {
    let directory = repository();
    let text = render(&changes(directory.path()), &CommentOptions::default(), &[]);
    insta::assert_snapshot!("pr_comment", text);
}

#[test]
fn test_comment_snapshot_with_a_small_budget() {
    let directory = repository();
    let options = CommentOptions {
        max_bytes: 2600,
        ..CommentOptions::default()
    };
    let text = render(&changes(directory.path()), &options, &[]);
    assert!(text.len() <= 2600, "{} bytes", text.len());
    insta::assert_snapshot!("pr_comment_small_budget", text);
}

#[test]
fn test_cli_gives_the_same_comment_as_the_library() {
    let directory = repository();
    let output = markdown_omen(directory.path())
        .args(["diff", "-t", "main"])
        .output()
        .expect("omen starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = render(&changes(directory.path()), &CommentOptions::default(), &[]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn test_cli_max_bytes_limits_the_comment() {
    let directory = repository();
    let output = markdown_omen(directory.path())
        .args(["diff", "-t", "main", "--max-bytes", "2600"])
        .output()
        .expect("omen starts");
    assert!(output.status.success());
    assert!(output.stdout.len() <= 2600);
}

#[test]
fn test_cli_json_keeps_the_risk_shape() {
    let directory = repository();
    let value = fixture::cli_json(directory.path(), &["diff", "-t", "main"]);
    assert!(value.get("score").is_some());
    assert!(value.get("symbols").is_none());
}
