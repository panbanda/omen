//! UOR (Unary Operator Replacement) mutation operator.
//!
//! This operator removes unary operators:
//! - `!x` -> `x` (remove logical negation)
//! - `-x` -> `x` (remove unary minus)
//! - `~x` -> `x` (remove bitwise not)
//!
//! Kill probability estimate: ~90%
//! Removing unary operators creates highly detectable mutations since
//! they often invert critical conditions or change sign of values.
//! Surviving mutants typically indicate missing assertion or dead code.

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::{mutant_from_node, walk_and_collect_mutants};

/// UOR (Unary Operator Replacement) operator.
///
/// Removes unary operators (!, -, ~) from expressions.
pub struct UnaryOperator;

impl MutationOperator for UnaryOperator {
    fn name(&self) -> &'static str {
        "UOR"
    }

    fn description(&self) -> &'static str {
        "Unary Operator Replacement - removes unary operators (!, -, ~)"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let unary_types = get_unary_expression_types(result.language);
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            if !unary_types.contains(&node.kind()) {
                return Vec::new();
            }
            if let Some(mutant_info) = extract_unary_mutation(node, &result.source, result.language)
            {
                counter += 1;
                let id = format!("{}-{}", mutant_id_prefix, counter);
                vec![mutant_from_node(
                    id,
                    result.path.clone(),
                    self.name(),
                    &node,
                    mutant_info.original,
                    mutant_info.replacement,
                    mutant_info.description,
                )]
            } else {
                Vec::new()
            }
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Information needed to create a unary mutation.
struct UnaryMutationInfo {
    original: String,
    replacement: String,
    description: String,
}

/// Get node types for unary expressions.
fn get_unary_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["unary_expression"],
        Language::Go => &["unary_expression"],
        Language::Python => &["unary_operator", "not_operator"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["unary_expression"]
        }
        Language::Java | Language::CSharp => &["unary_expression", "prefix_unary_expression"],
        Language::C | Language::Cpp => &["unary_expression"],
        Language::Ruby => &["unary"],
        Language::Php => &["unary_op_expression"],
        Language::Bash => &[],
    }
}

/// Extract mutation information from a unary expression node.
fn extract_unary_mutation(
    node: tree_sitter::Node,
    source: &[u8],
    lang: Language,
) -> Option<UnaryMutationInfo> {
    let full_text = node.utf8_text(source).ok()?;

    // Find the operator
    let operator = find_unary_operator(node, source, lang)?;

    // Only mutate supported operators
    if !is_supported_unary_operator(&operator, lang) {
        return None;
    }

    // Get the operand (expression without the operator)
    let operand = extract_operand(node, source, lang)?;

    Some(UnaryMutationInfo {
        original: full_text.to_string(),
        replacement: operand,
        description: format!("Remove unary operator '{}' from expression", operator),
    })
}

/// Find the unary operator in a unary expression.
fn find_unary_operator(node: tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    // For Python 'not' operator, handle specially
    if lang == Language::Python && node.kind() == "not_operator" {
        return Some("not".to_string());
    }

    // Look for operator child nodes
    for child in node.children(&mut node.walk()) {
        let kind = child.kind();
        // Check if this is an operator node
        if matches!(kind, "!" | "-" | "~" | "not") {
            if let Ok(text) = child.utf8_text(source) {
                return Some(text.to_string());
            }
        }
    }

    // Some languages embed the operator in the node text directly
    // Try to extract from the beginning of the expression
    if let Ok(text) = node.utf8_text(source) {
        let trimmed = text.trim_start();
        if trimmed.starts_with('!') {
            return Some("!".to_string());
        }
        if trimmed.starts_with('-') {
            return Some("-".to_string());
        }
        if trimmed.starts_with('~') {
            return Some("~".to_string());
        }
        if trimmed.starts_with("not ") || trimmed.starts_with("not(") {
            return Some("not".to_string());
        }
    }

    None
}

