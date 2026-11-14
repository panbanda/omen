//! Ground truth for `deadcode`. Each directory `tests/fixtures/deadcode/<language>/`
//! holds source files and a `labels.json`. A `dead` label names a definition
//! that the analysis must report. An `alive` label names a definition that the
//! analysis must not report.
//!
//! `expected_failures.json` lists the labels that omen gets wrong. The test
//! fails if a failure is not in the list, or if a listed label is now correct.
//! To write the list again from the current results, run:
//!
//! ```sh
//! OMEN_TRUTH_BLESS=1 cargo test --test deadcode_truth
//! ```
//!
//! Then examine the diff. A new entry is permitted only for a new label.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use omen::analyzers::deadcode::Analyzer;
use omen::benchmark::truth::{
    bless, check_ledger, render_table, summarize, Assertion, LedgerEntry,
};
use omen::config::Config;
use omen::core::{AnalysisContext, Analyzer as _, FileSet};
use serde::Deserialize;

const LEDGER_FILE: &str = "expected_failures.json";
const BLESS_TRACKING: &str = "phase-2-deadcode";
const LANGUAGES: [&str; 14] = [
    "bash",
    "c",
    "cpp",
    "csharp",
    "go",
    "java",
    "javascript",
    "jsx",
    "php",
    "python",
    "ruby",
    "rust",
    "tsx",
    "typescript",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    label: String,
    file: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    language: String,
    #[allow(dead_code)]
    oracle: String,
    not_applicable: Vec<String>,
    dead: Vec<Label>,
    alive: Vec<Label>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deadcode")
}

fn load_labels(language: &str) -> Labels {
    let path = root().join(language).join("labels.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn load_ledger() -> Vec<LedgerEntry> {
    let path = root().join(LEDGER_FILE);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The (file, name) pairs that the analysis reports for the files of one language.
fn reported(language: &str) -> BTreeSet<(String, String)> {
    let scratch = tempfile::Builder::new()
        .prefix("omen-deadcode-truth-")
        .tempdir()
        .unwrap();
    for entry in std::fs::read_dir(root().join(language)).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|name| name != "labels.json") {
            std::fs::copy(&path, scratch.path().join(path.file_name().unwrap())).unwrap();
        }
    }
    let config = Config::default();
    let files = FileSet::from_path(scratch.path(), &config).unwrap();
    let ctx = AnalysisContext::new(&files, &config, Some(scratch.path()));
    Analyzer::new()
        .analyze(&ctx)
        .unwrap()
        .items
        .into_iter()
        .map(|item| (item.file, item.name))
        .collect()
}

fn assertions_of(language: &str) -> Vec<Assertion> {
    let labels = load_labels(language);
    assert_eq!(labels.language, language);
    let found = reported(language);
    let mut assertions = Vec::new();
    for (kind, list, must_report) in [
        ("dead", &labels.dead, true),
        ("alive", &labels.alive, false),
    ] {
        for label in list {
            let is_reported = found.contains(&(label.file.clone(), label.name.clone()));
            let passed = is_reported == must_report;
            let detail = if passed {
                String::new()
            } else if must_report {
                format!("omen does not report {} in {}", label.name, label.file)
            } else {
                format!("omen reports {} in {} as dead", label.name, label.file)
            };
            assertions.push(Assertion {
                id: format!("{language}/deadcode/{kind}/{}", label.label),
                passed,
                detail,
            });
        }
    }
    assertions
}

#[test]
fn each_language_has_dead_labels_or_a_reason() {
    for language in LANGUAGES {
        let labels = load_labels(language);
        assert!(
            !labels.dead.is_empty() || !labels.not_applicable.is_empty(),
            "{language}: no dead label and no reason"
        );
        assert!(!labels.alive.is_empty(), "{language}: no alive label");
        for label in labels.dead.iter().chain(&labels.alive) {
            assert!(
                root().join(language).join(&label.file).is_file(),
                "{language}: {} is not a file",
                label.file
            );
        }
    }
}

#[test]
fn deadcode_agrees_with_the_labels_or_the_ledger() {
    let mut assertions = Vec::new();
    for language in LANGUAGES {
        assertions.extend(assertions_of(language));
    }
    println!("{}", render_table(&summarize(&assertions)));

    if std::env::var_os("OMEN_TRUTH_BLESS").is_some() {
        let blessed = bless(&assertions, &load_ledger(), BLESS_TRACKING);
        let mut text = serde_json::to_string_pretty(&blessed).unwrap();
        text.push('\n');
        std::fs::write(root().join(LEDGER_FILE), text).unwrap();
    }

    let violations = check_ledger(&assertions, &load_ledger());
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "{} ledger violations:\n{}",
        violations.len(),
        report.join("\n")
    );
}
