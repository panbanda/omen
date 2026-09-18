//! Paired quality benchmark: compare two omen binaries over a fixed corpus.
//!
//! This spawns the two binaries under test and reads their `symbol` reports. It
//! never executes fixture or project code, makes no network requests, and calls
//! no models.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::core::{Error, Result};

/// Observations per case beyond the retained warmup.
pub const DEFAULT_REPETITIONS: usize = 7;
/// Leading observations that are retained but excluded from timing summaries.
pub const WARMUPS: usize = 1;
/// Identifier recorded in every report.
pub const STANDARD: &str = "omen-improvement-v1";

/// Dimensions this harness does not measure. Recorded so a reader cannot take
/// a passing run for a broad improvement claim.
pub const UNMEASURED: [&str; 6] = [
    "model editing",
    "actual tokens/cost",
    "peak RSS",
    "release performance",
    "compiler binding",
    "held-out generalization",
];

/// Set-comparison of predicted against reviewed callees.
///
/// `precision` and `recall` are `None` rather than 1.0 when their denominator is
/// zero, so an empty prediction never scores as perfect.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub tp: usize,
    pub fp: usize,
    #[serde(rename = "fn")]
    pub fn_: usize,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

/// One invocation of one binary against one case.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Observation {
    /// `None` on success; the failure text otherwise. Failures are retained.
    pub error: Option<String>,
    pub predicted: Vec<String>,
    pub output_bytes: usize,
    /// Canonical digest of the whole report, absent when the run failed.
    pub report_sha256: Option<String>,
    pub elapsed_ms: f64,
}

/// Partial real-code labels: unreviewed edges are neither true nor false.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checks {
    pub missing: Vec<String>,
    pub forbidden: Vec<String>,
}

/// Every observation of one binary against one case.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VariantReport {
    pub reliable: bool,
    pub elapsed_ms: Vec<f64>,
    pub errors: Vec<Option<String>>,
    pub output_bytes: Vec<usize>,
    pub report_sha256: Vec<Option<String>>,
    pub predicted: Vec<String>,
    pub p50_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks: Option<Checks>,
}

/// One corpus case, scored for both binaries.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaseReport {
    pub id: String,
    pub language: String,
    pub non_regression: bool,
    pub baseline: VariantReport,
    pub candidate: VariantReport,
}

/// A binary under test.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BinaryRef {
    pub label: String,
    pub sha256: String,
}

/// Full benchmark output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub standard: String,
    pub corpus_sha256: String,
    pub baseline: BinaryRef,
    pub candidate: BinaryRef,
    pub repetitions: usize,
    pub warmups: usize,
    pub order: String,
    /// Always false: this harness cannot establish a broad improvement.
    pub broad_improvement_proven: bool,
    pub unmeasured: Vec<String>,
    pub cases: Vec<CaseReport>,
    pub non_regression: bool,
}

