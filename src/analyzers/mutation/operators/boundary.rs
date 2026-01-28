//! BVO (Boundary Value Operator) mutation operator.
//!
//! This operator mutates boundary conditions to test off-by-one errors:
//! - `x < n` -> `x < n-1`, `x < n+1`
//! - `x <= n` -> `x < n`, `x <= n+1`
//! - Array index `[i]` -> `[i-1]`, `[i+1]`
//!
//! Kill probability estimate: ~70-85%
//! Boundary mutations are highly effective because they target common
//! off-by-one errors that frequently escape code review.

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::walk_and_collect_mutants;

/// BVO (Boundary Value Operator) operator.
pub struct BoundaryOperator;

impl MutationOperator for BoundaryOperator {
    fn name(&self) -> &'static str {
        "BVO"
    }

    fn description(&self) -> &'static str {
        "Boundary Value Operator - mutates boundary conditions and array indices"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            let kind = node.kind();
            let mut mutants = Vec::new();
            if is_comparison_expression(kind, result.language) {
                mutants.extend(generate_comparison_boundary_mutants(
                    &node,
                    result,
                    mutant_id_prefix,
                    &mut counter,
                ));
            }
            if is_subscript_expression(kind, result.language) {
                mutants.extend(generate_index_boundary_mutants(
                    &node,
                    result,
                    mutant_id_prefix,
                    &mut counter,
                ));
            }
            mutants
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Check if node is a comparison expression.
fn is_comparison_expression(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => kind == "binary_expression",
        Language::Go => kind == "binary_expression",
        Language::Python => kind == "comparison_operator",
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            kind == "binary_expression"
        }
        Language::Java | Language::CSharp => kind == "binary_expression",
        Language::C | Language::Cpp => kind == "binary_expression",
        Language::Ruby => kind == "binary",
        Language::Php => kind == "binary_expression",
        Language::Bash => kind == "binary_expression",
    }
}

/// Check if node is a subscript/index expression.
fn is_subscript_expression(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => kind == "index_expression",
        Language::Go => kind == "index_expression",
        Language::Python => kind == "subscript",
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            kind == "subscript_expression"
        }
        Language::Java => kind == "array_access",
        Language::CSharp => kind == "element_access_expression",
        Language::C | Language::Cpp => kind == "subscript_expression",
        Language::Ruby => kind == "element_reference",
        Language::Php => kind == "subscript_expression",
        Language::Bash => kind == "subscript",
    }
}

/// Check if a comparison operator is a boundary operator (< or <=).
fn is_boundary_comparison(kind: &str) -> bool {
    matches!(kind, "<" | "<=" | ">" | ">=")
}

/// Generate boundary mutants for comparison expressions.
fn generate_comparison_boundary_mutants(
    node: &tree_sitter::Node,
    result: &ParseResult,
    prefix: &str,
    counter: &mut usize,
) -> Vec<Mutant> {
    let mut mutants = Vec::new();

    // Find operator and right operand
    let mut operator_node = None;
    let mut right_operand = None;

    for child in node.children(&mut node.walk()) {
        if is_boundary_comparison(child.kind()) {
            operator_node = Some(child);
        } else if operator_node.is_some() && right_operand.is_none() {
            // First child after operator is the right operand
            right_operand = Some(child);
        }
    }

    // Check if we have a boundary comparison with a numeric literal on the right
    if let (Some(op_node), Some(right)) = (operator_node, right_operand) {
        if let Ok(op_text) = op_node.utf8_text(&result.source) {
            // Check if right operand is a number literal
            if is_numeric_literal(right.kind(), result.language) {
                if let Ok(num_text) = right.utf8_text(&result.source) {
                    if let Ok(num_val) = parse_integer(num_text) {
                        // Generate n-1 and n+1 mutations
                        let replacements = get_boundary_numeric_replacements(num_val);
                        for replacement in replacements {
                            *counter += 1;
                            let id = format!("{}-{}", prefix, counter);
                            let start = right.start_position();
                            mutants.push(Mutant::new(
                                id,
                                result.path.clone(),
                                "BVO",
                                (start.row + 1) as u32,
                                (start.column + 1) as u32,
                                num_text,
                                replacement.clone(),
                                format!("Boundary: change {} to {}", num_text, replacement),
                                (right.start_byte(), right.end_byte()),
                            ));
                        }
                    }

                    // For <= operators, also generate mutation to <
                    if op_text == "<=" {
                        *counter += 1;
                        let id = format!("{}-{}", prefix, counter);
                        let start = op_node.start_position();
                        mutants.push(Mutant::new(
                            id,
                            result.path.clone(),
                            "BVO",
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            op_text,
                            "<",
                            format!("Boundary: change {} to <", op_text),
                            (op_node.start_byte(), op_node.end_byte()),
                        ));
                    } else if op_text == ">=" {
                        *counter += 1;
                        let id = format!("{}-{}", prefix, counter);
                        let start = op_node.start_position();
                        mutants.push(Mutant::new(
                            id,
                            result.path.clone(),
                            "BVO",
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            op_text,
                            ">",
                            format!("Boundary: change {} to >", op_text),
                            (op_node.start_byte(), op_node.end_byte()),
                        ));
                    }
                }
            }
        }
    }

    mutants
}

