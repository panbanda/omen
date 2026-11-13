//! The Markdown comment for a pull request.
//!
//! A CI step pipes the text into `gh pr comment --body-file -`. Omen does
//! not call a forge API.

use super::mermaid;
use super::model::{ChangeSet, FileChange, FindingKind, NewFinding, SymbolChange};

/// The first line of the comment. A CI step finds its own comment with it.
pub const MARKER: &str = "<!-- omen:diff -->";

/// The limit of GitHub is 65536 bytes. The default leaves room for the
/// text that the CI step adds.
pub const DEFAULT_MAX_BYTES: usize = 60000;

pub const DEFAULT_MAX_NODES: usize = 30;

#[derive(Debug, Clone)]
pub struct CommentOptions {
    /// The byte budget. The value 0 removes the limit.
    pub max_bytes: usize,
    pub max_nodes: usize,
}

impl Default for CommentOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_nodes: DEFAULT_MAX_NODES,
        }
    }
}

/// A section that a later feature adds to the comment, for example the
/// security section. The comment writes the sections after its own sections,
/// in slice order.
pub trait CommentSection {
    /// The text of the section. The text must fit in `budget` bytes, and it
    /// must end with a newline. A section that has nothing to say gives an
    /// empty text. The comment drops a text that is too long.
    fn render(&self, budget: usize) -> String;
}

/// The text of the comment. The text is not longer than `options.max_bytes`,
/// except when the title and the summary alone are longer.
///
/// The extra sections have priority over the own sections that can lose
/// rows. An extra section is in the text with all of its content or not at all.
pub fn render(
    changes: &ChangeSet,
    options: &CommentOptions,
    extra: &[&dyn CommentSection],
) -> String {
    let mut text = header(changes);
    let limit = if options.max_bytes == 0 {
        usize::MAX
    } else {
        options.max_bytes
    };
    let mut remaining = limit.saturating_sub(text.len());

    let mut extra_texts = Vec::new();
    for section in extra {
        let section_text = section.render(remaining);
        if section_text.len() <= remaining {
            remaining -= section_text.len();
            extra_texts.push(section_text);
        }
    }

    let sections = own_sections(changes, options);
    for section_text in fill_budget(&sections, remaining) {
        text.push_str(&section_text);
    }
    for section_text in extra_texts {
        text.push_str(&section_text);
    }
    text
}

type Section<'a> = (usize, Box<dyn Fn(usize) -> String + 'a>);

