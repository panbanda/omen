//! The `impact` command.

use std::io::Write;
use std::path::PathBuf;

use super::lookup::{write_failure, CommandLine};
use super::scope::{index_scope, Scope};
use crate::analyzers::impact::{analyze_view, Direction};
use crate::config::Config;
use crate::core::{AnalysisContext, Result};
use crate::index::model::ResolutionBasis;
use crate::index::view::CallView;
use crate::output::{format_with_limits, Format};

pub struct ImpactRequest {
    pub symbol: String,
    pub depth: usize,
    pub direction: Direction,
    pub min_basis: ResolutionBasis,
    pub scope: Scope,
    /// Read the files at this commit and not in the working tree.
    pub git_ref: Option<String>,
    /// The line on which the definition starts. It selects one of two definitions with one name.
    pub start_line: Option<u32>,
    /// The command line that each choice repeats. `None` when the command
    /// line cannot be repeated as text.
    pub command_line: Option<CommandLine>,
}

pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &ImpactRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let (file_set, source) = index_scope(path, config, &request.scope, request.git_ref.as_deref())?;
    let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
    if let Some(source) = source {
        ctx = ctx.with_content_source(source);
    }
    let view = CallView::from_index(&*ctx.code_index()?);
    let selected = analyze_view(
        &view,
        &request.symbol,
        request.start_line,
        request.depth,
        request.direction,
        request.min_basis,
    );
    let mut report = match selected {
        Err(crate::core::Error::SymbolLookup(failure)) => {
            write_failure(
                &failure,
                "symbol",
                request.command_line.as_ref(),
                format,
                out,
            )?;
            return Err(crate::core::Error::SymbolLookup(failure));
        }
        other => other?,
    };
    let analysis = crate::analyzers::security::analyze_context(&ctx)?;
    report.security = Some(crate::index::security::symbol_security(
        &analysis,
        &report.resolved[0],
    ));
    let mut out = out;
    format_with_limits(
        serde_json::to_value(&report)?,
        format,
        request.scope.top,
        request.scope.offset,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    fn request(symbol: &str, direction: Direction) -> ImpactRequest {
        ImpactRequest {
            symbol: symbol.to_string(),
            depth: 2,
            direction,
            min_basis: ResolutionBasis::SamePackage,
            scope: Scope::default(),
            git_ref: None,
            start_line: None,
            command_line: Some(CommandLine::plain("impact", symbol)),
        }
    }

    fn report(request: &ImpactRequest) -> serde_json::Value {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(&path, &Config::default(), request, Format::Json, &mut out).expect("impact");
        serde_json::from_slice(&out).expect("json")
    }

    #[test]
    fn test_callers_of_a_symbol() {
        let value = report(&request("helper", Direction::Callers));
        assert_eq!(value["resolved"], serde_json::json!(["shop.py#helper"]));
        assert_eq!(value["total_callers"], 1);
        assert_eq!(value["total_callees"], 0);
    }

    #[test]
    fn test_callees_of_a_symbol() {
        let value = report(&request("run", Direction::Callees));
        assert_eq!(value["total_callees"], 1);
        assert_eq!(value["total_callers"], 0);
    }

    /// A repository with `helper` in `shop.py` and in `util.py`.
    fn two_helpers() -> (tempfile::TempDir, PathBuf) {
        let (directory, path) = repository();
        std::fs::write(path.join("util.py"), "def helper():\n    return 2\n").expect("write");
        (directory, path)
    }

    #[test]
    fn test_an_ambiguous_name_writes_the_choices_and_fails() {
        let (_directory, path) = two_helpers();
        let mut out = Vec::new();
        let error = run(
            &path,
            &Config::default(),
            &request("helper", Direction::Both),
            Format::Json,
            &mut out,
        )
        .expect_err("ambiguous");
        assert!(
            matches!(error, crate::core::Error::SymbolLookup(_)),
            "{error}"
        );
        assert_eq!(error.exit_status(), 1);
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["error"]["code"], "ambiguous");
        let commands: Vec<&str> = value["error"]["suggestions"]
            .as_array()
            .expect("suggestions")
            .iter()
            .map(|choice| choice["command"].as_str().expect("command"))
            .collect();
        assert_eq!(
            commands,
            [
                "omen impact 'shop.py#helper'",
                "omen impact 'util.py#helper'"
            ]
        );
    }

    #[test]
    fn test_a_start_line_selects_one_definition() {
        let (_directory, path) = two_helpers();
        let mut selected = request("helper", Direction::Both);
        selected.start_line = Some(1);
        let mut out = Vec::new();
        assert!(run(&path, &Config::default(), &selected, Format::Json, &mut out).is_err());
        selected.symbol = "util.py:helper".to_string();
        let mut out = Vec::new();
        run(&path, &Config::default(), &selected, Format::Json, &mut out).expect("impact");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["resolved"], serde_json::json!(["util.py#helper"]));
    }

    #[test]
    fn test_an_unknown_name_writes_the_near_names_and_fails() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        let error = run(
            &path,
            &Config::default(),
            &request("helpr", Direction::Both),
            Format::Json,
            &mut out,
        )
        .expect_err("not found");
        assert!(
            matches!(error, crate::core::Error::SymbolLookup(_)),
            "{error}"
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["error"]["code"], "not_found");
        assert_eq!(value["error"]["suggestions"], serde_json::json!(["helper"]));
    }
}