/// Generate boundary mutants for array index expressions.
fn generate_index_boundary_mutants(
    node: &tree_sitter::Node,
    result: &ParseResult,
    prefix: &str,
    counter: &mut usize,
) -> Vec<Mutant> {
    let mut mutants = Vec::new();

    // Find the index expression (usually the second child or inside brackets)
    for child in node.children(&mut node.walk()) {
        // Look for numeric literals or identifiers used as indices
        if is_numeric_literal(child.kind(), result.language) {
            if let Ok(num_text) = child.utf8_text(&result.source) {
                if let Ok(num_val) = parse_integer(num_text) {
                    let replacements = get_boundary_index_replacements(num_val);
                    for replacement in replacements {
                        *counter += 1;
                        let id = format!("{}-{}", prefix, counter);
                        let start = child.start_position();
                        mutants.push(Mutant::new(
                            id,
                            result.path.clone(),
                            "BVO",
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            num_text,
                            replacement.clone(),
                            format!("Boundary: change index {} to {}", num_text, replacement),
                            (child.start_byte(), child.end_byte()),
                        ));
                    }
                }
            }
        } else if child.kind() == "identifier" || is_identifier_like(child.kind(), result.language)
        {
            if let Ok(id_text) = child.utf8_text(&result.source) {
                // Generate i-1 and i+1 mutations for identifier indices
                let replacements = get_boundary_identifier_replacements(id_text);
                for replacement in replacements {
                    *counter += 1;
                    let id = format!("{}-{}", prefix, counter);
                    let start = child.start_position();
                    mutants.push(Mutant::new(
                        id,
                        result.path.clone(),
                        "BVO",
                        (start.row + 1) as u32,
                        (start.column + 1) as u32,
                        id_text,
                        replacement.clone(),
                        format!("Boundary: change index {} to {}", id_text, replacement),
                        (child.start_byte(), child.end_byte()),
                    ));
                }
            }
        }
    }

    mutants
}

/// Check if a node kind represents a numeric literal.
fn is_numeric_literal(kind: &str, _lang: Language) -> bool {
    matches!(
        kind,
        "integer_literal"
            | "float_literal"
            | "number"
            | "integer"
            | "int_literal"
            | "decimal_integer_literal"
            | "NUMBER"
    )
}

/// Check if a node kind is identifier-like.
fn is_identifier_like(kind: &str, _lang: Language) -> bool {
    matches!(kind, "identifier" | "name" | "IDENTIFIER" | "NAME")
}

/// Parse an integer from a string, handling various formats.
fn parse_integer(s: &str) -> Result<i64, std::num::ParseIntError> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        i64::from_str_radix(&s[2..], 16)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        i64::from_str_radix(&s[2..], 8)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        i64::from_str_radix(&s[2..], 2)
    } else {
        // Remove underscores (Rust number separators)
        let cleaned: String = s.chars().filter(|&c| c != '_').collect();
        cleaned.parse()
    }
}

/// Get boundary replacements for a numeric value (n-1, n+1).
fn get_boundary_numeric_replacements(n: i64) -> Vec<String> {
    vec![(n - 1).to_string(), (n + 1).to_string()]
}

/// Get boundary replacements for an array index value.
fn get_boundary_index_replacements(n: i64) -> Vec<String> {
    let mut replacements = vec![];
    if n > 0 {
        replacements.push((n - 1).to_string());
    }
    replacements.push((n + 1).to_string());
    replacements
}

