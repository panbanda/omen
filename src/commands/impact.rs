//! The `impact` command.

use std::io::Write;
use std::path::PathBuf;

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
    let mut report = analyze_view(
        &view,
        &request.symbol,
        request.depth,
        request.direction,
        request.min_basis,
    );
    if let Some(first) = report.resolved.first() {
        let analysis = crate::analyzers::security::analyze_context(&ctx)?;
        report.security = Some(crate::index::security::symbol_security(&analysis, first));
    }
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

    #[test]
    fn test_an_unknown_symbol_resolves_to_nothing() {
        let value = report(&request("missing", Direction::Both));
        assert_eq!(value["resolved"], serde_json::json!([]));
    }
}