/// Gives each section a share of the budget in proportion to its weight. A
/// section that needs less than its share keeps the rest for the others.
fn fill_budget(sections: &[Section<'_>], budget: usize) -> Vec<String> {
    let full: Vec<String> = sections
        .iter()
        .map(|(_, build)| build(usize::MAX))
        .collect();
    let mut fixed = vec![false; sections.len()];
    let mut remaining = budget;
    loop {
        let weight_left: usize = (0..sections.len())
            .filter(|index| !fixed[*index])
            .map(|index| sections[index].0)
            .sum();
        let share = |index: usize| proportion(remaining, sections[index].0, weight_left);
        let next =
            (0..sections.len()).find(|index| !fixed[*index] && full[*index].len() <= share(*index));
        match next {
            Some(index) => {
                fixed[index] = true;
                remaining -= full[index].len();
            }
            None => {
                return (0..sections.len())
                    .map(|index| {
                        if fixed[index] {
                            full[index].clone()
                        } else {
                            sections[index].1(share(index))
                        }
                    })
                    .collect();
            }
        }
    }
}

/// `total * part / whole`. The budget can be `usize::MAX`, thus the product uses 128 bits.
fn proportion(total: usize, part: usize, whole: usize) -> usize {
    (total as u128 * part as u128 / whole.max(1) as u128) as usize
}

fn header(changes: &ChangeSet) -> String {
    let summary = &changes.summary;
    let delta = i64::from(summary.cyclomatic_after) - i64::from(summary.cyclomatic_before);
    let rows = [
        (
            "Files changed",
            format!(
                "{} (+{} -{} lines)",
                summary.files_changed, summary.lines_added, summary.lines_deleted
            ),
        ),
        (
            "Symbols",
            format!(
                "+{} added, -{} removed, ~{} changed",
                summary.symbols_added, summary.symbols_removed, summary.symbols_changed
            ),
        ),
        ("Callers affected", summary.callers_affected.to_string()),
        (
            "Cyclomatic complexity of the changed symbols",
            format!(
                "{} -> {} ({delta:+})",
                summary.cyclomatic_before, summary.cyclomatic_after
            ),
        ),
        ("New stubs", summary.new_stubs.to_string()),
        ("New debt comments", summary.new_satd.to_string()),
        ("Hotspots touched", summary.hotspots_touched.to_string()),
    ];
    let mut text = format!(
        "{MARKER}\n## Omen diff\n\nBase `{}` (merge base with `{}`), head `{}`.\n\n| Measure | Value |\n| --- | --- |\n",
        changes.base, changes.target, changes.head
    );
    for (name, value) in rows {
        text.push_str(&format!("| {name} | {value} |\n"));
    }
    text
}

fn own_sections<'a>(changes: &'a ChangeSet, options: &'a CommentOptions) -> Vec<Section<'a>> {
    vec![
        (4, Box::new(|budget| symbols_section(changes, budget))),
        (1, Box::new(|budget| signatures_section(changes, budget))),
        (
            3,
            Box::new(|budget| graph_section(changes, options, budget)),
        ),
        (2, Box::new(|budget| files_section(changes, budget))),
        (1, Box::new(|budget| findings_section(changes, budget))),
        (2, Box::new(|budget| callers_section(changes, budget))),
        (1, Box::new(|budget| owners_section(changes, budget))),
    ]
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn code(text: &str) -> String {
    format!("`{}`", cell(text))
}

/// The key of a symbol without the file, for example `Circle.area`.
fn display(key: &str) -> &str {
    key.split_once('#').map_or(key, |(_, name)| name)
}

/// The head, the rows, and the tail of one section that can lose rows.
struct Rows<'a> {
    head: String,
    rows: Vec<String>,
    tail: &'a str,
    /// The text for the number of rows that the budget cuts.
    note: Box<dyn Fn(usize) -> String + 'a>,
}

impl Rows<'_> {
    /// The note between the last row and the tail. In a code fence the note
    /// is a line of the fenced text.
    fn note_in_section(&self, count: usize) -> String {
        let note = (self.note)(count);
        if self.tail.starts_with("```") {
            format!("# {}\n", note.trim().trim_matches('_'))
        } else {
            note
        }
    }

    fn fit(&self, budget: usize) -> String {
        let total = self.rows.len();
        let whole = self.head.len() + self.rows.iter().map(String::len).sum::<usize>();
        if whole + self.tail.len() <= budget {
            return format!("{}{}{}", self.head, self.rows.concat(), self.tail);
        }
        let reserve = self.note_in_section(total).len() + self.tail.len();
        let mut used = self.head.len() + reserve;
        let mut kept = 0;
        for row in &self.rows {
            if used + row.len() > budget {
                break;
            }
            used += row.len();
            kept += 1;
        }
        if kept == 0 {
            let line = (self.note)(total);
            return if line.len() <= budget {
                line
            } else {
                String::new()
            };
        }
        format!(
            "{}{}{}{}",
            self.head,
            self.rows[..kept].concat(),
            self.note_in_section(total - kept),
            self.tail
        )
    }
}

fn details(title: &str, count: usize) -> String {
    format!("<details>\n<summary>{title} ({count})</summary>\n\n")
}

const DETAILS_END: &str = "\n</details>\n";

fn omitted(count: usize, what: &str, command: &str) -> String {
    format!("\n_{count} {what} omitted. Run `{command}` for the full list._\n")
}

fn full_command(changes: &ChangeSet) -> String {
    format!("omen diff -t {} -f markdown --max-bytes 0", changes.target)
}

fn sorted_symbols(changes: &ChangeSet) -> Vec<&SymbolChange> {
    let mut symbols: Vec<&SymbolChange> = changes.symbols.iter().collect();
    symbols.sort_by(|left, right| {
        (&left.file, left.line, &left.key).cmp(&(&right.file, right.line, &right.key))
    });
    symbols
}

fn complexity_cell(symbol: &SymbolChange) -> String {
    let side = |value: Option<u32>| value.map_or("none".to_string(), |n| n.to_string());
    format!(
        "CC {} -> {}, CG {} -> {}",
        side(symbol.complexity_before.map(|c| c.cyclomatic)),
        side(symbol.complexity_after.map(|c| c.cyclomatic)),
        side(symbol.complexity_before.map(|c| c.cognitive)),
        side(symbol.complexity_after.map(|c| c.cognitive)),
    )
}

