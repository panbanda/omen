//! The `benchmark` command.

use std::io::Write;
use std::path::PathBuf;

use crate::core::{Error, Result};
use crate::output::{format_with_limits, Format};

/// Compare two omen binaries over the corpus. With `enforce`, a regression is
/// a threshold violation, so the process exits 2 like each other omen gate.
pub struct BenchmarkRequest {
    pub baseline: PathBuf,
    pub candidate: PathBuf,
    pub baseline_label: String,
    pub candidate_label: String,
    pub corpus: PathBuf,
    pub repetitions: usize,
    pub timeout_seconds: f64,
    pub real_root: PathBuf,
    /// Fail with a threshold violation when the candidate regresses.
    pub enforce: bool,
}

pub fn run(request: &BenchmarkRequest, format: Format, out: &mut dyn Write) -> Result<()> {
    let options = crate::benchmark::Options {
        baseline: request.baseline.clone(),
        candidate: request.candidate.clone(),
        baseline_label: request.baseline_label.clone(),
        candidate_label: request.candidate_label.clone(),
        corpus: request.corpus.clone(),
        repetitions: request.repetitions,
        // `from_secs_f64` panics on infinity and on NaN, so convert fallibly.
        timeout: std::time::Duration::try_from_secs_f64(request.timeout_seconds).map_err(|_| {
            Error::InvalidArgument(
                "timeout must be a finite, non-negative number of seconds".to_string(),
            )
        })?,
        real_root: request.real_root.clone(),
    };

    let report = crate::benchmark::run(&options)?;
    let mut out = out;
    format_with_limits(serde_json::to_value(&report)?, format, None, None, &mut out)?;

    if request.enforce && !report.non_regression {
        return Err(Error::threshold_violation(
            "quality benchmark regressed",
            0.0,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(timeout_seconds: f64) -> BenchmarkRequest {
        BenchmarkRequest {
            baseline: PathBuf::from("baseline"),
            candidate: PathBuf::from("candidate"),
            baseline_label: "a".to_string(),
            candidate_label: "b".to_string(),
            corpus: PathBuf::from("corpus.json"),
            repetitions: 7,
            timeout_seconds,
            real_root: PathBuf::from("."),
            enforce: false,
        }
    }

    #[test]
    fn test_a_timeout_that_is_not_finite_is_refused_before_a_run() {
        for timeout in [f64::NAN, f64::INFINITY, -1.0] {
            let mut out = Vec::new();
            let result = run(&request(timeout), Format::Json, &mut out);
            assert!(
                matches!(result, Err(Error::InvalidArgument(_))),
                "{timeout}: {result:?}"
            );
            assert!(out.is_empty());
        }
    }
}
