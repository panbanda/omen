//! The security block of a change set: the sinks, the entry points, and the paths that a
//! change adds or removes, and the changed symbols that are on a path.
//!
//! The block reports facts. It does not say that a change makes the code vulnerable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::markdown::CommentSection;
use super::model::{ChangeSet, ChangeState};
use crate::config::Config;
use crate::core::{ContentSource, FileSet, Result, TreeSource};
use crate::git::GitRepo;
use crate::index::model::ResolutionBasis;
use crate::index::security::analysis::{analyze, Analysis};
use crate::index::security::entry::EntryPoint;
use crate::index::security::reach::SinkPath;
use crate::index::security::Sink;
use crate::index::view::DEFAULT_MIN_BASIS;

/// The rows of a table that the comment lists. The counts in the summary are complete.
const LISTED_ROWS: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityChanges {
    /// The sinks that the head has more than the base.
    pub sinks_added: Vec<Sink>,
    /// The sinks that the base has more than the head.
    pub sinks_removed: Vec<Sink>,
    pub entry_points_added: Vec<EntryPoint>,
    pub entry_points_removed: Vec<EntryPoint>,
    /// The paths of the head that the base does not have, with the keys of the head.
    pub paths_added: Vec<SinkPath>,
    /// The paths of the base that the head does not have, with the keys of the base.
    pub paths_removed: Vec<SinkPath>,
    /// The changed or added symbols of the head that are on a path of the head.
    pub changed_on_path: Vec<String>,
    /// For each added path, the entry point and the sink, as the head has them.
    #[serde(default)]
    pub path_details: Vec<PathDetail>,
}

/// What the diagram of a path needs: the label of the entry point and of the sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathDetail {
    pub entry: String,
    pub sink: String,
    pub entry_label: String,
    pub sink_label: String,
}

impl SecurityChanges {
    pub fn is_empty(&self) -> bool {
        self.sinks_added.is_empty()
            && self.sinks_removed.is_empty()
            && self.entry_points_added.is_empty()
            && self.entry_points_removed.is_empty()
            && self.paths_added.is_empty()
            && self.paths_removed.is_empty()
            && self.changed_on_path.is_empty()
    }
}

fn sink_id(sink: &Sink) -> String {
    format!(
        "{}|{}|{}",
        sink.category.name(),
        sink.enclosing.as_deref().unwrap_or(&sink.file),
        sink.callee_text
    )
}

fn entry_id(entry: &EntryPoint) -> String {
    let route = entry
        .route
        .as_ref()
        .map(|route| format!("{} {}", route.method, route.path))
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}",
        entry.kind.name(),
        entry.framework,
        route,
        entry.symbol.as_deref().unwrap_or(&entry.file)
    )
}

