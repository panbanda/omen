//! The `diff` command.

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::analyzers::changes::DiffResult;
use crate::changeset::model::{ChangeSet, FileChange, NewFinding, Summary, SymbolChange};
use crate::changeset::security::SecurityChanges;
use crate::config::Config;
use crate::core::Result;
use crate::output::{format_with_limits, truncate_field, Format};

pub struct DiffRequest {
    pub target: Option<String>,
    pub top: Option<usize>,
    pub offset: Option<usize>,
    /// The byte budget of the Markdown comment. 0 removes the limit.
    pub max_bytes: usize,
    /// The node cap of the call graph in the Markdown comment.
    pub max_nodes: usize,
    /// The cap of the direct callers of each symbol in the JSON. 0 removes the cap.
    pub max_callers: usize,
}

/// The default cap of the direct callers of each symbol in the JSON.
pub const DEFAULT_MAX_CALLERS: usize = 20;

/// The name of the list that `--top` and `--offset` cut in the JSON.
pub const PAGED_LIST: &str = "symbols";

/// One changed symbol in the JSON. The counts are complete when the cap cuts
/// `direct_callers`.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolEntry {
    #[serde(flatten)]
    pub change: SymbolChange,
    pub direct_caller_count: usize,
    pub test_caller_count: usize,
    pub direct_callers_omitted: usize,
}

impl SymbolEntry {
    /// Keeps a maximum of `max_callers` direct callers. The callers that are
    /// not test code come first, thus the cap removes test callers first.
    fn new(mut change: SymbolChange, max_callers: usize) -> Self {
        let direct_caller_count = change.direct_callers.len();
        let test_caller_count = change.test_callers();
        change
            .direct_callers
            .sort_by(|left, right| (left.is_test, &left.key).cmp(&(right.is_test, &right.key)));
        if max_callers > 0 {
            change.direct_callers.truncate(max_callers);
        }
        Self {
            direct_callers_omitted: direct_caller_count - change.direct_callers.len(),
            change,
            direct_caller_count,
            test_caller_count,
        }
    }
}

/// The JSON of `omen diff` and the result of the MCP tool `diff`: the risk of
/// the changes and the change set. The call graph is not here, because its
/// size has no limit.
#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    #[serde(flatten)]
    pub risk: DiffResult,
    /// The short hash of `HEAD`.
    pub head: String,
    pub summary: Summary,
    /// The changed symbols in the order of `SymbolChange::review_order`.
    pub symbols: Vec<SymbolEntry>,
    pub files: Vec<FileChange>,
    pub new_findings: Vec<NewFinding>,
    pub notes: Vec<String>,
    pub security: SecurityChanges,
}

/// Write the risk of the changes. With the JSON format, add the change set.
/// With the Markdown format, write the pull-request comment.
pub fn run(
    path: &Path,
    config: &Config,
    request: &DiffRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let target = request.target.as_deref();
    match format {
        Format::Markdown => write_comment(path, config, request, out),
        Format::Json | Format::JsonCompact => {
            let report = analyze(path, target, config, request.max_callers)?;
            let mut value = serde_json::to_value(&report)?;
            // `--top` and `--offset` page only the symbols, as the MCP tool does.
            if request.top.is_some() || request.offset.is_some() {
                truncate_field(
                    &mut value,
                    PAGED_LIST,
                    request.top.unwrap_or(0),
                    request.offset.unwrap_or(0),
                );
            }
            let mut out = out;
            format.format_value(&value, &mut out)
        }
        _ => {
            let mut out = out;
            format_with_limits(
                serde_json::to_value(risk(path, target)?)?,
                format,
                request.top,
                request.offset,
                &mut out,
            )
        }
    }
}

fn risk(path: &Path, target: Option<&str>) -> Result<DiffResult> {
    crate::analyzers::changes::Analyzer::default().analyze_diff(path, target)
}

/// The risk and the change set of the current branch. The MCP tool `diff`
/// calls this function.
pub fn analyze(
    path: &Path,
    target: Option<&str>,
    config: &Config,
    max_callers: usize,
) -> Result<DiffReport> {
    let (risk, changes) = rayon::join(
        || risk(path, target),
        || -> Result<(ChangeSet, SecurityChanges)> {
            let changes = crate::changeset::build::compute(path, target, config)?;
            let security = crate::changeset::security::compute(path, target, config, &changes)?;
            Ok((changes, security))
        },
    );
    let (changes, security) = changes?;
    Ok(DiffReport {
        risk: risk?,
        head: changes.head,
        summary: changes.summary,
        symbols: changes
            .symbols
            .into_iter()
            .map(|change| SymbolEntry::new(change, max_callers))
            .collect(),
        files: changes.files,
        new_findings: changes.new_findings,
        notes: changes.notes,
        security,
    })
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
    let security =
        crate::changeset::security::compute(path, request.target.as_deref(), config, &changes)?;
    let section = crate::changeset::security::SecuritySection {
        changes: &security,
        max_nodes: request.max_nodes,
    };
    let text = crate::changeset::markdown::render(&changes, &options, &[&section])?;
    out.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::model::{CallerRef, ChangeState};
    use crate::index::model::SymbolKind;

    fn change(callers: &[(&str, bool)]) -> SymbolChange {
        SymbolChange {
            key: "a.rs#run".to_string(),
            name: "run".to_string(),
            kind: SymbolKind::Function,
            file: "a.rs".to_string(),
            line: 1,
            state: ChangeState::Changed,
            signature_before: None,
            signature_after: None,
            complexity_before: None,
            complexity_after: None,
            direct_callers: callers
                .iter()
                .map(|(key, is_test)| CallerRef {
                    key: key.to_string(),
                    file: "a.rs".to_string(),
                    line: 2,
                    is_test: *is_test,
                })
                .collect(),
            transitive_callers: callers.len(),
            weakest_basis: None,
            is_test: false,
        }
    }

    fn caller_keys(entry: &SymbolEntry) -> Vec<&str> {
        entry
            .change
            .direct_callers
            .iter()
            .map(|caller| caller.key.as_str())
            .collect()
    }

    const CALLERS: [(&str, bool); 3] = [("a_test", true), ("b", false), ("c", false)];

    #[test]
    fn test_cap_removes_test_callers_first_and_keeps_the_counts() {
        let entry = SymbolEntry::new(change(&CALLERS), 2);
        assert_eq!(caller_keys(&entry), ["b", "c"]);
        assert_eq!(entry.direct_caller_count, 3);
        assert_eq!(entry.test_caller_count, 1);
        assert_eq!(entry.direct_callers_omitted, 1);
    }

    #[test]
    fn test_cap_of_zero_keeps_each_caller() {
        let entry = SymbolEntry::new(change(&CALLERS), 0);
        assert_eq!(caller_keys(&entry), ["b", "c", "a_test"]);
        assert_eq!(entry.direct_callers_omitted, 0);
    }

    #[test]
    fn test_entry_has_the_keys_of_the_change_and_the_counts() {
        let value = serde_json::to_value(SymbolEntry::new(change(&CALLERS), 1)).unwrap();
        assert_eq!(value["key"], "a.rs#run");
        assert_eq!(value["state"], "changed");
        assert_eq!(value["is_test"], false);
        assert_eq!(value["direct_caller_count"], 3);
        assert_eq!(value["direct_callers_omitted"], 2);
        assert!(value.get("change").is_none());
    }
}
