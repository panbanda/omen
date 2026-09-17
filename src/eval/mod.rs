//! Fail-closed scoring of registered, paired coding-agent outcome records.
//!
//! This does not run models, execute patches, or certify submitted test logs.

use serde::Serialize;
use serde_json::Value;

use crate::core::{Error, Result};

/// Minimum paired tasks required before any reported gain can pass.
pub const MIN_PAIRS: usize = 30;
/// Minimum distinct repository clusters required before any reported gain can pass.
pub const MIN_REPOSITORIES: usize = 5;
/// Non-inferiority margin for token, elapsed-time and cost ratios.
pub const RESOURCE_MARGIN: f64 = 0.05;
/// Bootstrap draws over repository clusters.
pub const BOOTSTRAP_DRAWS: usize = 2000;

/// One scored case, reported alongside the aggregate gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseRow {
    pub case_id: String,
    pub baseline_success: i64,
    pub candidate_success: i64,
    pub candidate_unrelated_edits: i64,
}

/// Repository-cluster bootstrap intervals for the four predeclared comparisons.
/// A ratio is `None` when a positive candidate value against a zero baseline
/// makes it unmeasurable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Intervals {
    pub success_delta: [f64; 2],
    pub token_ratio: Option<[f64; 2]>,
    pub elapsed_ratio: [f64; 2],
    pub cost_ratio: Option<[f64; 2]>,
}

/// Full scorer output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub registration_sha256: String,
    pub pairs: usize,
    pub repositories: usize,
    pub adequate_held_out_sample: bool,
    pub reported_outcome_gate: bool,
    pub outcomes_independently_verified: bool,
    pub caveat: String,
    pub repository_weighted_intervals: Intervals,
    pub per_language_success_delta: std::collections::BTreeMap<String, i64>,
    pub cases: Vec<CaseRow>,
}

/// Deterministic PRNG for the cluster bootstrap.
///
/// The draws must stay identical across toolchains and releases, so the
/// generator is written out rather than taken from a crate whose stream is free
/// to change between versions. SplitMix64, seeded at zero.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..n`, rejecting the unbalanced tail so no cluster is
    /// drawn more often than another.
    fn below(&mut self, n: u64) -> u64 {
        let ceiling = u64::MAX - (u64::MAX % n) - 1;
        loop {
            let drawn = self.next_u64();
            if drawn <= ceiling {
                return drawn % n;
            }
        }
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

/// Whether `value` is a lowercase hex string of exactly `size` characters.
fn is_hex(value: Option<&Value>, size: usize) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| {
        text.len() == size
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// A nonnegative integer that is not a boolean and not a float.
fn whole(run: &Value, key: &str) -> Result<u64> {
    run.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("invalid {key}")))
}

/// A finite nonnegative int or float that is not a boolean.
fn amount(run: &Value, key: &str) -> Result<f64> {
    let value = run
        .get(key)
        .ok_or_else(|| invalid(format!("invalid {key}")))?;
    if !value.is_number() {
        return Err(invalid(format!("invalid {key}")));
    }
    let number = value
        .as_f64()
        .ok_or_else(|| invalid(format!("invalid {key}")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(format!("invalid {key}")));
    }
    Ok(number)
}

/// Total tokens for a run. Both counts and their sum are validated here so no
/// caller can sum them itself and overflow.
fn token_total(run: &Value) -> Result<u64> {
    whole(run, "input_tokens")?
        .checked_add(whole(run, "output_tokens")?)
        .ok_or_else(|| invalid("invalid/exceeded resource budget"))
}

fn boolean(run: &Value, key: &str) -> Result<bool> {
    run.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid(format!("{key} must be boolean")))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("missing {key}")))
}

