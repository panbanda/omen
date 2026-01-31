//! ASR (Assignment Operator Replacement) mutation operator.
//!
//! This operator replaces compound assignment operators:
//! - `+=` -> `-=`
//! - `-=` -> `+=`
//! - `*=` -> `/=`
//! - `/=` -> `*=`
//! - `&=` -> `|=`
//! - `|=` -> `&=`
//!
//! Kill probability estimate: ~75-90%
//! Assignment mutations are highly effective because they directly affect
//! state changes. A well-tested program should catch most of these mutations.

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::{mutant_from_node, walk_and_collect_mutants};

/// ASR (Assignment Operator Replacement) operator.
pub struct AssignmentOperator;

impl MutationOperator for AssignmentOperator {
    fn name(&self) -> &'static str {
        "ASR"
    }

    fn description(&self) -> &'static str {
        "Assignment Operator Replacement - replaces compound assignment operators"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let assignment_types = get_assignment_expression_types(result.language);
        let lang = result.language;
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            let kind = node.kind();
            let mut mutants = Vec::new();

            if assignment_types.contains(&kind) {
                for child in node.children(&mut node.walk()) {
                    if is_compound_assignment_operator(child.kind()) {
                        if let Ok(op_text) = child.utf8_text(&result.source) {
                            let replacements = get_assignment_replacements(op_text);
                            for replacement in replacements {
                                counter += 1;
                                let id = format!("{}-{}", mutant_id_prefix, counter);
                                mutants.push(mutant_from_node(
                                    id,
                                    result.path.clone(),
                                    self.name(),
                                    &child,
                                    op_text,
                                    replacement.clone(),
                                    format!("Replace {} with {}", op_text, replacement),
                                ));
                            }
                        }
                    }
                }
            }

            else if is_compound_assignment_statement(kind, lang) {
                if let Some(replacements) = get_replacement_for_node_kind(kind) {
                    if let Ok(node_text) = node.utf8_text(&result.source) {
                        for (op, replacement) in replacements {
                            if node_text.contains(op) {
                                counter += 1;
                                let id = format!("{}-{}", mutant_id_prefix, counter);
                                let start = node.start_position();
                                mutants.push(Mutant::new(
                                    id,
                                    result.path.clone(),
                                    self.name(),
                                    (start.row + 1) as u32,
                                    (start.column + 1) as u32,
                                    op,
                                    replacement,
                                    format!("Replace {} with {}", op, replacement),
                                    find_operator_byte_range(&result.source, node.start_byte(), op),
                                ));
                            }
                        }
                    }
                }
            }

            mutants
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Get node types for assignment expressions.
fn get_assignment_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["compound_assignment_expr", "assignment_expression"],
        Language::Go => &["assignment_statement"],
        Language::Python => &["augmented_assignment"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["augmented_assignment_expression", "assignment_expression"]
        }
        Language::Java | Language::CSharp => &["assignment_expression"],
        Language::C | Language::Cpp => &["assignment_expression"],
        Language::Ruby => &["operator_assignment"],
        Language::Php => &["augmented_assignment_expression"],
        Language::Bash => &["assignment"],
    }
}

/// Check if a node kind is a compound assignment statement.
fn is_compound_assignment_statement(kind: &str, _lang: Language) -> bool {
    matches!(
        kind,
        "compound_assignment_expr"
            | "augmented_assignment"
            | "augmented_assignment_expression"
            | "operator_assignment"
    )
}

/// Check if a node kind is a compound assignment operator.
fn is_compound_assignment_operator(kind: &str) -> bool {
    matches!(
        kind,
        "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
    )
}

/// Get replacement operators for a compound assignment operator.
fn get_assignment_replacements(op: &str) -> Vec<String> {
    match op {
        "+=" => vec!["-=".to_string()],
        "-=" => vec!["+=".to_string()],
        "*=" => vec!["/=".to_string()],
        "/=" => vec!["*=".to_string()],
        "&=" => vec!["|=".to_string()],
        "|=" => vec!["&=".to_string()],
        "^=" => vec!["&=".to_string()],
        "<<=" => vec![">>=".to_string()],
        ">>=" => vec!["<<=".to_string()],
        "%=" => vec!["*=".to_string()],
        _ => vec![],
    }
}

/// Get replacement pairs for compound assignment node kinds.
fn get_replacement_for_node_kind(kind: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match kind {
        "compound_assignment_expr" | "augmented_assignment" | "augmented_assignment_expression" => {
            Some(vec![
                ("+=", "-="),
                ("-=", "+="),
                ("*=", "/="),
                ("/=", "*="),
                ("&=", "|="),
                ("|=", "&="),
            ])
        }
        _ => None,
    }
}