/// The items of `head` that `base` does not have. An item that occurs more often in the head
/// than in the base is added for each extra occurrence.
fn surplus<'a, T>(
    base: &'a [T],
    head: &'a [T],
    identify: impl Fn(&T) -> String,
) -> (Vec<&'a T>, Vec<&'a T>) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in base {
        *counts.entry(identify(item)).or_insert(0) += 1;
    }
    let mut added = Vec::new();
    for item in head {
        match counts.get_mut(&identify(item)) {
            Some(count) if *count > 0 => *count -= 1,
            _ => added.push(item),
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in head {
        *counts.entry(identify(item)).or_insert(0) += 1;
    }
    let mut removed = Vec::new();
    for item in base {
        match counts.get_mut(&identify(item)) {
            Some(count) if *count > 0 => *count -= 1,
            _ => removed.push(item),
        }
    }
    (added, removed)
}

fn path_identity(analysis: &Analysis, path: &SinkPath) -> Option<String> {
    let entry = analysis
        .entry_points
        .iter()
        .find(|entry| entry.key == path.entry)?;
    let sink = analysis.sinks.iter().find(|sink| sink.key == path.sink)?;
    Some(format!("{}=>{}", entry_id(entry), sink_id(sink)))
}

fn entry_label(entry: &EntryPoint) -> String {
    match &entry.route {
        Some(route) => format!("{} {}", route.method, route.path),
        None => format!("{} {}", entry.framework, entry.kind.name()),
    }
}

/// Compares the analysis of the base with the analysis of the head.
///
/// `changed` has the keys of the symbols that the change added or changed.
pub fn diff(base: &Analysis, head: &Analysis, changed: &BTreeSet<String>) -> SecurityChanges {
    let (sinks_added, sinks_removed) = surplus(&base.sinks, &head.sinks, sink_id);
    let (entries_added, entries_removed) =
        surplus(&base.entry_points, &head.entry_points, entry_id);
    let base_paths: BTreeSet<String> = base
        .reachability
        .paths
        .iter()
        .filter_map(|path| path_identity(base, path))
        .collect();
    let head_paths: BTreeSet<String> = head
        .reachability
        .paths
        .iter()
        .filter_map(|path| path_identity(head, path))
        .collect();
    let paths_added: Vec<SinkPath> = head
        .reachability
        .paths
        .iter()
        .filter(|path| path_identity(head, path).is_some_and(|id| !base_paths.contains(&id)))
        .cloned()
        .collect();
    let paths_removed: Vec<SinkPath> = base
        .reachability
        .paths
        .iter()
        .filter(|path| path_identity(base, path).is_some_and(|id| !head_paths.contains(&id)))
        .cloned()
        .collect();
    let on_path: BTreeSet<&str> = head
        .reachability
        .paths
        .iter()
        .flat_map(|path| path.steps.iter().map(|step| step.symbol.as_str()))
        .collect();
    let changed_on_path = changed
        .iter()
        .filter(|key| on_path.contains(key.as_str()))
        .cloned()
        .collect();
    let path_details = paths_added
        .iter()
        .filter_map(|path| {
            let entry = head
                .entry_points
                .iter()
                .find(|entry| entry.key == path.entry)?;
            let sink = head.sinks.iter().find(|sink| sink.key == path.sink)?;
            Some(PathDetail {
                entry: path.entry.clone(),
                sink: path.sink.clone(),
                entry_label: entry_label(entry),
                sink_label: format!("{} (CWE-{})", sink.category.name(), sink.cwe),
            })
        })
        .collect();
    SecurityChanges {
        sinks_added: sinks_added.into_iter().cloned().collect(),
        sinks_removed: sinks_removed.into_iter().cloned().collect(),
        entry_points_added: entries_added.into_iter().cloned().collect(),
        entry_points_removed: entries_removed.into_iter().cloned().collect(),
        paths_added,
        paths_removed,
        changed_on_path,
        path_details,
    }
}

/// Analyzes the tree of one commit.
fn analyze_revision(
    repository: &Path,
    revision: &str,
    config: &Config,
    min_basis: ResolutionBasis,
) -> Result<Analysis> {
    let source = TreeSource::new(repository, revision)?;
    let file_set = FileSet::from_tree_source(&source, config)?;
    let files: Vec<PathBuf> = file_set.iter().cloned().collect();
    Ok(analyze(
        Path::new(""),
        &files,
        &|path| source.read(path),
        &|directory| source.list_directory(directory).unwrap_or_default(),
        min_basis,
    ))
}

/// Computes the security block for the change set of `HEAD` against `target`.
pub fn compute(
    repository: &Path,
    target: Option<&str>,
    config: &Config,
    changes: &ChangeSet,
) -> Result<SecurityChanges> {
    let target = match target {
        Some(target) => target.to_string(),
        None => crate::analyzers::changes::detect_default_branch(repository)?,
    };
    let repo = GitRepo::open(repository)?;
    let base_sha = repo.merge_base(&target, "HEAD")?;
    let head_sha = repo.head_sha()?;
    let base = analyze_revision(repository, &base_sha, config, DEFAULT_MIN_BASIS)?;
    let head = analyze_revision(repository, &head_sha, config, DEFAULT_MIN_BASIS)?;
    let changed: BTreeSet<String> = changes
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.state, ChangeState::Added | ChangeState::Changed))
        .map(|symbol| symbol.key.clone())
        .collect();
    Ok(diff(&base, &head, &changed))
}