/// Canonical SHA-256 of a JSON value: sorted keys, no insignificant whitespace.
///
/// `serde_json` keeps object keys in a `BTreeMap` and writes no padding, so its
/// output is already the canonical form.
pub fn digest(value: &Value) -> String {
    use sha2::{Digest, Sha256};

    let canonical = serde_json::to_string(value).expect("a JSON value always serializes");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Validate and score a registration against its complete set of paired runs.
///
/// Every failure is rejected rather than dropped: the caller cannot turn a
/// missing or malformed observation into a silent pass.
pub fn score(registration: &Value, runs: &[Value]) -> Result<Report> {
    use std::collections::BTreeMap;

    if registration.get("version") != Some(&Value::from(1)) {
        return Err(invalid("invalid registration version/split"));
    }
    let split = registration
        .get("split")
        .and_then(Value::as_str)
        .filter(|s| *s == "held_out" || *s == "development")
        .ok_or_else(|| invalid("invalid registration version/split"))?;

    let model = registration
        .get("model_snapshot")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let budget = registration.get("token_budget").and_then(Value::as_u64);
    let (model, budget) = match (model, budget) {
        (Some(model), Some(budget))
            if budget > 0 && is_hex(registration.get("prompt_sha256"), 64) =>
        {
            (model, budget)
        }
        _ => {
            return Err(invalid(
                "register immutable model, prompt and positive token budget",
            ))
        }
    };
    let prompt = registration
        .get("prompt_sha256")
        .ok_or_else(|| invalid("missing prompt_sha256"))?;

    let cases = registration
        .get("cases")
        .and_then(Value::as_array)
        .filter(|cases| !cases.is_empty())
        .ok_or_else(|| invalid("each registered case needs a non-empty string id"))?;
    for case in cases {
        let id = case.get("id").and_then(Value::as_str).unwrap_or_default();
        if !case.is_object() || id.is_empty() {
            return Err(invalid("each registered case needs a non-empty string id"));
        }
    }

    let mut planned: BTreeMap<&str, &Value> = BTreeMap::new();
    for case in cases {
        let id = text(case, "id")?;
        if planned.insert(id, case).is_some() {
            return Err(invalid("missing/duplicate registered cases"));
        }
    }

    let development = registration
        .get("development_repositories")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("declare development repositories explicitly"))?;
    if development.iter().any(|repo| !repo.is_string()) {
        return Err(invalid("declare development repositories explicitly"));
    }

    for case in cases {
        if !is_hex(case.get("revision"), 40)
            || !case.get("repository").is_some_and(Value::is_string)
            || !case.get("language").is_some_and(Value::is_string)
        {
            return Err(invalid("each case needs repository, revision and language"));
        }
        if split == "held_out" && development.contains(&case["repository"]) {
            return Err(invalid("held-out repository overlaps development data"));
        }
    }

    let registration_hash = digest(registration);
    let mut paired: BTreeMap<String, BTreeMap<String, &Value>> = BTreeMap::new();

    for run in runs {
        let case_id = run
            .get("case_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let variant = run
            .get("variant")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let case = match (planned.get(case_id), variant) {
            (Some(case), "baseline") | (Some(case), "candidate") => *case,
            _ => return Err(invalid("unexpected case or variant")),
        };

        for key in ["repository", "revision", "language"] {
            if run.get(key) != case.get(key) {
                return Err(invalid("case identity mismatch"));
            }
        }
        if run.get("model_snapshot") != Some(&Value::from(model)) {
            return Err(invalid("confounded pair: model_snapshot"));
        }
        if run.get("prompt_sha256") != Some(prompt) {
            return Err(invalid("confounded pair: prompt_sha256"));
        }
        if run.get("token_budget").and_then(Value::as_u64) != Some(budget) {
            return Err(invalid("confounded pair: token_budget"));
        }
        if run.get("registration_sha256") != Some(&Value::from(registration_hash.clone())) {
            return Err(invalid("confounded pair: registration_sha256"));
        }
        for key in ["patch_sha256", "test_log_sha256", "tool_snapshot_sha256"] {
            if !is_hex(run.get(key), 64) {
                return Err(invalid(format!("missing evidence hash: {key}")));
            }
        }

        let status = run
            .get("status")
            .and_then(Value::as_str)
            .filter(|s| matches!(*s, "completed" | "error" | "timeout"))
            .ok_or_else(|| invalid("unknown run status"))?;
        let tests_pass = boolean(run, "tests_pass")?;
        boolean(run, "patch_applies")?;
        let tokens = token_total(run)?;
        whole(run, "unrelated_edits")?;
        whole(run, "seed")?;
        let elapsed = amount(run, "elapsed_ms")?;
        amount(run, "cost_usd")?;

        if (tokens == 0 && status == "completed") || tokens > budget || elapsed <= 0.0 {
            return Err(invalid("invalid/exceeded resource budget"));
        }
        if status != "completed" && tests_pass {
            return Err(invalid("failed/timed-out run cannot claim passing tests"));
        }

        if paired
            .entry(case_id.to_string())
            .or_default()
            .insert(variant.to_string(), run)
            .is_some()
        {
            return Err(invalid("duplicate observation"));
        }
    }

    if paired.len() != planned.len() || paired.values().any(|variants| variants.len() != 2) {
        return Err(invalid(
            "missing registered observations; failures must not be dropped",
        ));
    }

    let success = |run: &Value| -> i64 {
        let completed = run.get("status").and_then(Value::as_str) == Some("completed");
        let tests_pass = run
            .get("tests_pass")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let applies = run
            .get("patch_applies")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let clean = run.get("unrelated_edits").and_then(Value::as_u64) == Some(0);
        i64::from(completed && tests_pass && applies && clean)
    };

    let mut by_repo: BTreeMap<String, Vec<[f64; 4]>> = BTreeMap::new();
    let mut by_language: BTreeMap<String, i64> = BTreeMap::new();
    let mut rows = Vec::new();
    let mut new_cost_from_zero = false;
    let mut new_tokens_from_zero = false;

    for (case_id, variants) in &paired {
        let baseline = variants["baseline"];
        let candidate = variants["candidate"];
        if baseline.get("seed") != candidate.get("seed") {
            return Err(invalid("paired seeds differ"));
        }

        let quality = success(candidate) - success(baseline);
        *by_language
            .entry(text(baseline, "language")?.to_string())
            .or_default() += quality;

        let baseline_cost = amount(baseline, "cost_usd")?;
        let candidate_cost = amount(candidate, "cost_usd")?;
        if baseline_cost == 0.0 && candidate_cost > 0.0 {
            new_cost_from_zero = true;
        }
        let baseline_tokens = token_total(baseline)? as f64;
        let candidate_tokens = token_total(candidate)? as f64;
        if baseline_tokens == 0.0 && candidate_tokens > 0.0 {
            new_tokens_from_zero = true;
        }

        by_repo
            .entry(text(baseline, "repository")?.to_string())
            .or_default()
            .push([
                quality as f64,
                if baseline_tokens == 0.0 {
                    1.0
                } else {
                    candidate_tokens / baseline_tokens
                },
                amount(candidate, "elapsed_ms")? / amount(baseline, "elapsed_ms")?,
                if baseline_cost == 0.0 {
                    1.0
                } else {
                    candidate_cost / baseline_cost
                },
            ]);

        rows.push(CaseRow {
            case_id: case_id.clone(),
            baseline_success: success(baseline),
            candidate_success: success(candidate),
            candidate_unrelated_edits: whole(candidate, "unrelated_edits")? as i64,
        });
    }

    let clusters: Vec<[f64; 4]> = by_repo
        .values()
        .map(|cases| {
            let mut means = [0.0; 4];
            for (index, mean) in means.iter_mut().enumerate() {
                *mean = cases.iter().map(|m| m[index]).sum::<f64>() / cases.len() as f64;
            }
            means
        })
        .collect();

    let mut rng = SplitMix64(0);
    let mut draws: Vec<Vec<f64>> = (0..4)
        .map(|_| Vec::with_capacity(BOOTSTRAP_DRAWS))
        .collect();
    for _ in 0..BOOTSTRAP_DRAWS {
        let selected: Vec<&[f64; 4]> = (0..clusters.len())
            .map(|_| &clusters[rng.below(clusters.len() as u64) as usize])
            .collect();
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.push(selected.iter().map(|c| c[index]).sum::<f64>() / selected.len() as f64);
        }
    }

    // Four predeclared comparisons; Bonferroni-adjusted two-sided intervals.
    let tail = 0.05 / (2.0 * 4.0);
    let mut intervals = Vec::with_capacity(4);
    for sample in draws.iter_mut() {
        sample.sort_by(|a, b| a.total_cmp(b));
        let count = sample.len();
        let lower = (count as f64 * tail).floor() as usize;
        let upper = ((count as f64 * (1.0 - tail)).ceil() as usize)
            .saturating_sub(1)
            .min(count - 1);
        intervals.push([sample[lower], sample[upper]]);
    }

    let adequate =
        rows.len() >= MIN_PAIRS && clusters.len() >= MIN_REPOSITORIES && split == "held_out";
    let gate = adequate
        && intervals[0][0] > 0.0
        && intervals[1..]
            .iter()
            .all(|interval| interval[1] <= 1.0 + RESOURCE_MARGIN)
        && !new_cost_from_zero
        && !new_tokens_from_zero
        && by_language.values().all(|delta| *delta >= 0)
        && rows.iter().all(|row| row.candidate_unrelated_edits == 0);

    Ok(Report {
        registration_sha256: registration_hash,
        pairs: rows.len(),
        repositories: clusters.len(),
        adequate_held_out_sample: adequate,
        reported_outcome_gate: gate,
        outcomes_independently_verified: false,
        caveat: "Input outcome claims and artifact hashes must be independently verified. \
This scorer does not run models or tests. Bootstrap intervals do not establish \
benchmark representativeness."
            .to_string(),
        repository_weighted_intervals: Intervals {
            success_delta: intervals[0],
            token_ratio: (!new_tokens_from_zero).then_some(intervals[1]),
            elapsed_ratio: intervals[2],
            cost_ratio: (!new_cost_from_zero).then_some(intervals[3]),
        },
        per_language_success_delta: by_language,
        cases: rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A complete, internally consistent set of paired runs in which every
    /// candidate succeeds and every baseline fails.
    fn fixture(count: usize) -> (Value, Vec<Value>) {
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
        let hash = digest(&registration);
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

    /// Re-stamp every run with the registration hash after mutating the plan.
    fn restamp(registration: &Value, runs: &mut [Value]) {
        let hash = digest(registration);
        for run in runs.iter_mut() {
            run["registration_sha256"] = json!(hash);
        }
    }

    #[test]
    fn digest_is_canonical_over_key_order() {
        let a = json!({"alpha": 1, "beta": [1, 2], "gamma": {"x": true, "y": null}});
        let b = json!({"gamma": {"y": null, "x": true}, "beta": [1, 2], "alpha": 1});
        assert_eq!(digest(&a), digest(&b));
        assert_ne!(digest(&a), digest(&json!({"alpha": 2, "beta": [1, 2]})));
        assert_eq!(digest(&a).len(), 64);
    }

    #[test]
    fn complete_synthetic_gain_passes_reported_gate_not_independent_verification() {
        let (plan, runs) = fixture(30);
        let report = score(&plan, &runs).expect("valid fixture");
        assert!(report.reported_outcome_gate);
        assert!(!report.outcomes_independently_verified);
    }

    #[test]
    fn missing_duplicate_and_unregistered_rows_rejected() {
        let (plan, runs) = fixture(30);

        let mut missing = runs.clone();
        missing.pop();
        assert!(score(&plan, &missing).is_err());

        let mut duplicated = runs.clone();
        duplicated.push(runs[0].clone());
        assert!(score(&plan, &duplicated).is_err());

        let mut unregistered = runs.clone();
        unregistered[0]["case_id"] = json!("missing");
        assert!(score(&plan, &unregistered).is_err());
    }

    #[test]
    fn model_prompt_seed_revision_and_budget_confounds_rejected() {
        let confounds = [
            ("model_snapshot", json!("other")),
            ("prompt_sha256", json!("f".repeat(64))),
            ("seed", json!(3)),
            ("revision", json!("f".repeat(40))),
            ("token_budget", json!(3000)),
        ];
        for (key, value) in confounds {
            let (plan, mut runs) = fixture(30);
            runs[0][key] = value;
            assert!(score(&plan, &runs).is_err(), "{key} must be rejected");
        }
    }

    #[test]
    fn bad_resources_and_missing_artifacts_rejected() {
        let invalid = [
            ("input_tokens", json!(-1)),
            ("output_tokens", json!(true)),
            ("test_log_sha256", json!("")),
            ("cost_usd", json!(-0.5)),
            ("input_tokens", json!(9999)),
            ("elapsed_ms", json!(0)),
        ];
        for (key, value) in invalid {
            let (plan, mut runs) = fixture(30);
            runs[0][key] = value;
            assert!(score(&plan, &runs).is_err(), "{key} must be rejected");
        }
    }

    #[test]
    fn small_or_development_samples_never_prove_gain() {
        let (plan, runs) = fixture(4);
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);

        let (mut plan, mut runs) = fixture(30);
        plan["split"] = json!("development");
        restamp(&plan, &mut runs);
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn timeouts_remain_failures_not_dropped() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("candidate") {
                run["status"] = json!("timeout");
                run["tests_pass"] = json!(false);
            }
        }
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn elapsed_regression_blocks_quality_gain() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("candidate") {
                run["elapsed_ms"] = json!(2000);
            }
        }
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn token_regression_blocks_quality_gain() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("candidate") {
                run["input_tokens"] = json!(1000);
            }
        }
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn cost_regression_blocks_quality_gain() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("candidate") {
                run["cost_usd"] = json!(0.02);
            }
        }
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn unrelated_edits_cannot_be_hidden_by_aggregate_gains() {
        let (plan, mut runs) = fixture(30);
        runs[1]["unrelated_edits"] = json!(1);
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn candidate_unrelated_edits_rejected_even_when_baseline_matches() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["case_id"] == json!("0") {
                run["unrelated_edits"] = json!(1);
            }
        }
        let report = score(&plan, &runs).expect("valid");
        assert!(!report.reported_outcome_gate);
        assert_eq!(report.cases[0].candidate_unrelated_edits, 1);
    }

    #[test]
    fn unapplied_patch_is_not_success() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("candidate") {
                run["patch_applies"] = json!(false);
            }
        }
        assert!(!score(&plan, &runs).expect("valid").reported_outcome_gate);
    }

    #[test]
    fn new_cost_from_zero_cannot_pass() {
        let (plan, mut runs) = fixture(30);
        for run in runs.iter_mut() {
            if run["variant"] == json!("baseline") {
                run["cost_usd"] = json!(0);
            }
        }
        let report = score(&plan, &runs).expect("valid");
        assert!(!report.reported_outcome_gate);
        assert!(report.repository_weighted_intervals.cost_ratio.is_none());
    }

    #[test]
    fn zero_token_connection_failure_is_retained() {
        let (plan, mut runs) = fixture(30);
        runs[0]["status"] = json!("error");
        runs[0]["tests_pass"] = json!(false);
        runs[0]["input_tokens"] = json!(0);
        runs[0]["output_tokens"] = json!(0);
        let report = score(&plan, &runs).expect("valid");
        assert_eq!(report.pairs, 30);
        assert!(!report.reported_outcome_gate);
        assert!(report.repository_weighted_intervals.token_ratio.is_none());
    }

    #[test]
    fn development_repository_cannot_be_called_held_out() {
        let (mut plan, mut runs) = fixture(30);
        plan["development_repositories"] = json!(["repo-0"]);
        restamp(&plan, &mut runs);
        assert!(score(&plan, &runs).is_err());
    }

    #[test]
    fn malformed_cases_rejected_as_invalid_evidence() {
        let malformed = [
            json!([{}]),
            json!("not-a-list"),
            json!([{"id": ""}]),
            json!([null]),
            json!([]),
        ];
        for cases in malformed {
            let (mut plan, runs) = fixture(30);
            plan["cases"] = cases.clone();
            assert!(score(&plan, &runs).is_err(), "{cases} must be rejected");
        }
    }

    #[test]
    fn failed_run_cannot_claim_passing_tests() {
        let (plan, mut runs) = fixture(30);
        runs[0]["status"] = json!("error");
        runs[0]["tests_pass"] = json!(true);
        assert!(score(&plan, &runs).is_err());
    }

    #[test]
    fn overflowing_token_counts_are_rejected() {
        let (mut plan, mut runs) = fixture(30);
        plan["token_budget"] = json!(u64::MAX);
        restamp(&plan, &mut runs);
        for run in runs.iter_mut() {
            run["token_budget"] = json!(u64::MAX);
        }
        runs[0]["input_tokens"] = json!(u64::MAX);
        runs[0]["output_tokens"] = json!(u64::MAX);
        assert!(score(&plan, &runs).is_err());
    }

    #[test]
    fn scoring_is_deterministic_across_runs() {
        let (plan, runs) = fixture(30);
        let first = score(&plan, &runs).expect("valid");
        let second = score(&plan, &runs).expect("valid");
        assert_eq!(first, second);
    }
}
