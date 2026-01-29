//! AOR (Arithmetic Operator Replacement) mutation operator.
//!
//! This operator replaces arithmetic operators:
//! - + -> -, *, /, %
//! - - -> +, *, /, %
//! - * -> +, -, /, %
//! - / -> +, -, *, %
//! - % -> +, -, *, /

use crate::core::Language;
use crate::parser::queries::get_binary_expression_types;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::generate_binary_operator_mutants;

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
        let node_types = get_binary_expression_types(result.language);
        generate_binary_operator_mutants(
            result,
            mutant_id_prefix,
            self.name(),
            node_types,
            is_arithmetic_operator,
            get_arithmetic_replacements,
        )
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Check if a node kind is an arithmetic operator.
fn is_arithmetic_operator(kind: &str) -> bool {
    matches!(kind, "+" | "-" | "*" | "/" | "%")
}

/// Get replacement operators for an arithmetic operator.
fn get_arithmetic_replacements(op: &str) -> Vec<String> {
    super::replacements_excluding_self(&["+", "-", "*", "/", "%"], op)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
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
