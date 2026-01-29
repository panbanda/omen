//! BOR (Bitwise Operator Replacement) mutation operator.
//!
//! This operator replaces bitwise operators:
//! - `&` -> `|`
//! - `|` -> `&`
//! - `^` -> `&`
//! - `<<` -> `>>`
//! - `>>` -> `<<`
//!
//! Kill probability estimate: ~60-75%
//! Bitwise mutations are moderately effective. Code that uses bitwise
//! operations often has specific bit-level tests, but the mutations
//! can sometimes produce equivalent programs for certain inputs.

use crate::core::Language;
use crate::parser::queries::get_binary_expression_types;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::generate_binary_operator_mutants;

/// BOR (Bitwise Operator Replacement) operator.
pub struct BitwiseOperator;

impl MutationOperator for BitwiseOperator {
    fn name(&self) -> &'static str {
        "BOR"
    }

    fn description(&self) -> &'static str {
        "Bitwise Operator Replacement - replaces bitwise operators"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let node_types = get_binary_expression_types(result.language);
        generate_binary_operator_mutants(
            result,
            mutant_id_prefix,
            self.name(),
            node_types,
            is_bitwise_operator,
            get_bitwise_replacements,
        )
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Check if a node kind is a bitwise operator.
fn is_bitwise_operator(kind: &str) -> bool {
    matches!(kind, "&" | "|" | "^" | "<<" | ">>" | "~")
}

/// Get replacement operators for a bitwise operator.
fn get_bitwise_replacements(op: &str) -> Vec<String> {
    match op {
        "&" => vec!["|".to_string()],
        "|" => vec!["&".to_string()],
        "^" => vec!["&".to_string()],
        "<<" => vec![">>".to_string()],
        ">>" => vec!["<<".to_string()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_bitwise_operator_name() {
        let op = BitwiseOperator;
        assert_eq!(op.name(), "BOR");
    }

    #[test]
    fn test_bitwise_operator_description() {
        let op = BitwiseOperator;
        assert!(op.description().contains("Bitwise"));
    }

    #[test]
    fn test_bitwise_operator_supports_all_languages() {
        let op = BitwiseOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::TypeScript));
        assert!(op.supports_language(Language::C));
        assert!(op.supports_language(Language::Cpp));
    }

    #[test]
    fn test_get_bitwise_replacements_and() {
        let replacements = get_bitwise_replacements("&");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"|".to_string()));
    }

    #[test]
    fn test_get_bitwise_replacements_or() {
        let replacements = get_bitwise_replacements("|");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"&".to_string()));
    }

    #[test]
    fn test_get_bitwise_replacements_xor() {
        let replacements = get_bitwise_replacements("^");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"&".to_string()));
    }

    #[test]
    fn test_get_bitwise_replacements_left_shift() {
        let replacements = get_bitwise_replacements("<<");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&">>".to_string()));
    }

    #[test]
    fn test_get_bitwise_replacements_right_shift() {
        let replacements = get_bitwise_replacements(">>");
        assert_eq!(replacements.len(), 1);
        assert!(replacements.contains(&"<<".to_string()));
    }

    #[test]
    fn test_get_bitwise_replacements_unknown() {
        let replacements = get_bitwise_replacements("+");
        assert!(replacements.is_empty());
    }

    #[test]
    fn test_is_bitwise_operator() {
        assert!(is_bitwise_operator("&"));
        assert!(is_bitwise_operator("|"));
        assert!(is_bitwise_operator("^"));
        assert!(is_bitwise_operator("<<"));
        assert!(is_bitwise_operator(">>"));
        assert!(!is_bitwise_operator("+"));
        assert!(!is_bitwise_operator("-"));
        assert!(!is_bitwise_operator("<"));
    }

    #[test]
    fn test_generate_mutants_rust_bitwise_and() {
        let code = b"fn mask(a: u32, b: u32) -> u32 { a & b }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        let and_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "&").collect();
        assert!(!and_mutants.is_empty(), "Expected mutants for & operator");

        // Check that & is replaced with |
        assert!(and_mutants.iter().any(|m| m.replacement == "|"));
    }

    #[test]
    fn test_generate_mutants_rust_bitwise_or() {
        let code = b"fn combine(a: u32, b: u32) -> u32 { a | b }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        let or_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "|").collect();
        assert!(!or_mutants.is_empty(), "Expected mutants for | operator");

        // Check that | is replaced with &
        assert!(or_mutants.iter().any(|m| m.replacement == "&"));
    }

    #[test]
    fn test_generate_mutants_rust_left_shift() {
        let code = b"fn shift(x: u32) -> u32 { x << 2 }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        let shift_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "<<").collect();
        assert!(
            !shift_mutants.is_empty(),
            "Expected mutants for << operator"
        );

        // Check that << is replaced with >>
        assert!(shift_mutants.iter().any(|m| m.replacement == ">>"));
    }

    #[test]
    fn test_generate_mutants_rust_right_shift() {
        let code = b"fn shift(x: u32) -> u32 { x >> 2 }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        let shift_mutants: Vec<_> = mutants.iter().filter(|m| m.original == ">>").collect();
        assert!(
            !shift_mutants.is_empty(),
            "Expected mutants for >> operator"
        );

        // Check that >> is replaced with <<
        assert!(shift_mutants.iter().any(|m| m.replacement == "<<"));
    }

    #[test]
    fn test_empty_code_produces_no_mutants() {
        let code = b"fn empty() {}";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.is_empty());
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"fn mask(a: u32, b: u32) -> u32 { a & b }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "BOR");
        }
    }

    #[test]
    fn test_mutant_has_correct_byte_range() {
        let code = b"fn mask(a: u32, b: u32) -> u32 { a & b }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_multiple_bitwise_operators() {
        let code = b"fn complex(a: u32, b: u32, c: u32) -> u32 { (a & b) | c }";
        let result = parse_code(code, Language::Rust);
        let op = BitwiseOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have mutants for both & and |
        let and_count = mutants.iter().filter(|m| m.original == "&").count();
        let or_count = mutants.iter().filter(|m| m.original == "|").count();

        assert!(and_count > 0, "Expected at least one & mutation");
        assert!(or_count > 0, "Expected at least one | mutation");
    }

    #[test]
    fn test_get_binary_expression_types() {
        assert_eq!(
            get_binary_expression_types(Language::Rust),
            &["binary_expression"]
        );
        assert_eq!(
            get_binary_expression_types(Language::Python),
            &["binary_operator"]
        );
        assert_eq!(get_binary_expression_types(Language::Ruby), &["binary"]);
    }
}