/// The section "Security" of the pull-request comment.
pub struct SecuritySection<'a> {
    pub changes: &'a SecurityChanges,
    /// The node cap for all diagrams together.
    pub max_nodes: usize,
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn name_of(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(_, name)| name)
}

fn location(file: &str, line: u32) -> String {
    format!("`{}:{}`", cell(file), line)
}

impl SecuritySection<'_> {
    fn summary(&self) -> String {
        let changes = self.changes;
        let rows = [
            ("Sinks added", changes.sinks_added.len()),
            ("Sinks removed", changes.sinks_removed.len()),
            ("Entry points added", changes.entry_points_added.len()),
            ("Entry points removed", changes.entry_points_removed.len()),
            ("Paths added", changes.paths_added.len()),
            ("Paths removed", changes.paths_removed.len()),
            ("Changed symbols on a path", changes.changed_on_path.len()),
        ];
        let mut text = String::from(
            "### Security\n\nOmen reports sinks and paths as facts. It does not say that the code has a vulnerability.\n\n| Measure | Value |\n| --- | --- |\n",
        );
        for (name, count) in rows {
            text.push_str(&format!("| {name} | {count} |\n"));
        }
        text
    }

    fn sink_rows(&self) -> String {
        let mut text = String::new();
        let sinks = &self.changes.sinks_added;
        if sinks.is_empty() {
            return text;
        }
        text.push_str(
            "\n<details><summary>New sinks</summary>\n\n| Category | CWE | Location | Basis | Callee |\n| --- | --- | --- | --- | --- |\n",
        );
        for sink in sinks.iter().take(LISTED_ROWS) {
            text.push_str(&format!(
                "| {} | {} | {} | {} | `{}` |\n",
                sink.category.name(),
                sink.cwe,
                location(&sink.file, sink.range.start_line),
                sink.basis.name(),
                cell(&sink.callee_text)
            ));
        }
        if sinks.len() > LISTED_ROWS {
            text.push_str(&format!(
                "\n{} more sinks are not listed.\n",
                sinks.len() - LISTED_ROWS
            ));
        }
        text.push_str("\n</details>\n");
        text
    }

    fn entry_rows(&self) -> String {
        let entries = &self.changes.entry_points_added;
        if entries.is_empty() {
            return String::new();
        }
        let mut text = String::from(
            "\n<details><summary>New entry points</summary>\n\n| Kind | Framework | Route | Guards | Location |\n| --- | --- | --- | --- | --- |\n",
        );
        for entry in entries.iter().take(LISTED_ROWS) {
            let route = entry
                .route
                .as_ref()
                .map(|route| format!("`{} {}`", route.method, cell(&route.path)))
                .unwrap_or_default();
            text.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                entry.kind.name(),
                entry.framework,
                route,
                cell(&entry.guards.join(", ")),
                location(&entry.file, entry.range.start_line)
            ));
        }
        text.push_str("\n</details>\n");
        text
    }

    fn changed_rows(&self) -> String {
        let keys = &self.changes.changed_on_path;
        if keys.is_empty() {
            return String::new();
        }
        let mut text = String::from(
            "\n<details><summary>Changed symbols on an entry-to-sink path</summary>\n\n",
        );
        for key in keys.iter().take(LISTED_ROWS) {
            text.push_str(&format!("- `{}`\n", cell(key)));
        }
        text.push_str("\n</details>\n");
        text
    }

    /// One diagram for one added path: entry point, hops, sink. A changed hop has the
    /// style `changed`. The diagram has at most `max_nodes` nodes.
    fn diagram(&self, path: &SinkPath, max_nodes: usize) -> Option<(String, usize)> {
        let detail = self
            .changes
            .path_details
            .iter()
            .find(|detail| detail.entry == path.entry && detail.sink == path.sink)?;
        let changed: BTreeSet<&str> = self
            .changes
            .changed_on_path
            .iter()
            .map(String::as_str)
            .collect();
        // The first node is the entry point, then the hops, and the last node is the sink.
        let mut labels: Vec<(String, bool)> = vec![(detail.entry_label.clone(), false)];
        for step in &path.steps {
            labels.push((
                name_of(&step.symbol).to_string(),
                changed.contains(step.symbol.as_str()),
            ));
        }
        labels.push((detail.sink_label.clone(), false));
        if max_nodes < 3 {
            return None;
        }
        if labels.len() > max_nodes {
            let omitted = labels.len() - max_nodes + 1;
            let keep_tail = (max_nodes - 1) / 2;
            let keep_head = max_nodes - 1 - keep_tail;
            let tail: Vec<(String, bool)> = labels[labels.len() - keep_tail..].to_vec();
            labels.truncate(keep_head);
            labels.push((format!("+{omitted} more"), false));
            labels.extend(tail);
        }
        let mut text = String::from("flowchart LR\n");
        for (position, (label, _)) in labels.iter().enumerate() {
            text.push_str(&format!(
                "  p{position}[\"{}\"]\n",
                super::mermaid::escape(label)
            ));
        }
        for position in 1..labels.len() {
            text.push_str(&format!("  p{} --> p{position}\n", position - 1));
        }
        text.push_str("  classDef changed fill:#fff8c5,stroke:#9a6700,color:#1f2328\n");
        let marked: Vec<String> = labels
            .iter()
            .enumerate()
            .filter(|(_, (_, is_changed))| *is_changed)
            .map(|(position, _)| format!("p{position}"))
            .collect();
        if !marked.is_empty() {
            text.push_str(&format!("  class {} changed\n", marked.join(",")));
        }
        let used = labels.len();
        Some((text, used))
    }
}

