//! Successful runs of the `eval` and `benchmark` command handlers.

use std::path::PathBuf;

use omen::commands::benchmark::{self, BenchmarkRequest};
use omen::commands::eval::{self, EvalRequest};
use omen::core::Error;
use omen::output::Format;
use serde_json::{json, Value};

/// A registration and a complete set of paired runs in which each candidate
/// succeeds and each baseline fails.
fn eval_fixture(count: usize) -> (Value, Vec<Value>) {
    let languages = ["rust", "go", "ruby", "typescript"];
    let cases: Vec<Value> = (0..count)
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
    (registration, runs)
}

fn eval_request(directory: &std::path::Path, registration: &Value, runs: &[Value]) -> EvalRequest {
    std::fs::write(
        directory.join("registration.json"),
        registration.to_string(),
    )
    .expect("write registration");
    std::fs::write(directory.join("runs.json"), json!(runs).to_string()).expect("write runs");
    EvalRequest {
        registration: directory.join("registration.json"),
        runs: directory.join("runs.json"),
        output: Some(directory.join("report.json")),
    }
}

#[test]
fn eval_writes_the_report_to_the_writer_and_to_the_output_file() {
    let directory = tempfile::tempdir().expect("directory");
    let (registration, runs) = eval_fixture(30);
    let request = eval_request(directory.path(), &registration, &runs);
    let mut out = Vec::new();
    eval::run(&request, Format::Json, &mut out).expect("eval");
    let printed: Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(printed["reported_outcome_gate"], true);
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("report.json")).expect("report file"),
    )
    .expect("json");
    assert_eq!(file, printed);
}

#[test]
fn eval_fails_the_gate_after_the_report_when_the_candidate_does_not_gain() {
    let directory = tempfile::tempdir().expect("directory");
    let (registration, mut runs) = eval_fixture(30);
    for run in &mut runs {
        run["tests_pass"] = json!(false);
    }
    let request = eval_request(directory.path(), &registration, &runs);
    let mut out = Vec::new();
    let result = eval::run(&request, Format::Json, &mut out);
    assert!(
        matches!(result, Err(Error::ThresholdViolation { .. })),
        "{result:?}"
    );
    assert!(!out.is_empty());
}

fn benchmark_request(directory: &std::path::Path, enforce: bool) -> BenchmarkRequest {
    let corpus = json!({
        "version": 1,
        "split": "development",
        "oracle": "test",
        "cases": [{
            "id": "rust_unique",
            "language": "rust",
            "category": "unique",
            "files": {
                "z.rs": "pub fn helper() -> &'static str { \"Z\" }\n",
                "main.rs": "mod z;\nuse z::helper;\nfn caller() -> &'static str { helper() }\nfn main(){println!(\"{}\",caller());}\n"
            },
            "expected": ["z.rs:helper"]
        }]
    });
    let corpus_path = directory.join("corpus.json");
    std::fs::write(&corpus_path, corpus.to_string()).expect("write corpus");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_omen"));
    BenchmarkRequest {
        baseline: binary.clone(),
        candidate: binary,
        baseline_label: "base".to_string(),
        candidate_label: "same".to_string(),
        corpus: corpus_path,
        repetitions: 2,
        timeout_seconds: 60.0,
        real_root: directory.to_path_buf(),
        enforce,
    }
}

#[test]
fn benchmark_of_a_binary_against_itself_does_not_regress() {
    let directory = tempfile::tempdir().expect("directory");
    let mut out = Vec::new();
    benchmark::run(
        &benchmark_request(directory.path(), true),
        Format::Json,
        &mut out,
    )
    .expect("benchmark");
    let report: Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(report["non_regression"], true);
}
