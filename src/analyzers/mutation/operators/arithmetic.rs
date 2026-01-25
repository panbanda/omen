//! AOR (Arithmetic Operator Replacement) mutation operator.
//!
//! This operator replaces arithmetic operators:
//! - + -> -, *, /, %
//! - - -> +, *, /, %
//! - * -> +, -, /, %
//! - / -> +, -, *, %
//! - % -> +, -, *, /

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;

/// AOR (Arithmetic Operator Replacement) operator.
pub struct ArithmeticOperator;

impl MutationOperator for ArithmeticOperator {
    fn name(&self) -> &'static str {
        "AOR"
    }

    fn description(&self) -> &'static str {
        "Arithmetic Operator Replacement - replaces arithmetic operators"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let root = result.root_node();
        let binary_types = get_binary_expression_types(result.language);

        let mut counter = 0;
        let mut cursor = root.walk();

        loop {
            let node = cursor.node();
            let kind = node.kind();

            if binary_types.contains(&kind) {
                // Find the operator child
                for child in node.children(&mut node.walk()) {
                    if is_arithmetic_operator(child.kind()) {
                        if let Ok(op_text) = child.utf8_text(&result.source) {
                            let replacements = get_arithmetic_replacements(op_text);
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

/// Get node types for binary expressions.
fn get_binary_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["binary_expression"],
        Language::Go => &["binary_expression"],
        Language::Python => &["binary_operator"],
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

/// Check if a node kind is an arithmetic operator.
fn is_arithmetic_operator(kind: &str) -> bool {
    matches!(kind, "+" | "-" | "*" | "/" | "%")
}

/// Get replacement operators for an arithmetic operator.
fn get_arithmetic_replacements(op: &str) -> Vec<String> {
    let all_ops = ["+", "-", "*", "/", "%"];

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
    fn test_arithmetic_operator_name() {
        let op = ArithmeticOperator;
        assert_eq!(op.name(), "AOR");
    }

    #[test]
    fn test_get_arithmetic_replacements_plus() {
        let replacements = get_arithmetic_replacements("+");
        assert_eq!(replacements.len(), 4);
        assert!(!replacements.contains(&"+".to_string()));
        assert!(replacements.contains(&"-".to_string()));
        assert!(replacements.contains(&"*".to_string()));
        assert!(replacements.contains(&"/".to_string()));
        assert!(replacements.contains(&"%".to_string()));
    }

    #[test]
    fn test_get_arithmetic_replacements_multiply() {
        let replacements = get_arithmetic_replacements("*");
        assert_eq!(replacements.len(), 4);
        assert!(!replacements.contains(&"*".to_string()));
        assert!(replacements.contains(&"+".to_string()));
    }

    #[test]
    fn test_generate_mutants_rust_arithmetic() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);
        let op = ArithmeticOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutations for +
        let plus_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "+").collect();
        assert!(!plus_mutants.is_empty());
    }

    #[test]
    fn test_is_arithmetic_operator() {
        assert!(is_arithmetic_operator("+"));
        assert!(is_arithmetic_operator("-"));
        assert!(is_arithmetic_operator("*"));
        assert!(is_arithmetic_operator("/"));
        assert!(is_arithmetic_operator("%"));
        assert!(!is_arithmetic_operator("<"));
        assert!(!is_arithmetic_operator("=="));
    }
}
