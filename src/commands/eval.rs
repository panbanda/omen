//! The `eval` command.

use std::io::Write;
use std::path::PathBuf;

use super::scope::write_report_file;
use crate::core::{Error, Result};
use crate::output::{format_with_limits, Format};

/// Score paired coding-agent runs. A failed gate is a threshold violation, so
/// the process exits 2 like each other omen gate.
pub struct EvalRequest {
    pub registration: PathBuf,
    pub runs: PathBuf,
    /// A file that receives the full report as JSON.
    pub output: Option<PathBuf>,
}

pub fn run(request: &EvalRequest, format: Format, out: &mut dyn Write) -> Result<()> {
    let registration: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&request.registration)?)?;
    let runs_document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&request.runs)?)?;
    let runs = runs_document
        .as_array()
        .ok_or_else(|| Error::InvalidArgument("runs file must be a JSON array".to_string()))?;

    let report = crate::eval::score(&registration, runs)?;
    let value = serde_json::to_value(&report)?;

    write_report_file(request.output.as_deref(), &value)?;
    let mut out = out;
    format_with_limits(value, format, None, None, &mut out)?;

    if !report.reported_outcome_gate {
        return Err(Error::threshold_violation(
            "reported outcome gate failed",
            0.0,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(directory: &std::path::Path, runs: &str) -> EvalRequest {
        std::fs::write(directory.join("registration.json"), "{}").expect("write");
        std::fs::write(directory.join("runs.json"), runs).expect("write");
        EvalRequest {
            registration: directory.join("registration.json"),
            runs: directory.join("runs.json"),
            output: Some(directory.join("report.json")),
        }
    }

    #[test]
    fn test_a_runs_file_that_is_not_an_array_is_refused() {
        let directory = tempfile::tempdir().expect("directory");
        let mut out = Vec::new();
        let result = run(&request(directory.path(), "{}"), Format::Json, &mut out);
        assert!(
            matches!(result, Err(Error::InvalidArgument(_))),
            "{result:?}"
        );
        assert!(out.is_empty());
        assert!(!directory.path().join("report.json").exists());
    }

    #[test]
    fn test_a_registration_without_a_plan_gives_no_report() {
        let directory = tempfile::tempdir().expect("directory");
        let mut out = Vec::new();
        let result = run(&request(directory.path(), "[]"), Format::Json, &mut out);
        assert!(result.is_err());
        assert!(out.is_empty());
        assert!(!directory.path().join("report.json").exists());
    }
}