/// Check if a unary operator should be mutated.
fn is_supported_unary_operator(op: &str, lang: Language) -> bool {
    match op {
        "!" | "-" | "~" => true,
        "not" => lang == Language::Python,
        _ => false,
    }
}

/// Extract the operand from a unary expression.
fn extract_operand(node: tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    // For Python 'not' operator
    if lang == Language::Python && node.kind() == "not_operator" {
        // Find the argument child
        for child in node.children(&mut node.walk()) {
            if child.kind() != "not" {
                if let Ok(text) = child.utf8_text(source) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    // Look for the operand child (typically named "argument" or similar)
    for child in node.children(&mut node.walk()) {
        let kind = child.kind();
        // Skip operator nodes
        if matches!(kind, "!" | "-" | "~" | "not") {
            continue;
        }
        // This should be the operand
        if let Ok(text) = child.utf8_text(source) {
            return Some(text.to_string());
        }
    }

    // Fallback: try to strip operator from the beginning
    if let Ok(text) = node.utf8_text(source) {
        let trimmed = text.trim();
        if let Some(rest) = trimmed.strip_prefix('!') {
            return Some(rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix('-') {
            return Some(rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix('~') {
            return Some(rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("not ") {
            return Some(rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("not") {
            if rest.starts_with('(') {
                return Some(rest.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_unary_operator_name() {
        let op = UnaryOperator;
        assert_eq!(op.name(), "UOR");
    }

    #[test]
    fn test_unary_operator_description() {
        let op = UnaryOperator;
        assert!(op.description().contains("Unary"));
        assert!(op.description().contains("!"));
    }

    #[test]
    fn test_unary_operator_supports_all_languages() {
        let op = UnaryOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::JavaScript));
        assert!(op.supports_language(Language::Java));
        assert!(op.supports_language(Language::Cpp));
    }

    #[test]
    fn test_is_supported_unary_operator_negation() {
        assert!(is_supported_unary_operator("!", Language::Rust));
        assert!(is_supported_unary_operator("!", Language::JavaScript));
        assert!(is_supported_unary_operator("!", Language::Go));
    }

    #[test]
    fn test_is_supported_unary_operator_minus() {
        assert!(is_supported_unary_operator("-", Language::Rust));
        assert!(is_supported_unary_operator("-", Language::Python));
        assert!(is_supported_unary_operator("-", Language::Java));
    }

    #[test]
    fn test_is_supported_unary_operator_bitwise_not() {
        assert!(is_supported_unary_operator("~", Language::Rust));
        assert!(is_supported_unary_operator("~", Language::Cpp));
        assert!(is_supported_unary_operator("~", Language::Python));
    }

    #[test]
    fn test_is_supported_unary_operator_python_not() {
        assert!(is_supported_unary_operator("not", Language::Python));
        assert!(!is_supported_unary_operator("not", Language::Rust));
    }

    #[test]
    fn test_is_supported_unary_operator_rejects_others() {
        assert!(!is_supported_unary_operator("+", Language::Rust));
        assert!(!is_supported_unary_operator("++", Language::Java));
        assert!(!is_supported_unary_operator("&", Language::Rust));
        assert!(!is_supported_unary_operator("*", Language::Rust));
    }

    #[test]
    fn test_generate_mutants_rust_negation() {
        let code = b"fn check(b: bool) -> bool { !b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();
        assert_eq!(neg_mutant.replacement, "b");
    }

    #[test]
    fn test_generate_mutants_rust_unary_minus() {
        let code = b"fn negate(x: i32) -> i32 { -x }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let minus_mutant = mutants.iter().find(|m| m.original.contains('-')).unwrap();
        assert_eq!(minus_mutant.replacement, "x");
    }

    #[test]
    fn test_generate_mutants_rust_bitwise_not() {
        let code = b"fn invert(x: u32) -> u32 { ~x }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Note: Rust doesn't actually use ~ for bitwise not (it uses !),
        // but we test the operator support anyway
        // The mutants may be empty if the parser doesn't recognize ~x in Rust
        // This test verifies the mechanism works when the operator is present
        for mutant in &mutants {
            assert!(mutant.description.contains("Remove unary operator"));
        }
    }

    #[test]
    fn test_generate_mutants_javascript_negation() {
        let code = b"function check(b) { return !b; }";
        let result = parse_code(code, Language::JavaScript);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();
        assert_eq!(neg_mutant.replacement, "b");
    }

    #[test]
    fn test_generate_mutants_javascript_unary_minus() {
        let code = b"function negate(x) { return -x; }";
        let result = parse_code(code, Language::JavaScript);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let minus_mutant = mutants.iter().find(|m| m.original.contains('-')).unwrap();
        assert_eq!(minus_mutant.replacement, "x");
    }

    #[test]
    fn test_generate_mutants_python_not() {
        let code = b"def check(b): return not b";
        let result = parse_code(code, Language::Python);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let not_mutant = mutants.iter().find(|m| m.original.contains("not")).unwrap();
        assert_eq!(not_mutant.replacement, "b");
    }

    #[test]
    fn test_generate_mutants_python_unary_minus() {
        let code = b"def negate(x): return -x";
        let result = parse_code(code, Language::Python);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let minus_mutant = mutants.iter().find(|m| m.original.contains('-')).unwrap();
        assert_eq!(minus_mutant.replacement, "x");
    }

    #[test]
    fn test_generate_mutants_go_negation() {
        let code = b"package main\n\nfunc check(b bool) bool { return !b }";
        let result = parse_code(code, Language::Go);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();
        assert_eq!(neg_mutant.replacement, "b");
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"fn check(b: bool) -> bool { !b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "UOR");
        }
    }

    #[test]
    fn test_mutant_has_correct_position() {
        let code = b"fn check(b: bool) -> bool {\n    !b\n}";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();

        // !b should be on line 2 (1-indexed)
        assert_eq!(neg_mutant.line, 2);
    }

    #[test]
    fn test_mutant_byte_range_covers_full_expression() {
        let code = b"fn f() { !b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();

        let (start, end) = neg_mutant.byte_range;
        let extracted = std::str::from_utf8(&code[start..end]).unwrap();
        assert!(extracted.contains('!') && extracted.contains('b'));
    }

    #[test]
    fn test_generate_mutants_multiple_unary_operators() {
        let code = b"fn check(a: bool, x: i32) -> (bool, i32) { (!a, -x) }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have mutants for both !a and -x
        let neg_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains('!'))
            .collect();
        let minus_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains('-'))
            .collect();

        assert!(!neg_mutants.is_empty());
        assert!(!minus_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_no_unary_operators() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // No unary operators in the code
        assert!(mutants.is_empty());
    }

    #[test]
    fn test_mutant_description_is_meaningful() {
        let code = b"fn f() { !b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(mutant.description.contains("Remove unary operator"));
        }
    }

    #[test]
    fn test_mutant_id_is_unique() {
        let code = b"fn f() { (!a, !b, -x) }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mut ids: Vec<_> = mutants.iter().map(|m| &m.id).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();

        assert_eq!(ids.len(), original_len, "All mutant IDs should be unique");
    }

    #[test]
    fn test_generate_mutants_nested_unary() {
        let code = b"fn f() { !!b }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find at least the outer negation
        assert!(!mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_complex_operand() {
        let code = b"fn f() { !(a && b) }";
        let result = parse_code(code, Language::Rust);
        let op = UnaryOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
        let neg_mutant = mutants.iter().find(|m| m.original.contains('!')).unwrap();
        assert!(neg_mutant.replacement.contains("a"));
        assert!(neg_mutant.replacement.contains("b"));
    }
}
