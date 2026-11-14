//! The `security` command.

use std::io::Write;
use std::path::PathBuf;

use super::gate::{gate_dispatch, resolve_gate_mode};
use super::scope::{build_context, scoped_file_set, Scope};
use crate::analyzers::security::{
    basis_of, category_of, mermaid, sarif, Analyzer as SecurityAnalyzer, Report, Section,
};
use crate::cli::SecuritySection;
use crate::config::{Config, GateMode};
use crate::core::{Analyzer, Error, Result};
use crate::index::model::ResolutionBasis;
use crate::index::security::{Sink, SinkCategory};
use crate::output::{format_with_limits, Format};

pub struct SecurityRequest {
    pub section: Option<SecuritySection>,
    pub category: Option<String>,
    pub entry: Option<String>,
    pub sink: Option<String>,
    pub min_basis: Option<String>,
    /// Look for the sinks of the experimental categories also.
    pub experimental: bool,
    pub scope: Scope,
    /// The `--gate` flag. Without it, `gate` in the table `[security]` of the configuration.
    pub gate: Option<GateMode>,
    /// The categories that the gate counts. Empty means the value of the configuration.
    pub gate_categories: Vec<String>,
    /// The weakest basis that the gate counts. Without it, the value of the configuration.
    pub gate_basis: Option<String>,
    /// The branch to compare with. Without it, the default branch.
    pub target: Option<String>,
}

/// Writes the report. Then it applies the gate to the sinks that the change adds. The
/// verdict goes to standard error.
pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &SecurityRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    // The report and the gate use this one configuration.
    let mut effective = config.clone();
    if request.experimental {
        effective.security.experimental = true;
    }
    if let Some(name) = &request.min_basis {
        ResolutionBasis::from_name(name)
            .ok_or_else(|| Error::config(format!("`{name}` is not a resolution basis.")))?;
        effective.security.min_basis = name.clone();
    }
    for category in &request.gate_categories {
        category_of(category)?;
    }
    if let Some(basis) = &request.gate_basis {
        basis_of(basis)?;
    }
    let config = &effective;
    let analyzer = SecurityAnalyzer::from_config(config)?;
    if let Some(category) = &request.category {
        if !SinkCategory::ALL
            .iter()
            .any(|known| known.name() == category)
        {
            return Err(Error::config(format!(
                "`{category}` is not a sink category."
            )));
        }
    }
    if (request.entry.is_some() || request.sink.is_some())
        && request.section != Some(SecuritySection::Paths)
    {
        return Err(Error::config(
            "`--entry` and `--sink` apply to `omen security paths` only.".to_string(),
        ));
    }
    let file_set = scoped_file_set(path, config, &request.scope)?;
    let ctx = build_context(path, &file_set, config);
    let report = analyzer.analyze(&ctx)?.restricted(
        request.category.as_deref(),
        request.entry.as_deref(),
        request.sink.as_deref(),
    );
    let section = match request.section {
        None => Section::All,
        Some(SecuritySection::Sinks) => Section::Sinks,
        Some(SecuritySection::Surface) => Section::Surface,
        Some(SecuritySection::Paths) => Section::Paths,
    };
    let mut out = out;
    match format {
        Format::Sarif => {
            serde_json::to_writer_pretty(&mut out, &sarif(&report))?;
            writeln!(out)?;
        }
        Format::Mermaid => {
            if section != Section::Paths {
                return Err(Error::config(
                    "`-f mermaid` applies to `omen security paths` only.".to_string(),
                ));
            }
            write!(out, "{}", mermaid(&report, 25))?;
        }
        _ => format_with_limits(
            report.section(section),
            format,
            request.scope.top,
            request.scope.offset,
            &mut out,
        )?,
    }

    let gate = resolve_gate_mode(request.gate, config.security.gate);
    if gate == GateMode::Off {
        return Ok(());
    }
    let categories = if request.gate_categories.is_empty() {
        config.security.gate_categories.clone()
    } else {
        request.gate_categories.clone()
    };
    let basis = basis_of(
        request
            .gate_basis
            .as_deref()
            .unwrap_or(&config.security.gate_basis),
    )?;
    let changes = crate::changeset::security::compute_with(
        path,
        request.target.as_deref(),
        config,
        &std::collections::BTreeSet::new(),
    )?;
    let violations: Vec<Sink> = Report::gated(&changes.sinks_added, &categories, basis)
        .into_iter()
        .cloned()
        .collect();
    if violations.is_empty() {
        eprintln!(
            "The change adds no sink that the gate counts ({} sink(s) added in total).",
            changes.sinks_added.len()
        );
        return Ok(());
    }
    eprintln!(
        "The change adds {} sink(s) that the gate counts:\n",
        violations.len()
    );
    for sink in &violations {
        eprintln!(
            "  {}:{} [{}] {} (basis {})",
            sink.file,
            sink.range.start_line,
            sink.category.name(),
            sink.callee_text,
            sink.basis.name()
        );
    }
    gate_dispatch(
        gate,
        Error::threshold_violation(
            format!("{} new sink(s)", violations.len()),
            violations.len() as f64,
        ),
    )
}