/// Inputs for one benchmark run.
#[derive(Debug, Clone)]
pub struct Options {
    pub baseline: PathBuf,
    pub candidate: PathBuf,
    pub baseline_label: String,
    pub candidate_label: String,
    pub corpus: PathBuf,
    pub repetitions: usize,
    pub timeout: Duration,
    pub real_root: PathBuf,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

/// Set comparison of predicted callees against reviewed expectations.
pub fn score(predicted: &[String], expected: &[String]) -> Score {
    let predicted: BTreeSet<&String> = predicted.iter().collect();
    let expected: BTreeSet<&String> = expected.iter().collect();
    let tp = predicted.intersection(&expected).count();
    let fp = predicted.difference(&expected).count();
    let fn_ = expected.difference(&predicted).count();
    Score {
        tp,
        fp,
        fn_,
        precision: (tp + fp > 0).then(|| tp as f64 / (tp + fp) as f64),
        recall: (tp + fn_ > 0).then(|| tp as f64 / (tp + fn_) as f64),
    }
}

/// A candidate may not add false positives or lose true positives.
pub fn gate(before: &Score, after: &Score, healthy: bool) -> bool {
    healthy && after.fp <= before.fp && after.fn_ <= before.fn_
}

/// Every observation succeeded and produced byte-identical reports.
pub fn reliable(observations: &[Observation]) -> bool {
    observations.iter().all(|o| o.error.is_none())
        && observations
            .iter()
            .all(|o| o.report_sha256 == observations[0].report_sha256)
}

/// Median, averaging the two middle values for an even count.
pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

/// SHA-256 of a file's bytes.
pub fn file_digest(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(std::fs::read(path)?);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Reject fixture paths that could escape the scratch directory.
///
/// A corpus is portable data, so this cannot rely on how the host platform
/// parses paths. Unix `std::path` has no notion of Windows prefixes: it reads
/// `C:/target` as two ordinary components. Colons and backslashes are
/// therefore rejected outright, and `Component::Normal` covers absolute
/// paths, traversal, and the prefixes Windows itself parses.
pub fn is_safe_fixture_path(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.contains(':')
        && Path::new(name)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

/// Run one binary against one prepared case root, retaining any failure.
fn observe(binary: &Path, root: &Path, query: &str, timeout: Duration) -> Observation {
    let started = Instant::now();
    let outcome = capture(binary, root, query, timeout).and_then(|(status, stdout)| {
        if !status {
            return Err("binary exited nonzero".to_string());
        }
        let report: Value =
            serde_json::from_slice(&stdout).map_err(|e| format!("invalid JSON: {e}"))?;
        let callees = report
            .get("callees")
            .and_then(Value::as_array)
            .ok_or_else(|| "missing callees array".to_string())?;
        let mut predicted = BTreeSet::new();
        for entry in callees {
            let name = entry
                .get("qualified_name")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid callee".to_string())?;
            predicted.insert(name.to_string());
        }
        Ok((
            stdout.len(),
            predicted.into_iter().collect::<Vec<String>>(),
            crate::eval::digest(&report),
        ))
    });

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    match outcome {
        Ok((output_bytes, predicted, report_sha256)) => Observation {
            error: None,
            predicted,
            output_bytes,
            report_sha256: Some(report_sha256),
            elapsed_ms,
        },
        Err(error) => Observation {
            error: Some(error),
            predicted: Vec::new(),
            output_bytes: 0,
            report_sha256: None,
            elapsed_ms,
        },
    }
}

/// Spawn the binary and collect stdout, killing it if it outlives `timeout`.
///
/// stdout is drained on its own thread so a full pipe buffer cannot deadlock
/// the wait loop.
fn capture(
    binary: &Path,
    root: &Path,
    query: &str,
    timeout: Duration,
) -> std::result::Result<(bool, Vec<u8>), String> {
    let mut child = Command::new(binary)
        .args(["-p".as_ref(), root.as_os_str()])
        .args(["-f", "json", "--compact", "symbol", query, "--no-source"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        let _ = sender.send(buffer);
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {:?}", timeout));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };

    let buffer = receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| format!("stdout read failed: {e}"))?;
    Ok((status.success(), buffer))
}

/// Confirm a real repository sits at the pinned revision with a clean tree.
fn verify_real_checkout(root: &Path, revision: &str, case_id: &str) -> Result<()> {
    let head = Command::new("git")
        .args(["-C".as_ref(), root.as_os_str()])
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !head.status.success() {
        return Err(invalid(format!("not a git repository: {case_id}")));
    }
    if String::from_utf8_lossy(&head.stdout).trim() != revision {
        return Err(invalid(format!("revision mismatch: {case_id}")));
    }
    let status = Command::new("git")
        .args(["-C".as_ref(), root.as_os_str()])
        .args(["status", "--porcelain"])
        .output()?;
    if !status.status.success() {
        return Err(invalid(format!("git status failed: {case_id}")));
    }
    if !status.stdout.is_empty() {
        return Err(invalid(format!("dirty real repository: {case_id}")));
    }
    Ok(())
}

/// Require an array of strings, rejecting a non-array or any non-string entry.
fn require_strings(case: &Value, key: &str, case_id: &str) -> Result<()> {
    match case.get(key) {
        None => Ok(()),
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Ok(()),
        Some(_) => Err(invalid(format!(
            "{key} must be an array of strings: {case_id}"
        ))),
    }
}

/// Reject a malformed case before it can be coerced into a passing result.
///
/// Silently defaulting a non-string fixture body to an empty file, or a
/// malformed label list to no labels, would let a case record zero false
/// positives and zero misses and pass the gate on nothing at all.
fn validate_case(case: &Value) -> Result<()> {
    let case_id = case.get("id").and_then(Value::as_str).unwrap_or_default();
    if !case.get("language").is_some_and(Value::is_string) {
        return Err(invalid(format!("case needs a string language: {case_id}")));
    }

    if case.get("repository").is_some() {
        if case.get("expected").is_some() {
            return Err(invalid(format!(
                "a real case cannot also declare expected: {case_id}"
            )));
        }
        for key in ["repository", "directory", "revision", "query"] {
            if !case.get(key).is_some_and(Value::is_string) {
                return Err(invalid(format!(
                    "real case needs a string {key}: {case_id}"
                )));
            }
        }
        require_strings(case, "required", case_id)?;
        require_strings(case, "forbidden", case_id)?;
        return Ok(());
    }

    let files = case
        .get("files")
        .and_then(Value::as_object)
        .filter(|files| !files.is_empty())
        .ok_or_else(|| invalid(format!("synthetic case needs files: {case_id}")))?;
    for (name, source) in files {
        if !is_safe_fixture_path(name) {
            return Err(invalid(format!("unsafe fixture path: {case_id}")));
        }
        if !source.is_string() {
            return Err(invalid(format!(
                "fixture contents must be a string: {case_id}"
            )));
        }
    }
    match case.get("expected") {
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Ok(()),
        _ => Err(invalid(format!(
            "synthetic case needs expected as an array of strings: {case_id}"
        ))),
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Validate and run the paired benchmark.
pub fn run(options: &Options) -> Result<Report> {
    if options.repetitions < 2 || options.timeout.is_zero() {
        return Err(invalid("require repetitions >= 2 and positive timeout"));
    }

    let corpus: Value = serde_json::from_slice(&std::fs::read(&options.corpus)?)?;
    if corpus.get("version") != Some(&Value::from(1)) {
        return Err(invalid("require version 1 and nonempty cases"));
    }
    let cases = corpus
        .get("cases")
        .and_then(Value::as_array)
        .filter(|cases| !cases.is_empty())
        .ok_or_else(|| invalid("require version 1 and nonempty cases"))?;

    let mut seen = BTreeSet::new();
    for case in cases {
        let id = case
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("each case needs a string id"))?;
        if !seen.insert(id) {
            return Err(invalid("duplicate case IDs"));
        }
        validate_case(case)?;
    }

    let binaries = [
        std::fs::canonicalize(&options.baseline)?,
        std::fs::canonicalize(&options.candidate)?,
    ];

    let mut results = Vec::new();
    for (case_index, case) in cases.iter().enumerate() {
        let case_id = case.get("id").and_then(Value::as_str).unwrap_or_default();
        let scratch = tempfile::Builder::new().prefix("omen-quality-").tempdir()?;
        let real = case.get("repository").is_some();

        let root = if real {
            let directory = case
                .get("directory")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(format!("real case needs a directory: {case_id}")))?;
            let revision = case
                .get("revision")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(format!("real case needs a revision: {case_id}")))?;
            let root = std::fs::canonicalize(options.real_root.join(directory))?;
            verify_real_checkout(&root, revision, case_id)?;
            root
        } else {
            let files = case
                .get("files")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid(format!("synthetic case needs files: {case_id}")))?;
            for (name, source) in files {
                if !is_safe_fixture_path(name) {
                    return Err(invalid("unsafe fixture path"));
                }
                let destination = scratch.path().join(name);
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(destination, source.as_str().unwrap_or_default())?;
                // validated above
            }
            scratch.path().to_path_buf()
        };

        let query = case
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("caller");

        // The first observation is a retained warmup: it counts for reliability
        // but not for timing or scoring.
        let mut observations: [Vec<Observation>; 2] = [Vec::new(), Vec::new()];
        for repetition in 0..=options.repetitions {
            let order = if (repetition + case_index) % 2 == 0 {
                [0, 1]
            } else {
                [1, 0]
            };
            for variant in order {
                let observed = observe(&binaries[variant], &root, query, options.timeout);
                observations[variant].push(observed);
            }
        }

        let expected = case.get("expected");
        let mut variants = Vec::new();
        for observed in &observations {
            let measured = &observed[WARMUPS];
            let variant = VariantReport {
                reliable: reliable(observed),
                elapsed_ms: observed.iter().map(|o| o.elapsed_ms).collect(),
                errors: observed.iter().map(|o| o.error.clone()).collect(),
                output_bytes: observed.iter().map(|o| o.output_bytes).collect(),
                report_sha256: observed.iter().map(|o| o.report_sha256.clone()).collect(),
                predicted: measured.predicted.clone(),
                p50_ms: median(
                    &observed[WARMUPS..]
                        .iter()
                        .map(|o| o.elapsed_ms)
                        .collect::<Vec<f64>>(),
                ),
                score: expected.map(|e| score(&measured.predicted, &strings(Some(e)))),
                checks: expected.is_none().then(|| {
                    let predicted: BTreeSet<&String> = measured.predicted.iter().collect();
                    Checks {
                        missing: strings(case.get("required"))
                            .into_iter()
                            .filter(|r| !predicted.contains(r))
                            .collect(),
                        forbidden: strings(case.get("forbidden"))
                            .into_iter()
                            .filter(|f| predicted.contains(f))
                            .collect(),
                    }
                }),
            };
            variants.push(variant);
        }

        let healthy = variants.iter().all(|v| v.reliable);
        let non_regression = match (&variants[0].score, &variants[1].score) {
            (Some(before), Some(after)) => gate(before, after, healthy),
            _ => {
                let subset = |key: fn(&Checks) -> &Vec<String>| match (
                    &variants[0].checks,
                    &variants[1].checks,
                ) {
                    (Some(before), Some(after)) => {
                        let before: BTreeSet<&String> = key(before).iter().collect();
                        key(after).iter().all(|item| before.contains(item))
                    }
                    _ => false,
                };
                healthy && subset(|c| &c.missing) && subset(|c| &c.forbidden)
            }
        };

        results.push(CaseReport {
            id: case_id.to_string(),
            language: case
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            non_regression,
            candidate: variants.pop().unwrap_or_else(|| unreachable!()),
            baseline: variants.pop().unwrap_or_else(|| unreachable!()),
        });
    }

    let non_regression = results.iter().all(|case| case.non_regression);
    Ok(Report {
        standard: STANDARD.to_string(),
        corpus_sha256: file_digest(&options.corpus)?,
        baseline: BinaryRef {
            label: options.baseline_label.clone(),
            sha256: file_digest(&binaries[0])?,
        },
        candidate: BinaryRef {
            label: options.candidate_label.clone(),
            sha256: file_digest(&binaries[1])?,
        },
        repetitions: options.repetitions,
        warmups: WARMUPS,
        order: "alternating".to_string(),
        broad_improvement_proven: false,
        unmeasured: UNMEASURED.iter().map(|u| (*u).to_string()).collect(),
        cases: results,
        non_regression,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(error: Option<&str>, sha: Option<&str>) -> Observation {
        Observation {
            error: error.map(str::to_string),
            predicted: Vec::new(),
            output_bytes: 0,
            report_sha256: sha.map(str::to_string),
            elapsed_ms: 1.0,
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn wrong_target_is_false_and_missed() {
        let result = score(&strings(&["wrong"]), &strings(&["right"]));
        assert_eq!(result.tp, 0);
        assert_eq!(result.fp, 1);
        assert_eq!(result.fn_, 1);
        assert_eq!(result.precision, Some(0.0));
        assert_eq!(result.recall, Some(0.0));
    }

    #[test]
    fn empty_precision_is_not_perfect() {
        assert_eq!(score(&[], &strings(&["right"])).precision, None);
    }

    #[test]
    fn duplicates_do_not_inflate() {
        assert_eq!(score(&strings(&["a", "a"]), &strings(&["a"])).tp, 1);
    }

    #[test]
    fn gain_cannot_hide_recall_loss() {
        let before = Score {
            tp: 0,
            fp: 4,
            fn_: 0,
            precision: None,
            recall: None,
        };
        let after = Score {
            tp: 0,
            fp: 0,
            fn_: 1,
            precision: None,
            recall: None,
        };
        assert!(!gate(&before, &after, true));
    }

    #[test]
    fn healthy_is_required_even_without_regression() {
        let before = Score {
            tp: 0,
            fp: 1,
            fn_: 1,
            precision: None,
            recall: None,
        };
        let after = Score {
            tp: 0,
            fp: 1,
            fn_: 1,
            precision: None,
            recall: None,
        };
        assert!(gate(&before, &after, true));
        assert!(!gate(&before, &after, false));
    }

    #[test]
    fn errors_including_warmups_fail() {
        assert!(!reliable(&[
            observation(Some("timeout"), None),
            observation(None, Some("a")),
        ]));
    }

    #[test]
    fn changing_output_fails() {
        assert!(!reliable(&[
            observation(None, Some("a")),
            observation(None, Some("b")),
        ]));
    }

    #[test]
    fn identical_successful_reports_are_reliable() {
        assert!(reliable(&[
            observation(None, Some("a")),
            observation(None, Some("a")),
        ]));
    }

    #[test]
    fn median_averages_the_middle_pair() {
        assert_eq!(median(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 5.0]), 2.5);
        assert_eq!(median(&[4.0]), 4.0);
    }

    /// Both binary paths point at a file that exists, so path resolution cannot
    /// be the reason a case is rejected. A malformed corpus that is merely
    /// coerced instead of rejected produces a report, not an error, and these
    /// assertions fail.
    fn options_for(corpus: &Path) -> Options {
        Options {
            baseline: corpus.to_path_buf(),
            candidate: corpus.to_path_buf(),
            baseline_label: "a".to_string(),
            candidate_label: "b".to_string(),
            corpus: corpus.to_path_buf(),
            repetitions: 2,
            timeout: Duration::from_secs(5),
            real_root: PathBuf::from(".."),
        }
    }

    fn rejects(corpus_json: &str) -> bool {
        let dir = tempfile::tempdir().expect("temp dir");
        let corpus = dir.path().join("corpus.json");
        std::fs::write(&corpus, corpus_json).expect("write corpus");
        run(&options_for(&corpus)).is_err()
    }

    #[test]
    fn windows_drive_prefixes_are_rejected() {
        assert!(!is_safe_fixture_path("C:/target"));
        assert!(!is_safe_fixture_path("C:target"));
    }

    #[test]
    fn non_string_fixture_contents_are_rejected() {
        assert!(rejects(
            r#"{"version":1,"cases":[{"id":"a","language":"rust","files":{"main.rs":7},"expected":[]}]}"#
        ));
    }

    #[test]
    fn non_string_expected_entries_are_rejected() {
        assert!(rejects(
            r#"{"version":1,"cases":[{"id":"a","language":"rust","files":{"main.rs":"fn a(){}"},"expected":[7]}]}"#
        ));
    }

    #[test]
    fn non_array_expected_is_rejected() {
        assert!(rejects(
            r#"{"version":1,"cases":[{"id":"a","language":"rust","files":{"main.rs":"fn a(){}"},"expected":"nope"}]}"#
        ));
    }

    #[test]
    fn non_string_real_labels_are_rejected() {
        assert!(rejects(
            r#"{"version":1,"cases":[{"id":"a","language":"ruby","repository":"o/r","directory":"d","revision":"abc","query":"q","required":[7],"forbidden":[]}]}"#
        ));
    }

    #[test]
    fn synthetic_case_without_files_is_rejected() {
        assert!(rejects(
            r#"{"version":1,"cases":[{"id":"a","language":"rust","expected":[]}]}"#
        ));
    }

    #[test]
    fn unsafe_fixture_paths_are_rejected() {
        assert!(is_safe_fixture_path("src/main.rs"));
        assert!(is_safe_fixture_path("z.rs"));
        assert!(!is_safe_fixture_path("/etc/passwd"));
        assert!(!is_safe_fixture_path("../escape.rs"));
        assert!(!is_safe_fixture_path("a/../../escape.rs"));
        assert!(!is_safe_fixture_path("a\\b.rs"));
    }
}