impl CommentSection for SecuritySection<'_> {
    fn render(&self, budget: usize) -> String {
        if self.changes.is_empty() {
            return String::new();
        }
        let mut text = self.summary();
        for part in [self.sink_rows(), self.entry_rows(), self.changed_rows()] {
            if text.len() + part.len() <= budget {
                text.push_str(&part);
            }
        }
        let mut nodes_left = self.max_nodes;
        let mut omitted = 0usize;
        for path in &self.changes.paths_added {
            let block = self.diagram(path, nodes_left).map(|(diagram, used)| {
                (
                    format!(
                        "\n<details><summary>New path to `{}`</summary>\n\n```mermaid\n{diagram}```\n\n</details>\n",
                        cell(&path.sink)
                    ),
                    used,
                )
            });
            match block {
                Some((block, used)) if text.len() + block.len() <= budget => {
                    text.push_str(&block);
                    nodes_left -= used.min(nodes_left);
                }
                _ => omitted += 1,
            }
        }
        if omitted > 0 {
            let note = format!("\n{omitted} new paths are not drawn, because of the node cap or the byte budget.\n");
            if text.len() + note.len() <= budget {
                text.push_str(&note);
            }
        }
        if text.len() > budget {
            // The summary alone does not fit. The comment drops an empty section.
            let summary = self.summary();
            return if summary.len() <= budget {
                summary
            } else {
                String::new()
            };
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::security::analysis::analyze_sources;

    const BASE: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry():\n    return helper()\n\ndef helper():\n    return 1\n";
    const HEAD: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.get(\"/a\")\ndef entry():\n    return helper()\n\n@app.post(\"/b\")\ndef extra(command):\n    return helper()\n\ndef helper():\n    os.system(\"x\" + os.environ[\"A\"])\n";

    fn changes() -> SecurityChanges {
        let base = analyze_sources(&[("app.py", BASE)], DEFAULT_MIN_BASIS);
        let head = analyze_sources(&[("app.py", HEAD)], DEFAULT_MIN_BASIS);
        let changed: BTreeSet<String> = ["app.py#helper".to_string(), "app.py#extra".to_string()]
            .into_iter()
            .collect();
        diff(&base, &head, &changed)
    }

    #[test]
    fn a_new_sink_a_new_entry_point_and_new_paths_are_found() {
        let found = changes();
        assert_eq!(found.sinks_added.len(), 1);
        assert_eq!(found.sinks_added[0].category.name(), "command_execution");
        assert!(found.sinks_removed.is_empty());
        assert_eq!(found.entry_points_added.len(), 1);
        assert_eq!(
            found.entry_points_added[0].route.as_ref().unwrap().path,
            "/b"
        );
        assert_eq!(
            found.paths_added.len(),
            2,
            "both entry points reach the sink"
        );
        assert!(found.paths_removed.is_empty());
        assert_eq!(found.changed_on_path, ["app.py#extra", "app.py#helper"]);
    }

    #[test]
    fn a_removed_sink_is_found_when_the_sides_swap() {
        let base = analyze_sources(&[("app.py", HEAD)], DEFAULT_MIN_BASIS);
        let head = analyze_sources(&[("app.py", BASE)], DEFAULT_MIN_BASIS);
        let found = diff(&base, &head, &BTreeSet::new());
        assert_eq!(found.sinks_removed.len(), 1);
        assert_eq!(found.paths_removed.len(), 2);
        assert_eq!(found.entry_points_removed.len(), 1);
        assert!(found.sinks_added.is_empty());
    }

    #[test]
    fn a_moved_line_is_not_a_new_sink() {
        let moved = format!("\n\n{HEAD}");
        let base = analyze_sources(&[("app.py", HEAD)], DEFAULT_MIN_BASIS);
        let head = analyze_sources(&[("app.py", moved.as_str())], DEFAULT_MIN_BASIS);
        assert!(diff(&base, &head, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn the_section_has_the_counts_the_tables_and_one_diagram_for_each_new_path() {
        let found = changes();
        let section = SecuritySection {
            changes: &found,
            max_nodes: 40,
        };
        let text = section.render(usize::MAX);
        assert!(text.contains("| Sinks added | 1 |"), "{text}");
        assert!(text.contains("<summary>New sinks</summary>"));
        assert!(text.contains("<summary>New entry points</summary>"));
        assert_eq!(text.matches("```mermaid").count(), 2);
        assert!(text.contains("flowchart LR"));
        assert!(text.contains("classDef changed"));
        assert!(text.contains("command_execution (CWE-78)"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_node_cap_limits_the_diagrams_and_the_section_says_so() {
        let found = changes();
        let section = SecuritySection {
            changes: &found,
            max_nodes: 4,
        };
        let text = section.render(usize::MAX);
        assert_eq!(text.matches("```mermaid").count(), 1);
        assert!(text.contains("1 new paths are not drawn"), "{text}");
    }

    #[test]
    fn the_byte_budget_is_hard() {
        let found = changes();
        let section = SecuritySection {
            changes: &found,
            max_nodes: 40,
        };
        for budget in [0usize, 100, 400, 800, 1500, 3000] {
            let text = section.render(budget);
            assert!(text.len() <= budget, "{budget}: {}", text.len());
        }
    }

    #[test]
    fn a_change_without_a_security_difference_gives_an_empty_section() {
        let section = SecuritySection {
            changes: &SecurityChanges::default(),
            max_nodes: 40,
        };
        assert_eq!(section.render(usize::MAX), "");
    }
}
