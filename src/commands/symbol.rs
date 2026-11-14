//! The `symbol` command.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::scope::{index_scope, Scope};
use crate::config::Config;
use crate::core::{AnalysisContext, Result};
use crate::index::model::ResolutionBasis;
use crate::index::view::CallView;
use crate::output::{truncate_field, Format};
use crate::symbol::{report_with_reader, SymbolOptions};

pub struct SymbolRequest {
    pub name: String,
    pub include_source: bool,
    pub max_source_lines: usize,
    pub min_basis: ResolutionBasis,
    pub scope: Scope,
    /// Read the files at this commit and not in the working tree.
    pub git_ref: Option<String>,
}

pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &SymbolRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let (file_set, source) = index_scope(path, config, &request.scope, request.git_ref.as_deref())?;
    let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
    if let Some(source) = source {
        ctx = ctx.with_content_source(source);
    }
    let view = CallView::from_index(&*ctx.code_index()?);

    let options = SymbolOptions {
        include_source: request.include_source,
        max_source_lines: request.max_source_lines,
        min_basis: request.min_basis,
    };
    let read = |file: &Path| ctx.read_file(file);
    let mut report = report_with_reader(&view, &request.name, &options, None, &read)?;
    let analysis = crate::analyzers::security::analyze_context(&ctx)?;
    report.security = Some(crate::index::security::symbol_security(
        &analysis,
        &report.qualified_name,
    ));
    let mut value = serde_json::to_value(&report)?;
    // `--top` and `--offset` page only the callers, as the MCP tool does.
    let scope = &request.scope;
    if !matches!(format, Format::Sarif) && (scope.top.is_some() || scope.offset.is_some()) {
        truncate_field(
            &mut value,
            "callers",
            scope.top.unwrap_or(0),
            scope.offset.unwrap_or(0),
        );
    }
    let mut out = out;
    format.format_value(&value, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    fn request(name: &str) -> SymbolRequest {
        SymbolRequest {
            name: name.to_string(),
            include_source: true,
            max_source_lines: 200,
            min_basis: ResolutionBasis::SamePackage,
            scope: Scope::default(),
            git_ref: None,
        }
    }

    fn report(request: &SymbolRequest) -> serde_json::Value {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(&path, &Config::default(), request, Format::Json, &mut out).expect("symbol");
        serde_json::from_slice(&out).expect("json")
    }

    #[test]
    fn test_report_has_the_source_and_the_callers() {
        let value = report(&request("helper"));
        assert_eq!(value["qualified_name"], "shop.py#helper");
        assert!(value["source"]
            .as_str()
            .expect("source")
            .contains("return 1"));
        assert_eq!(value["callers"].as_array().expect("callers").len(), 1);
    }

    #[test]
    fn test_report_without_source() {
        let mut without = request("helper");
        without.include_source = false;
        assert!(report(&without)["source"].is_null());
    }

    #[test]
    fn test_top_pages_only_the_callers() {
        let mut paged = request("helper");
        paged.scope.top = Some(1);
        paged.scope.offset = Some(1);
        let value = report(&paged);
        assert_eq!(value["callers"], serde_json::json!([]));
        assert_eq!(value["qualified_name"], "shop.py#helper");
    }
}
