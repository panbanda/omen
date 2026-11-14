//! The `complexity` command.

use std::io::Write;
use std::path::PathBuf;

use super::analyzers::run_analyzer;
use super::gate::{gate_dispatch, resolve_gate_mode};
use super::scope::Scope;
use crate::cli::GateMode;
use crate::config::Config;
use crate::core::Result;
use crate::output::Format;

pub struct ComplexityRequest {
    pub scope: Scope,
    pub gate: Option<GateMode>,
    pub max_cyclomatic: Option<u32>,
    pub max_cognitive: Option<u32>,
}

/// Write the report, then apply the gate. The verdict goes to stderr, so
/// stdout stays one document in each format.
pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &ComplexityRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let result = run_analyzer::<crate::analyzers::complexity::Analyzer>(
        path,
        config,
        format,
        Some(&request.scope),
        out,
    )?;
    let gate = resolve_gate_mode(request.gate, config.complexity.gate);
    apply_gate(&result, config, request, gate)
}

fn apply_gate(
    result: &crate::analyzers::complexity::Analysis,
    config: &Config,
    request: &ComplexityRequest,
    gate: GateMode,
) -> Result<()> {
    if gate == GateMode::Off {
        return Ok(());
    }

    let max_cyclomatic = request
        .max_cyclomatic
        .unwrap_or(config.complexity.cyclomatic_max);
    let max_cognitive = request
        .max_cognitive
        .unwrap_or(config.complexity.cognitive_max);

    match result.check_thresholds(max_cyclomatic, max_cognitive) {
        Ok(()) => {
            eprintln!(
                "All {} functions within thresholds (cyclomatic <= {}, cognitive <= {})",
                result.summary.total_functions, max_cyclomatic, max_cognitive
            );
            Ok(())
        }
        Err(violations) => {
            eprintln!(
                "Complexity threshold exceeded in {} function(s):\n",
                violations.len()
            );
            for v in &violations {
                eprintln!(
                    "  {}:{} - {}: cyclomatic={}, cognitive={}",
                    v.file, v.line, v.name, v.cyclomatic, v.cognitive
                );
            }
            eprintln!(
                "\nThresholds: cyclomatic <= {}, cognitive <= {}",
                max_cyclomatic, max_cognitive
            );
            gate_dispatch(
                gate,
                crate::core::Error::threshold_violation(
                    format!(
                        "{} function(s) exceed complexity thresholds",
                        violations.len()
                    ),
                    violations.len() as f64,
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    fn request(gate: Option<GateMode>, max_cyclomatic: Option<u32>) -> ComplexityRequest {
        ComplexityRequest {
            scope: Scope::default(),
            gate,
            max_cyclomatic,
            max_cognitive: None,
        }
    }

    #[test]
    fn test_report_goes_to_the_writer() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(
            &path,
            &Config::default(),
            &request(None, None),
            Format::Json,
            &mut out,
        )
        .expect("complexity");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["summary"]["total_functions"], 3);
    }

    #[test]
    fn test_gate_error_fails_after_the_report_when_a_function_is_above_the_threshold() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        let result = run(
            &path,
            &Config::default(),
            &request(Some(GateMode::Error), Some(0)),
            Format::Json,
            &mut out,
        );
        assert!(matches!(
            result,
            Err(crate::core::Error::ThresholdViolation { .. })
        ));
        assert!(!out.is_empty());
    }

    #[test]
    fn test_gate_warn_does_not_fail() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(
            &path,
            &Config::default(),
            &request(Some(GateMode::Warn), Some(0)),
            Format::Json,
            &mut out,
        )
        .expect("warn gate does not fail");
    }
}
