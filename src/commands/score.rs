//! The `score` command and `score trend`.

use std::io::Write;
use std::path::PathBuf;

use super::analyzers::{run_analyzer_instance, score_analyzer};
use super::gate::{gate_dispatch, resolve_gate_mode};
use crate::cli::{GateMode, TrendPeriod};
use crate::config::Config;
use crate::core::Result;
use crate::output::Format;

pub struct ScoreRequest {
    pub gate: Option<GateMode>,
    pub fail_under: Option<f64>,
}

pub struct TrendRequest {
    pub since: String,
    pub period: TrendPeriod,
    pub samples: Option<usize>,
}

/// Write the health score, then apply the gate.
pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &ScoreRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let gate = resolve_gate_mode(request.gate, config.score.gate);
    let result = run_analyzer_instance(path, config, format, None, score_analyzer(config), out)?;
    apply_gate(&result, config, request, gate)
}

/// Write the score trend over the history. The trend has no gate.
pub fn run_trend(
    path: &std::path::Path,
    config: &Config,
    request: &TrendRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let trend_data = crate::score::analyze_trend(
        path,
        config,
        &request.since,
        request.period,
        request.samples,
    )?;
    let mut out = out;
    match format {
        Format::Json | Format::JsonCompact => {
            format.format(&trend_data, &mut out)?;
        }
        Format::Markdown => {
            say!(out, "# Score Trend Analysis\n");
            say!(
                out,
                "**Period**: {} ({})",
                request.since,
                format!("{:?}", request.period).to_lowercase()
            );
            say!(out, "**Data Points**: {}\n", trend_data.points.len());

            if !trend_data.points.is_empty() {
                say!(out, "## Overall Trend\n");
                say!(out, "- **Start Score**: {}", trend_data.start_score);
                say!(out, "- **End Score**: {}", trend_data.end_score);
                say!(
                    out,
                    "- **Change**: {:+}",
                    trend_data.end_score - trend_data.start_score
                );
                say!(out, "- **Slope**: {:.2} points/period", trend_data.slope);
                say!(out, "- **R-squared**: {:.3}\n", trend_data.r_squared);

                if !trend_data.component_trends.is_empty() {
                    say!(out, "## Component Trends\n");
                    say!(out, "| Component | Slope | Correlation |");
                    say!(out, "|-----------|-------|-------------|");
                    let mut component_trends: Vec<_> = trend_data.component_trends.iter().collect();
                    component_trends.sort_by_key(|(name, _)| *name);
                    for (name, stats) in component_trends {
                        say!(
                            out,
                            "| {} | {:.2} | {:.3} |",
                            name,
                            stats.slope,
                            stats.correlation
                        );
                    }
                    say!(out);
                }

                say!(out, "## History\n");
                say!(out, "| Date | Score |");
                say!(out, "|------|-------|");
                for point in &trend_data.points {
                    say!(out, "| {} | {} |", point.date, point.score);
                }
            } else {
                say!(
                    out,
                    "No historical data available for the specified period."
                );
            }
        }
        Format::Text => {
            say!(
                out,
                "Score Trend: {} - {}",
                trend_data.start_score,
                trend_data.end_score
            );
            say!(
                out,
                "Change: {:+}",
                trend_data.end_score - trend_data.start_score
            );
            say!(out, "Slope: {:.2}", trend_data.slope);
        }
        Format::Sarif => format.format(&trend_data, &mut out)?,
    }
    Ok(())
}

/// Apply the gate of `score` to a result. The report is already written. The
/// verdict goes to stderr.
fn apply_gate(
    result: &crate::score::Analysis,
    config: &Config,
    request: &ScoreRequest,
    gate: GateMode,
) -> Result<()> {
    if gate == GateMode::Off {
        return Ok(());
    }

    let min_score = request.fail_under.unwrap_or(config.score.min);

    match result.check_threshold(min_score) {
        Ok(()) => {
            eprintln!(
                "Score {:.1} ({}) meets minimum {:.1}",
                result.overall_score, result.grade, min_score
            );
            Ok(())
        }
        Err(violation) => {
            eprintln!("{violation}");
            gate_dispatch(gate, violation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    #[test]
    fn test_score_report_goes_to_the_writer_and_gate_error_fails_below_the_minimum() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        let request = ScoreRequest {
            gate: Some(GateMode::Error),
            fail_under: Some(101.0),
        };
        let result = run(&path, &Config::default(), &request, Format::Json, &mut out);
        assert!(matches!(
            result,
            Err(crate::core::Error::ThresholdViolation { .. })
        ));
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert!(value["overall_score"].is_number());
    }

    #[test]
    fn test_score_without_a_gate_does_not_fail() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        let request = ScoreRequest {
            gate: Some(GateMode::Off),
            fail_under: Some(101.0),
        };
        run(&path, &Config::default(), &request, Format::Json, &mut out).expect("score");
    }
}
