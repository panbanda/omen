//! Suppression of a sink with a comment: `omen:allow <category>: <reason>`.
//!
//! The comment applies to the sinks of the category on its own line, and on the next line.
//! The reason is mandatory. A comment without a reason, or with an unknown category, is not
//! a suppression, and the analysis reports it. The output counts the suppressed sinks.

use serde::{Deserialize, Serialize};

use super::{Sink, SinkCategory};
use crate::parser::ParseResult;

const MARKER: &str = "omen:allow";

/// A sink that a comment suppresses, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suppressed {
    pub sink: Sink,
    pub reason: String,
    /// The line of the comment.
    pub comment_line: u32,
}

/// A comment with the marker that is not a valid suppression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidSuppression {
    pub file: String,
    pub line: u32,
    pub problem: String,
}

struct Allow {
    line: u32,
    category: SinkCategory,
    reason: String,
}

/// The suppression comments of one file, and the problems with the other comments.
fn comments_of(parsed: &ParseResult, file: &str) -> (Vec<Allow>, Vec<InvalidSuppression>) {
    let text = String::from_utf8_lossy(&parsed.source);
    let root = parsed.root_node();
    let mut allowed = Vec::new();
    let mut invalid = Vec::new();
    let mut offset = 0usize;
    for (number, line) in text.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        let Some(position) = line.find(MARKER) else {
            continue;
        };
        let inside_comment = root
            .descendant_for_byte_range(line_start + position, line_start + position + 1)
            .is_some_and(|node| node.kind().contains("comment"));
        if !inside_comment {
            continue;
        }
        let line_number = number as u32 + 1;
        let rest = line[position + MARKER.len()..]
            .trim()
            .trim_end_matches("*/")
            .trim_end_matches("-->")
            .trim();
        let Some((name, reason)) = rest.split_once(':') else {
            invalid.push(InvalidSuppression {
                file: file.to_string(),
                line: line_number,
                problem: "The comment has no category and no reason. Write `omen:allow <category>: <reason>`."
                    .to_string(),
            });
            continue;
        };
        let name = name.trim();
        let Some(category) = SinkCategory::ALL
            .iter()
            .copied()
            .find(|category| category.name() == name)
        else {
            invalid.push(InvalidSuppression {
                file: file.to_string(),
                line: line_number,
                problem: format!("The category `{name}` is not a sink category."),
            });
            continue;
        };
        let reason = reason.trim();
        if reason.is_empty() {
            invalid.push(InvalidSuppression {
                file: file.to_string(),
                line: line_number,
                problem: "The suppression has no reason. A reason is mandatory.".to_string(),
            });
            continue;
        }
        allowed.push(Allow {
            line: line_number,
            category,
            reason: reason.to_string(),
        });
    }
    (allowed, invalid)
}

/// Splits the sinks into the sinks that remain and the sinks that a comment suppresses.
///
/// `parsed[i]` is the tree of the file `files[i]`.
pub fn apply(
    sinks: Vec<Sink>,
    parsed: &[ParseResult],
    files: &[String],
) -> (Vec<Sink>, Vec<Suppressed>, Vec<InvalidSuppression>) {
    let mut allowed: Vec<(String, Allow)> = Vec::new();
    let mut invalid = Vec::new();
    for (tree, file) in parsed.iter().zip(files) {
        let (found, problems) = comments_of(tree, file);
        allowed.extend(found.into_iter().map(|allow| (file.clone(), allow)));
        invalid.extend(problems);
    }
    let mut kept = Vec::new();
    let mut suppressed = Vec::new();
    for sink in sinks {
        let matching = allowed.iter().find(|(file, allow)| {
            *file == sink.file
                && allow.category == sink.category
                && (allow.line == sink.range.start_line || allow.line + 1 == sink.range.start_line)
        });
        match matching {
            Some((_, allow)) => suppressed.push(Suppressed {
                reason: allow.reason.clone(),
                comment_line: allow.line,
                sink,
            }),
            None => kept.push(sink),
        }
    }
    (kept, suppressed, invalid)
}
