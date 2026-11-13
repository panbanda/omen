//! The fixture repository and the process helpers that the integration tests
//! share. Each test file that uses them includes this file as a module.

// Each test file uses a different part of this module.
#![allow(dead_code)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/truth/rust/basics"
);

/// A feature flag for the `flags` analyzer, which has no provider for Rust.
const FLAG_SOURCE: &str =
    "export function flagged(client) {\n  return client.isEnabled(\"new-shape\");\n}\n";

/// A source file that the test adds to the copy of the fixture.
///
/// The fixture has no code clone and no import cycle. This file adds one of
/// each, thus the related arrays have an element shape.
const EXTRA_SOURCE: &str = r#"use super::other::only_here;

pub fn extra_total() -> u32 {
    only_here() + 1
}

pub fn sum_first(values: &[u32]) -> u32 {
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
        total
    }
}

pub fn sum_second(values: &[u32]) -> u32 {
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
        total
    }
}
"#;

pub fn git(repository: &Path, author: &str, date: &str, arguments: &[&str]) {
    let email = format!("{author}@example.com");
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        // The configuration of the user can add hooks or commit signatures.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", &email)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", &email)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git starts");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn append(repository: &Path, file: &str, text: &str) {
    let path = repository.join(file);
    let mut content = std::fs::read_to_string(&path).expect("fixture file");
    content.push_str(text);
    std::fs::write(&path, content).expect("write fixture file");
}

pub fn commit(repository: &Path, author: &str, date: &str, message: &str) {
    git(repository, author, date, &["add", "."]);
    git(repository, author, date, &["commit", "-m", message]);
}

/// Copies the Rust fixture into a new repository with a fixed history.
///
/// The history has three commits by two authors on `main` and one commit on
/// `feature`. `HEAD` is `feature`, thus `diff --target main` has content.
pub fn fixture_repository() -> TempDir {
    let directory = TempDir::new().expect("temporary directory");
    let repository = directory.path();
    for entry in std::fs::read_dir(FIXTURE).expect("fixture directory") {
        let source = entry.expect("fixture entry").path();
        if source
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            let name = source.file_name().expect("file name");
            std::fs::copy(&source, repository.join(name)).expect("copy fixture file");
        }
    }

    let first = "2024-01-01T12:00:00Z";
    git(
        repository,
        "ada",
        first,
        &["init", "--initial-branch", "main"],
    );
    std::fs::write(repository.join("extra.rs"), EXTRA_SOURCE).expect("write extra file");
    std::fs::write(repository.join("flags.js"), FLAG_SOURCE).expect("write flag file");
    append(repository, "other.rs", "\nuse super::extra::extra_total;\n");
    commit(repository, "ada", first, "feat: add the shapes");

    append(
        repository,
        "util.rs",
        "\n// TODO: remove this shortcut\npub fn later() -> u32 {\n    todo!()\n}\n",
    );
    append(
        repository,
        "main.rs",
        "\nfn added() -> u32 {\n    if local() > 3 {\n        1\n    } else {\n        2\n    }\n}\n",
    );
    commit(
        repository,
        "bob",
        "2024-01-08T12:00:00Z",
        "fix: correct the helper",
    );

    append(
        repository,
        "util.rs",
        "\npub fn third() -> u32 {\n    3\n}\n",
    );
    append(repository, "main.rs", "\nfn fourth() -> u32 {\n    4\n}\n");
    commit(
        repository,
        "ada",
        "2024-01-15T12:00:00Z",
        "fix: repair the total",
    );

    let last = "2024-01-22T12:00:00Z";
    git(repository, "bob", last, &["checkout", "-b", "feature"]);
    append(
        repository,
        "other.rs",
        "\npub fn fifth() -> u32 {\n    5\n}\n",
    );
    commit(repository, "bob", last, "feat: add the fifth function");

    directory
}

pub fn omen(repository: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omen"));
    command
        .arg("-p")
        .arg(repository)
        .args(["-f", "json"])
        .current_dir(repository);
    command
}

pub fn cli_json(repository: &Path, arguments: &[&str]) -> Value {
    let output = omen(repository)
        .args(arguments)
        .output()
        .expect("omen starts");
    assert!(
        output.status.success(),
        "omen {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("omen {arguments:?} did not write JSON: {error}"))
}

/// Sends the requests to one `omen mcp` process and returns the responses.
pub fn mcp_responses(repository: &Path, requests: &[Value]) -> Vec<Value> {
    let mut child = omen(repository)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("omen mcp starts");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write request");
        }
    }
    let output = child.wait_with_output().expect("omen mcp stops");
    assert!(
        output.status.success(),
        "omen mcp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .expect("UTF-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect();
    assert_eq!(
        responses.len(),
        requests.len(),
        "one response for each request"
    );
    responses
}