/// Get boundary replacements for an identifier index (i -> i-1, i+1).
fn get_boundary_identifier_replacements(id: &str) -> Vec<String> {
    vec![format!("{} - 1", id), format!("{} + 1", id)]
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
    fn test_boundary_operator_name() {
        let op = BoundaryOperator;
        assert_eq!(op.name(), "BVO");
    }

    #[test]
    fn test_boundary_operator_description() {
        let op = BoundaryOperator;
        assert!(op.description().contains("Boundary"));
    }

    #[test]
    fn test_boundary_operator_supports_all_languages() {
        let op = BoundaryOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::TypeScript));
    }

    #[test]
    fn test_parse_integer_decimal() {
        assert_eq!(parse_integer("42").unwrap(), 42);
        assert_eq!(parse_integer("0").unwrap(), 0);
        assert_eq!(parse_integer("-1").unwrap(), -1);
    }

    #[test]
    fn test_parse_integer_hex() {
        assert_eq!(parse_integer("0x10").unwrap(), 16);
        assert_eq!(parse_integer("0xFF").unwrap(), 255);
    }

    #[test]
    fn test_parse_integer_with_underscores() {
        assert_eq!(parse_integer("1_000").unwrap(), 1000);
        assert_eq!(parse_integer("1_000_000").unwrap(), 1000000);
    }

    #[test]
    fn test_get_boundary_numeric_replacements() {
        let replacements = get_boundary_numeric_replacements(10);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"9".to_string()));
        assert!(replacements.contains(&"11".to_string()));
    }

    #[test]
    fn test_get_boundary_numeric_replacements_zero() {
        let replacements = get_boundary_numeric_replacements(0);
        assert!(replacements.contains(&"-1".to_string()));
        assert!(replacements.contains(&"1".to_string()));
    }

    #[test]
    fn test_get_boundary_index_replacements() {
        let replacements = get_boundary_index_replacements(5);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"4".to_string()));
        assert!(replacements.contains(&"6".to_string()));
    }

    #[test]
    fn test_get_boundary_index_replacements_zero() {
        let replacements = get_boundary_index_replacements(0);
        // Should not include -1 for array index at 0
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"1".to_string()));
    }

    #[test]
    fn test_get_boundary_identifier_replacements() {
        let replacements = get_boundary_identifier_replacements("i");
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"i - 1".to_string()));
        assert!(replacements.contains(&"i + 1".to_string()));
    }

    #[test]
    fn test_is_boundary_comparison() {
        assert!(is_boundary_comparison("<"));
        assert!(is_boundary_comparison("<="));
        assert!(is_boundary_comparison(">"));
        assert!(is_boundary_comparison(">="));
        assert!(!is_boundary_comparison("=="));
        assert!(!is_boundary_comparison("!="));
    }

    #[test]
    fn test_generate_mutants_rust_less_than() {
        let code = b"fn check(x: i32) -> bool { x < 10 }";
        let result = parse_code(code, Language::Rust);
        let op = BoundaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find boundary mutations for the literal 10
        let boundary_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "10" || m.replacement == "9" || m.replacement == "11")
            .collect();
        assert!(
            !boundary_mutants.is_empty(),
            "Expected boundary mutants for numeric literal"
        );
    }

    #[test]
    fn test_generate_mutants_rust_less_than_or_equal() {
        let code = b"fn check(x: i32) -> bool { x <= 10 }";
        let result = parse_code(code, Language::Rust);
        let op = BoundaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutation from <= to <
        let op_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "<=" && m.replacement == "<")
            .collect();
        assert!(
            !op_mutants.is_empty(),
            "Expected <= to < mutation for boundary"
        );
    }

    #[test]
    fn test_generate_mutants_rust_array_index() {
        let code = b"fn get(arr: &[i32]) -> i32 { arr[5] }";
        let result = parse_code(code, Language::Rust);
        let op = BoundaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find boundary mutations for array index 5
        let index_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.replacement == "4" || m.replacement == "6")
            .collect();
        assert!(
            !index_mutants.is_empty(),
            "Expected boundary mutants for array index"
        );
    }

    #[test]
    fn test_is_comparison_expression_rust() {
        assert!(is_comparison_expression(
            "binary_expression",
            Language::Rust
        ));
        assert!(!is_comparison_expression("identifier", Language::Rust));
    }

    #[test]
    fn test_is_subscript_expression_languages() {
        assert!(is_subscript_expression("index_expression", Language::Rust));
        assert!(is_subscript_expression("index_expression", Language::Go));
        assert!(is_subscript_expression("subscript", Language::Python));
        assert!(is_subscript_expression(
            "subscript_expression",
            Language::JavaScript
        ));
        assert!(is_subscript_expression("array_access", Language::Java));
    }

    #[test]
    fn test_empty_code_produces_no_mutants() {
        let code = b"fn empty() {}";
        let result = parse_code(code, Language::Rust);
        let op = BoundaryOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.is_empty());
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"fn check(x: i32) -> bool { x < 10 }";
        let result = parse_code(code, Language::Rust);
        let op = BoundaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "BVO");
        }
    }
}
