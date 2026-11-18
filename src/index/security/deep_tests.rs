//! The security analysis reads source with very deep nesting on a small stack.

use crate::parser::deep::{self, nest, on_small_stack};

use super::analysis::{analyze_sources, Analysis};
use super::SinkCategory;
use crate::index::view::DEFAULT_MIN_BASIS;

fn analyze(file: &'static str, source: String) -> Analysis {
    on_small_stack(move || analyze_sources(&[(file, source.as_str())], DEFAULT_MIN_BASIS))
}

fn categories(analysis: &Analysis) -> Vec<SinkCategory> {
    analysis.sinks.iter().map(|sink| sink.category).collect()
}

/// `tree_sinks` reads each node of each deep source.
#[test]
fn security_reads_each_deep_source() {
    on_small_stack(|| {
        for (source, _) in deep::parsed_deep_sources() {
            analyze_sources(&[(&source.file, &source.source)], DEFAULT_MIN_BASIS);
        }
    });
}

/// `is_built_string` reads each parenthesis around the argument of a sink.
#[test]
fn a_built_query_inside_deep_parentheses() {
    let source = format!(
        "def run(cursor, name):\n    cursor.execute({})\n",
        nest("(", "\"SELECT \" + name", ")")
    );
    let analysis = analyze("app.py", source);
    assert_eq!(categories(&analysis), [SinkCategory::SqlStringConstruction]);
}

/// `has_descendant` reads each level of an operand of a built string.
#[test]
fn a_built_query_with_a_deep_string_operand() {
    let source = format!(
        "def run(cursor, name):\n    cursor.execute(name + {})\n",
        nest("(", "\" WHERE 1\"", ")")
    );
    let analysis = analyze("app.py", source);
    assert_eq!(categories(&analysis), [SinkCategory::SqlStringConstruction]);
}
