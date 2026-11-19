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

/// The value of `x = <expression>` in `source`.
fn literal_at(language: crate::core::Language, file: &str, source: &str) -> bool {
    let parsed = crate::parser::Parser::new()
        .parse(source.as_bytes(), language, std::path::Path::new(file))
        .unwrap();
    let mut value = None;
    crate::parser::walk_preorder(parsed.root_node(), |node| {
        if matches!(node.kind(), "assignment" | "variable_declarator") {
            value = node
                .child_by_field_name("right")
                .or_else(|| node.child_by_field_name("value"));
            return crate::parser::Walk::Stop;
        }
        crate::parser::Walk::Children
    });
    super::syntax::is_literal(value.unwrap())
}

/// `is_literal` reads each level of a list of literals.
#[test]
fn a_deep_list_of_literals_is_a_literal() {
    let literal = on_small_stack(|| {
        let source = format!("x = {}\n", nest("[", "1", "]"));
        literal_at(crate::core::Language::Python, "a.py", &source)
    });
    assert!(literal);
}

/// `is_literal` and `outer_interpolations` read each level of a template in a template.
#[test]
fn a_deep_template_of_literals_is_a_literal() {
    let literal = on_small_stack(|| {
        let source = format!("const x = {};\n", nest("`${", "'a'", "}`"));
        literal_at(crate::core::Language::JavaScript, "a.js", &source)
    });
    assert!(literal);
}
