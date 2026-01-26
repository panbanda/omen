//! ROR (Relational Operator Replacement) mutation operator.
//!
//! This operator replaces relational operators:
//! - < -> <=, >, >=, ==, !=
//! - <= -> <, >, >=, ==, !=
//! - > -> <, <=, >=, ==, !=
//! - >= -> <, <=, >, ==, !=
//! - == -> <, <=, >, >=, !=
//! - != -> <, <=, >, >=, ==

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;

/// ROR (Relational Operator Replacement) operator.
pub struct RelationalOperator;

impl MutationOperator for RelationalOperator {
    fn name(&self) -> &'static str {
        "ROR"
    }

    fn description(&self) -> &'static str {
        "Relational Operator Replacement - replaces comparison operators"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let root = result.root_node();
        let comparison_types = get_comparison_node_types(result.language);

        let mut counter = 0;
        let mut cursor = root.walk();

        loop {
            let node = cursor.node();
            let kind = node.kind();

            if comparison_types.contains(&kind) {
                // Find the operator child
                for child in node.children(&mut node.walk()) {
                    if is_relational_operator(child.kind(), result.language) {
                        if let Ok(op_text) = child.utf8_text(&result.source) {
                            let replacements =
                                get_relational_replacements(op_text, result.language);
                            for replacement in replacements {
                                counter += 1;
                                let id = format!("{}-{}", mutant_id_prefix, counter);
                                let start = child.start_position();
                                mutants.push(Mutant::new(
                                    id,
                                    result.path.clone(),
                                    self.name(),
                                    (start.row + 1) as u32,
                                    (start.column + 1) as u32,
                                    op_text,
                                    replacement.clone(),
                                    format!("Replace {} with {}", op_text, replacement),
                                    (child.start_byte(), child.end_byte()),
                                ));
                            }
                        }
                    }
                }
            }

            // Tree traversal
            if cursor.goto_first_child() {
                continue;
            }

            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return mutants;
                }
            }
        }
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Get node types for comparison expressions.
fn get_comparison_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["binary_expression"],
        Language::Go => &["binary_expression"],
        Language::Python => &["comparison_operator"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["binary_expression"]
        }
        Language::Java | Language::CSharp => &["binary_expression"],
        Language::C | Language::Cpp => &["binary_expression"],
        Language::Ruby => &["binary"],
        Language::Php => &["binary_expression"],
        Language::Bash => &["binary_expression"],
    }
}

/// Check if a node kind is a relational operator.
fn is_relational_operator(kind: &str, _lang: Language) -> bool {
    matches!(
        kind,
        "<" | ">" | "<=" | ">=" | "==" | "!=" | "eq" | "ne" | "lt" | "le" | "gt" | "ge"
    )
}

/// Get replacement operators for a relational operator.
fn get_relational_replacements(op: &str, _lang: Language) -> Vec<String> {
    let all_ops = ["<", "<=", ">", ">=", "==", "!="];

    all_ops
        .iter()
        .filter(|&&o| o != op)
        .map(|&o| o.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use std::path::Path;

    fn parse_code(code: &[u8], lang: Language) -> ParseResult {
        let parser = Parser::new();
        parser.parse(code, lang, Path::new("test.rs")).unwrap()
    }

    #[test]
    fn test_relational_operator_name() {
        let op = RelationalOperator;
        assert_eq!(op.name(), "ROR");
    }

    #[test]
    fn test_get_relational_replacements_less_than() {
        let replacements = get_relational_replacements("<", Language::Rust);
        assert_eq!(replacements.len(), 5);
        assert!(!replacements.contains(&"<".to_string()));
        assert!(replacements.contains(&"<=".to_string()));
        assert!(replacements.contains(&">".to_string()));
        assert!(replacements.contains(&">=".to_string()));
        assert!(replacements.contains(&"==".to_string()));
        assert!(replacements.contains(&"!=".to_string()));
    }

    #[test]
    fn test_get_relational_replacements_equals() {
        let replacements = get_relational_replacements("==", Language::Rust);
        assert_eq!(replacements.len(), 5);
        assert!(!replacements.contains(&"==".to_string()));
        assert!(replacements.contains(&"<".to_string()));
    }

    #[test]
    fn test_generate_mutants_rust_comparison() {
        let code = b"fn check(x: i32) -> bool { x < 10 }";
        let result = parse_code(code, Language::Rust);
        let op = RelationalOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutations for <
        let lt_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "<").collect();
        assert!(!lt_mutants.is_empty());
    }

    #[test]
    fn test_is_relational_operator() {
        assert!(is_relational_operator("<", Language::Rust));
        assert!(is_relational_operator("<=", Language::Rust));
        assert!(is_relational_operator(">", Language::Rust));
        assert!(is_relational_operator(">=", Language::Rust));
        assert!(is_relational_operator("==", Language::Rust));
        assert!(is_relational_operator("!=", Language::Rust));
        assert!(!is_relational_operator("+", Language::Rust));
    }
}