fn symbols_section(changes: &ChangeSet, budget: usize) -> String {
    let symbols = sorted_symbols(changes);
    if symbols.is_empty() {
        return String::new();
    }
    let rows = symbols
        .iter()
        .map(|symbol| {
            format!(
                "| {} {} | {} | {} | {} | {} / {} | {} | {} |\n",
                symbol.state.prefix(),
                symbol.state.name(),
                code(display(&symbol.key)),
                code(&format!("{}:{}", symbol.file, symbol.line)),
                complexity_cell(symbol),
                symbol.direct_callers.len(),
                symbol.transitive_callers,
                symbol.test_callers(),
                symbol.weakest_basis.as_deref().unwrap_or("none"),
            )
        })
        .collect();
    let command = full_command(changes);
    Rows {
        head: "\n### Changed symbols\n\n| Change | Symbol | Location | Complexity | Direct / transitive callers | Tests | Weakest edge |\n| --- | --- | --- | --- | --- | --- | --- |\n".to_string(),
        rows,
        tail: "",
        note: Box::new(move |count| omitted(count, "symbols", &command)),
    }
    .fit(budget)
}

fn signatures_section(changes: &ChangeSet, budget: usize) -> String {
    let rows: Vec<String> = sorted_symbols(changes)
        .into_iter()
        .filter(|symbol| symbol.signature_changed())
        .map(|symbol| {
            let one_line = |text: &Option<String>| {
                text.as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            format!(
                "@@ {} @@\n- {}\n+ {}\n",
                symbol.key,
                one_line(&symbol.signature_before),
                one_line(&symbol.signature_after)
            )
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    let command = full_command(changes);
    Rows {
        head: "\n### Signature changes\n\n```diff\n".to_string(),
        rows,
        tail: "```\n",
        note: Box::new(move |count| omitted(count, "signature changes", &command)),
    }
    .fit(budget)
}

fn graph_section(changes: &ChangeSet, options: &CommentOptions, budget: usize) -> String {
    let frame = |diagram: &str| {
        format!(
            "\n### Call graph\n\nLegend: `+` added, `-` removed, `~` changed, no sign for context. A dashed line is a removed call.\n\n```mermaid\n{diagram}```\n"
        )
    };
    let mut nodes = options.max_nodes.min(changes.graph.nodes.len());
    loop {
        let Some(diagram) = mermaid::render(&changes.graph, nodes) else {
            return String::new();
        };
        let text = frame(&diagram);
        if text.len() <= budget {
            return text;
        }
        if nodes == 0 {
            return String::new();
        }
        nodes -= 1;
    }
}

fn files_section(changes: &ChangeSet, budget: usize) -> String {
    if changes.files.is_empty() {
        return String::new();
    }
    let mut files: Vec<&FileChange> = changes.files.iter().collect();
    files.sort_by(|left, right| left.file.cmp(&right.file));
    let rows = files
        .iter()
        .map(|file| {
            format!(
                "| {} {} | {} | +{} -{} | {} | {} | {} |\n",
                file.state.prefix(),
                file.state.name(),
                code(&file.file),
                file.additions,
                file.deletions,
                file.symbols_changed,
                file.max_cyclomatic
                    .map_or("none".to_string(), |n| n.to_string()),
                file.hotspot.as_deref().unwrap_or("none"),
            )
        })
        .collect();
    let command = full_command(changes);
    Rows {
        head: format!(
            "\n{}| Change | File | Lines | Changed symbols | Max cyclomatic | Hotspot |\n| --- | --- | --- | --- | --- | --- |\n",
            details("Changed files", files.len())
        ),
        rows,
        tail: DETAILS_END,
        note: Box::new(move |count| omitted(count, "files", &command)),
    }
    .fit(budget)
}

fn findings_section(changes: &ChangeSet, budget: usize) -> String {
    if changes.new_findings.is_empty() {
        return String::new();
    }
    let mut findings: Vec<&NewFinding> = changes.new_findings.iter().collect();
    findings.sort_by(|left, right| {
        (&left.file, left.line, &left.detail).cmp(&(&right.file, right.line, &right.detail))
    });
    let rows = findings
        .iter()
        .map(|finding| {
            let kind = match finding.kind {
                FindingKind::Stub => "stub",
                FindingKind::Satd => "debt comment",
            };
            format!(
                "- {kind}: {} {}\n",
                code(&format!("{}:{}", finding.file, finding.line)),
                code(&finding.detail)
            )
        })
        .collect();
    let command = full_command(changes);
    Rows {
        head: format!(
            "\n{}",
            details("New stubs and debt comments", findings.len())
        ),
        rows,
        tail: DETAILS_END,
        note: Box::new(move |count| omitted(count, "findings", &command)),
    }
    .fit(budget)
}

fn callers_section(changes: &ChangeSet, budget: usize) -> String {
    let symbols: Vec<&SymbolChange> = sorted_symbols(changes)
        .into_iter()
        .filter(|symbol| !symbol.direct_callers.is_empty())
        .collect();
    if symbols.is_empty() {
        return String::new();
    }
    let rows = symbols
        .iter()
        .map(|symbol| {
            let callers: Vec<String> = symbol
                .direct_callers
                .iter()
                .map(|caller| {
                    let test = if caller.is_test { " (test)" } else { "" };
                    format!(
                        "{} at {}{test}",
                        code(display(&caller.key)),
                        code(&format!("{}:{}", caller.file, caller.line))
                    )
                })
                .collect();
            format!(
                "- {} is called by {}.\n",
                code(display(&symbol.key)),
                callers.join(", ")
            )
        })
        .collect();
    Rows {
        head: format!(
            "\n{}",
            details("Callers of the changed symbols", symbols.len())
        ),
        rows,
        tail: DETAILS_END,
        note: Box::new(|count| omitted(count, "symbols", "omen impact SYMBOL --direction callers")),
    }
    .fit(budget)
}

fn owners_section(changes: &ChangeSet, budget: usize) -> String {
    let mut files: Vec<&FileChange> = changes
        .files
        .iter()
        .filter(|file| !file.owners.is_empty())
        .collect();
    if files.is_empty() {
        return String::new();
    }
    files.sort_by(|left, right| left.file.cmp(&right.file));
    let rows = files
        .iter()
        .map(|file| {
            format!(
                "| {} | {} |\n",
                code(&file.file),
                cell(&file.owners.join(", "))
            )
        })
        .collect();
    Rows {
        head: format!(
            "\n{}| File | Authors |\n| --- | --- |\n",
            details("Owners of the changed files", files.len())
        ),
        rows,
        tail: DETAILS_END,
        note: Box::new(|count| omitted(count, "files", "omen ownership")),
    }
    .fit(budget)
}

#[cfg(test)]
mod tests {
    use super::super::graph::fixtures::{edge, node};
    use super::super::mermaid::tests::check_subset;
    use super::super::model::*;
    use super::*;
    use crate::index::model::SymbolKind;

    fn symbol(
        key: &str,
        state: ChangeState,
        before: Option<u32>,
        after: Option<u32>,
    ) -> SymbolChange {
        let (file, name) = key.split_once('#').unwrap();
        SymbolChange {
            key: key.to_string(),
            name: name.rsplit('.').next().unwrap().to_string(),
            kind: SymbolKind::Function,
            file: file.to_string(),
            line: 10,
            state,
            signature_before: before.map(|_| format!("fn {name}(a: i32)")),
            signature_after: after.map(|_| format!("fn {name}(a: i32, b: i32)")),
            complexity_before: before.map(|n| Complexity {
                cyclomatic: n,
                cognitive: n + 1,
            }),
            complexity_after: after.map(|n| Complexity {
                cyclomatic: n,
                cognitive: n + 2,
            }),
            direct_callers: vec![CallerRef {
                key: "app/main.rs#main".to_string(),
                file: "app/main.rs".to_string(),
                line: 4,
                is_test: false,
            }],
            transitive_callers: 3,
            weakest_basis: Some("same_file".to_string()),
        }
    }

    fn sample() -> ChangeSet {
        ChangeSet {
            target: "main".to_string(),
            base: "aaaaaaa".to_string(),
            head: "bbbbbbb".to_string(),
            summary: Summary {
                files_changed: 2,
                lines_added: 20,
                lines_deleted: 5,
                symbols_added: 1,
                symbols_removed: 1,
                symbols_changed: 1,
                callers_affected: 1,
                cyclomatic_before: 3,
                cyclomatic_after: 5,
                new_stubs: 1,
                new_satd: 2,
                hotspots_touched: 1,
            },
            symbols: vec![
                symbol("src/a.rs#run", ChangeState::Changed, Some(3), Some(5)),
                symbol("src/a.rs#helper", ChangeState::Added, None, Some(1)),
                symbol("src/b.rs#old", ChangeState::Removed, Some(2), None),
            ],
            files: vec![FileChange {
                file: "src/a.rs".to_string(),
                state: ChangeState::Changed,
                additions: 20,
                deletions: 5,
                symbols_changed: 2,
                max_cyclomatic: Some(5),
                hotspot: Some("high".to_string()),
                owners: vec!["ada".to_string(), "grace".to_string()],
            }],
            new_findings: vec![NewFinding {
                kind: FindingKind::Stub,
                file: "src/a.rs".to_string(),
                line: 12,
                detail: "todo!()".to_string(),
            }],
            graph: ChangeGraph {
                nodes: vec![
                    node("src/a.rs#run", "src", ChangeState::Changed),
                    node("src/a.rs#helper", "src", ChangeState::Added),
                    node("app/main.rs#main", "app", ChangeState::Unchanged),
                ],
                edges: vec![
                    edge("app/main.rs#main", "src/a.rs#run", ChangeState::Unchanged),
                    edge("src/a.rs#run", "src/a.rs#helper", ChangeState::Added),
                ],
            },
        }
    }

    fn large() -> ChangeSet {
        let mut changes = sample();
        changes.symbols = (0..300)
            .map(|n| {
                symbol(
                    &format!("src/module{n:03}.rs#function"),
                    ChangeState::Changed,
                    Some(3),
                    Some(5),
                )
            })
            .collect();
        changes.files = (0..300)
            .map(|n| FileChange {
                file: format!("src/module{n:03}.rs"),
                owners: vec!["ada".to_string()],
                ..changes.files[0].clone()
            })
            .collect();
        changes
    }

    fn default_render(changes: &ChangeSet) -> String {
        render(changes, &CommentOptions::default(), &[])
    }

    #[test]
    fn test_comment_starts_with_the_marker() {
        let text = default_render(&sample());
        assert_eq!(text.lines().next(), Some(MARKER));
    }

    #[test]
    fn test_summary_table_gives_the_counts() {
        let text = default_render(&sample());
        assert!(
            text.contains("| Files changed | 2 (+20 -5 lines) |"),
            "{text}"
        );
        assert!(
            text.contains("| Symbols | +1 added, -1 removed, ~1 changed |"),
            "{text}"
        );
        assert!(text.contains("| Callers affected | 1 |"), "{text}");
        assert!(
            text.contains("| Cyclomatic complexity of the changed symbols | 3 -> 5 (+2) |"),
            "{text}"
        );
        assert!(text.contains("| New stubs | 1 |"), "{text}");
        assert!(text.contains("| New debt comments | 2 |"), "{text}");
        assert!(text.contains("| Hotspots touched | 1 |"), "{text}");
    }

    #[test]
    fn test_symbol_table_has_a_sign_for_each_change() {
        let text = default_render(&sample());
        assert!(
            text.contains("| ~ changed | `run` | `src/a.rs:10` |"),
            "{text}"
        );
        assert!(text.contains("| + added | `helper` |"), "{text}");
        assert!(text.contains("| - removed | `old` |"), "{text}");
    }

    #[test]
    fn test_symbol_table_gives_complexity_callers_and_basis() {
        let text = default_render(&sample());
        assert!(
            text.contains("| CC 3 -> 5, CG 4 -> 7 | 1 / 3 | 0 | same_file |"),
            "{text}"
        );
        assert!(text.contains("| CC none -> 1, CG none -> 3 |"), "{text}");
    }

    #[test]
    fn test_signature_changes_use_a_diff_block() {
        let text = default_render(&sample());
        let block = text.split("```diff\n").nth(1).expect("diff block");
        assert!(
            block.contains("- fn run(a: i32)\n+ fn run(a: i32, b: i32)\n"),
            "{text}"
        );
    }

    #[test]
    fn test_mermaid_block_is_in_the_subset() {
        let text = default_render(&sample());
        let block = text
            .split("```mermaid\n")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .expect("mermaid block");
        check_subset(block);
    }

    #[test]
    fn test_long_lists_are_in_details_blocks() {
        let text = default_render(&sample());
        assert!(text.matches("<details>").count() >= 3, "{text}");
        assert_eq!(
            text.matches("<details>").count(),
            text.matches("</details>").count()
        );
        assert!(
            text.contains("<summary>Changed files (1)</summary>"),
            "{text}"
        );
        assert!(
            text.contains("<summary>Owners of the changed files (1)</summary>"),
            "{text}"
        );
        assert!(
            text.contains("<summary>Callers of the changed symbols (3)</summary>"),
            "{text}"
        );
    }

    #[test]
    fn test_two_runs_give_the_same_bytes() {
        assert_eq!(default_render(&large()), default_render(&large()));
    }

    #[test]
    fn test_order_of_the_input_does_not_change_the_text() {
        let mut shuffled = sample();
        shuffled.symbols.reverse();
        shuffled.graph.nodes.reverse();
        shuffled.graph.edges.reverse();
        assert_eq!(default_render(&sample()), default_render(&shuffled));
    }

    #[test]
    fn test_budget_limits_the_size_of_the_text() {
        let options = CommentOptions {
            max_bytes: 6000,
            ..CommentOptions::default()
        };
        let text = render(&large(), &options, &[]);
        assert!(text.len() <= 6000, "{} bytes", text.len());
    }

    #[test]
    fn test_budget_names_the_omitted_rows_and_the_command() {
        let options = CommentOptions {
            max_bytes: 6000,
            ..CommentOptions::default()
        };
        let text = render(&large(), &options, &[]);
        let note = text
            .lines()
            .find(|line| line.contains("symbols omitted"))
            .expect("note");
        assert!(
            note.contains("omen diff -t main -f markdown --max-bytes 0"),
            "{note}"
        );
        let omitted: usize = note
            .trim_start_matches('_')
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let rows = text.matches("| ~ changed | `function` |").count();
        assert_eq!(rows + omitted, 300);
    }

    #[test]
    fn test_omission_note_is_a_paragraph() {
        for max_bytes in (1200..6000).step_by(250) {
            let options = CommentOptions {
                max_bytes,
                ..CommentOptions::default()
            };
            let text = render(&large(), &options, &[]);
            let lines: Vec<&str> = text.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if line.starts_with('_') && line.ends_with('_') {
                    assert!(lines[index - 1].is_empty(), "{max_bytes}: {line}");
                }
            }
        }
    }

    #[test]
    fn test_budget_keeps_every_section_that_has_rows() {
        let options = CommentOptions {
            max_bytes: 6000,
            ..CommentOptions::default()
        };
        let text = render(&large(), &options, &[]);
        assert!(text.contains("```mermaid"), "{text}");
        assert!(text.contains("<summary>Changed files"), "{text}");
        assert_eq!(
            text.matches("<details>").count(),
            text.matches("</details>").count()
        );
    }

    #[test]
    fn test_budget_of_zero_removes_the_limit() {
        let options = CommentOptions {
            max_bytes: 0,
            ..CommentOptions::default()
        };
        let text = render(&large(), &options, &[]);
        assert_eq!(text.matches("| ~ changed | `function` |").count(), 300);
        assert!(!text.contains("omitted"));
    }

    #[test]
    fn test_default_budget_is_under_the_limit_of_github() {
        const { assert!(DEFAULT_MAX_BYTES < 65536) };
        assert!(default_render(&large()).len() <= DEFAULT_MAX_BYTES);
    }

    struct Extra(&'static str);

    impl CommentSection for Extra {
        fn render(&self, _budget: usize) -> String {
            self.0.to_string()
        }
    }

    #[test]
    fn test_extra_sections_follow_the_own_sections() {
        let extra = Extra("### Extra\n");
        let text = render(&sample(), &CommentOptions::default(), &[&extra]);
        assert!(text.ends_with("### Extra\n"), "{text}");
    }

    #[test]
    fn test_extra_section_that_is_too_long_is_dropped() {
        let extra = Extra("### Extra\nlong text\n");
        let header_only = CommentOptions {
            max_bytes: 1,
            ..CommentOptions::default()
        };
        let options = CommentOptions {
            max_bytes: render(&sample(), &header_only, &[]).len() + 5,
            ..CommentOptions::default()
        };
        let text = render(&sample(), &options, &[&extra]);
        assert!(!text.contains("Extra"), "{text}");
        assert!(text.len() <= options.max_bytes);
    }

    #[test]
    fn test_comment_of_an_empty_change_set_gives_the_summary_only() {
        let mut changes = sample();
        changes.symbols.clear();
        changes.files.clear();
        changes.new_findings.clear();
        changes.graph = ChangeGraph::default();
        let text = default_render(&changes);
        assert!(text.contains("| Files changed |"));
        assert!(!text.contains("<details>"));
        assert!(!text.contains("mermaid"));
    }

    #[test]
    fn test_cell_text_cannot_break_a_table_row() {
        let mut changes = sample();
        changes.symbols[0].name = "a|b".to_string();
        changes.symbols[0].key = "src/a.rs#a|b".to_string();
        let text = default_render(&changes);
        assert!(text.contains("`a\\|b`"), "{text}");
    }
}