/// Find the byte range of an operator within a source slice.
fn find_operator_byte_range(source: &[u8], start: usize, op: &str) -> (usize, usize) {
    let op_bytes = op.as_bytes();
    if let Some(pos) = source[start..]
        .windows(op_bytes.len())
        .position(|w| w == op_bytes)
    {
        (start + pos, start + pos + op_bytes.len())
    } else {
        (start, start + op.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_assignment_operator_name() {
        let op = AssignmentOperator;
        assert_eq!(op.name(), "ASR");
    }

    #[test]
    fn test_assignment_operator_description() {
        let op = AssignmentOperator;
        assert!(op.description().contains("Assignment"));
    }

    #[test]
    fn test_assignment_operator_supports_all_languages() {
        let op = AssignmentOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::TypeScript));
        assert!(op.supports_language(Language::Java));
    }

    #[test]
    fn test_get_assignment_replacements_plus_equals() {
        let replacements = get_assignment_replacements("+=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"-=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_minus_equals() {
        let replacements = get_assignment_replacements("-=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"+=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_multiply_equals() {
        let replacements = get_assignment_replacements("*=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"/=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_divide_equals() {
        let replacements = get_assignment_replacements("/=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"*=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_and_equals() {
        let replacements = get_assignment_replacements("&=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"|=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_or_equals() {
        let replacements = get_assignment_replacements("|=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"&=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_shift_left_equals() {
        let replacements = get_assignment_replacements("<<=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&">>=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_shift_right_equals() {
        let replacements = get_assignment_replacements(">>=");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"<<=".to_string()));
    }

    #[test]
    fn test_get_assignment_replacements_unknown() {
        let replacements = get_assignment_replacements("=");
        assert!(replacements.is_empty());
    }

    #[test]
    fn test_is_compound_assignment_operator() {
        assert!(is_compound_assignment_operator("+="));
        assert!(is_compound_assignment_operator("-="));
        assert!(is_compound_assignment_operator("*="));
        assert!(is_compound_assignment_operator("/="));
        assert!(is_compound_assignment_operator("&="));
        assert!(is_compound_assignment_operator("|="));
        assert!(is_compound_assignment_operator("<<="));
        assert!(is_compound_assignment_operator(">>="));
        assert!(!is_compound_assignment_operator("="));
        assert!(!is_compound_assignment_operator("+"));
    }

    #[test]
    fn test_generate_mutants_rust_plus_equals() {
        let code = b"fn add(x: &mut i32) { *x += 1; }";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");

        let plus_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "+=").collect();
        assert!(!plus_mutants.is_empty(), "Expected mutants for += operator");

        // Check that += is replaced with -=
        assert!(plus_mutants.iter().any(|m| m.replacement == "-="));
    }

    #[test]
    fn test_generate_mutants_rust_multiply_equals() {
        let code = b"fn mul(x: &mut i32) { *x *= 2; }";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mul_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "*=").collect();
        assert!(!mul_mutants.is_empty(), "Expected mutants for *= operator");

        // Check that *= is replaced with /=
        assert!(mul_mutants.iter().any(|m| m.replacement == "/="));
    }

    #[test]
    fn test_empty_code_produces_no_mutants() {
        let code = b"fn empty() {}";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.is_empty());
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"fn add(x: &mut i32) { *x += 1; }";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "ASR");
        }
    }

    #[test]
    fn test_mutant_has_valid_byte_range() {
        let code = b"fn add(x: &mut i32) { *x += 1; }";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_multiple_assignment_operators() {
        let code = b"fn compute(x: &mut i32, y: &mut i32) { *x += 1; *y *= 2; }";
        let result = parse_code(code, Language::Rust);
        let op = AssignmentOperator;

        let mutants = op.generate_mutants(&result, "test");

        let plus_count = mutants.iter().filter(|m| m.original == "+=").count();
        let mul_count = mutants.iter().filter(|m| m.original == "*=").count();

        assert!(plus_count > 0, "Expected at least one += mutation");
        assert!(mul_count > 0, "Expected at least one *= mutation");
    }

    #[test]
    fn test_find_operator_byte_range() {
        let source = b"x += 1";
        let range = find_operator_byte_range(source, 0, "+=");
        assert_eq!(range, (2, 4));
    }

    #[test]
    fn test_get_assignment_expression_types() {
        let rust_types = get_assignment_expression_types(Language::Rust);
        assert!(rust_types.contains(&"compound_assignment_expr"));

        let python_types = get_assignment_expression_types(Language::Python);
        assert!(python_types.contains(&"augmented_assignment"));
    }
}
