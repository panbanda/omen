//! The `diff` command.

use std::io::Write;
use std::path::Path;

use crate::config::Config;
use crate::core::Result;
use crate::output::{format_with_limits, Format};

pub struct DiffRequest {
    pub target: Option<String>,
    pub top: Option<usize>,
    pub offset: Option<usize>,
    /// The byte budget of the Markdown comment. 0 removes the limit.
    pub max_bytes: usize,
    /// The node cap of the call graph in the Markdown comment.
    pub max_nodes: usize,
}

/// Write the risk of the changes. With the Markdown format, write the
/// pull-request comment.
pub fn run(
    path: &Path,
    config: &Config,
    request: &DiffRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    if matches!(format, Format::Markdown) {
        return write_comment(path, config, request, out);
    }
    let result = analyze(path, request.target.as_deref())?;
    let mut out = out;
    format_with_limits(
        serde_json::to_value(&result)?,
        format,
        request.top,
        request.offset,
        &mut out,
    )
}

/// The risk analysis of the changes on the current branch. The MCP tool `diff`
/// calls this function.
pub fn analyze(path: &Path, target: Option<&str>) -> Result<crate::analyzers::changes::DiffResult> {
    crate::analyzers::changes::Analyzer::default().analyze_diff(path, target)
}

/// Writes the pull-request comment. `--top` and `--offset` do not apply: the
/// byte budget cuts the sections.
fn write_comment(
    path: &Path,
    config: &Config,
    request: &DiffRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let changes = crate::changeset::build::compute(path, request.target.as_deref(), config)?;
    let options = crate::changeset::markdown::CommentOptions {
        max_bytes: request.max_bytes,
        max_nodes: request.max_nodes,
    };
    let text = crate::changeset::markdown::render(&changes, &options, &[])?;
    out.write_all(text.as_bytes())?;
    Ok(())
}
