//! The `stubs` command.

use std::io::Write;
use std::path::PathBuf;

use super::gate::{gate_dispatch, resolve_gate_mode};
use super::scope::{build_context_from, scoped_inputs, Scope};
use crate::cli::{GateMode, GateSeverity};
use crate::config::Config;
use crate::core::{Analyzer, Result};
use crate::output::{format_with_limits, Format};

pub struct StubsRequest {
    pub scope: Scope,
    pub gate: Option<GateMode>,
    pub gate_severity: GateSeverity,
}

/// Run the `stubs` analyzer, then apply the gate.
///
/// The report is always written before the gate is evaluated, so
/// `-f json --gate error` still gives callers the full finding list on
/// stdout. The gate verdict goes to stderr, as it does for `complexity`.
pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &StubsRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let (file_set, source) = scoped_inputs(path, config, &request.scope)?;
    let ctx = build_context_from(path, &file_set, config, source);

    let analyzer = crate::analyzers::stubs::Analyzer::new();
    let result = analyzer.analyze(&ctx)?;

    let value = serde_json::to_value(&result)?;
    let mut out = out;
    format_with_limits(
        value,
        format,
        request.scope.top,
        request.scope.offset,
        &mut out,
    )?;

    let gate = resolve_gate_mode(request.gate, config.stubs.gate);
    if gate == GateMode::Off {
        return Ok(());
    }

    let gate_severity = match request.gate_severity {
        GateSeverity::Low => crate::analyzers::stubs::Severity::Low,
        GateSeverity::Medium => crate::analyzers::stubs::Severity::Medium,
        GateSeverity::High => crate::analyzers::stubs::Severity::High,
    };

    let violations: Vec<_> = result
        .stubs
        .iter()
        .filter(|s| s.severity >= gate_severity)
        .collect();

    if violations.is_empty() {
        eprintln!(
            "No stubs at or above severity '{}' found ({} total stub(s))",
            gate_severity, result.summary.total_stubs
        );
        return Ok(());
    }

    eprintln!(
        "Found {} stub(s) at or above severity '{}':\n",
        violations.len(),
        gate_severity
    );
    for v in &violations {
        eprintln!("  {}:{} [{}] {}", v.file, v.line, v.severity, v.snippet);
    }

    gate_dispatch(
        gate,
        crate::core::Error::threshold_violation(
            format!(
                "{} stub(s) at or above severity '{}'",
                violations.len(),
                gate_severity
            ),
            violations.len() as f64,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_with_a_stub() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("directory");
        std::fs::write(
            directory.path().join("lib.rs"),
            "pub fn pending() {\n    todo!()\n}\n",
        )
        .expect("write");
        let path = directory.path().to_path_buf();
        (directory, path)
    }

    fn request(gate: Option<GateMode>) -> StubsRequest {
        StubsRequest {
            scope: Scope::default(),
            gate,
            gate_severity: GateSeverity::Low,
        }
    }

    #[test]
    fn test_report_lists_the_stub() {
        let (_directory, path) = repository_with_a_stub();
        let mut out = Vec::new();
        run(
            &path,
            &Config::default(),
            &request(Some(GateMode::Off)),
            Format::Json,
            &mut out,
        )
        .expect("stubs");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["stubs"].as_array().expect("stubs").len(), 1);
    }

    #[test]
    fn test_gate_error_writes_the_report_and_then_fails() {
        let (_directory, path) = repository_with_a_stub();
        let mut out = Vec::new();
        let result = run(
            &path,
            &Config::default(),
            &request(Some(GateMode::Error)),
            Format::Json,
            &mut out,
        );
        assert!(matches!(
            result,
            Err(crate::core::Error::ThresholdViolation { .. })
        ));
        assert!(!out.is_empty());
    }
}
