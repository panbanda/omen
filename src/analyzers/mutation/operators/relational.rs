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
use crate::parser::queries::get_comparison_expression_types;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::generate_binary_operator_mutants;

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
        let node_types = get_comparison_expression_types(result.language);
        generate_binary_operator_mutants(
            result,
            mutant_id_prefix,
            self.name(),
            node_types,
            is_relational_operator,
            get_relational_replacements,
        )
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Check if a node kind is a relational operator.
fn is_relational_operator(kind: &str) -> bool {
    matches!(
        kind,
        "<" | ">" | "<=" | ">=" | "==" | "!=" | "eq" | "ne" | "lt" | "le" | "gt" | "ge"
    )
}

/// Get replacement operators for a relational operator.
fn get_relational_replacements(op: &str) -> Vec<String> {
    super::replacements_excluding_self(&["<", "<=", ">", ">=", "==", "!="], op)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_relational_operator_name() {
        let op = RelationalOperator;
        assert_eq!(op.name(), "ROR");
    }

    #[test]
    fn test_get_relational_replacements_less_than() {
        let replacements = get_relational_replacements("<");
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
        let replacements = get_relational_replacements("==");
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
        assert!(is_relational_operator("<"));
        assert!(is_relational_operator("<="));
        assert!(is_relational_operator(">"));
        assert!(is_relational_operator(">="));
        assert!(is_relational_operator("=="));
        assert!(is_relational_operator("!="));
        assert!(!is_relational_operator("+"));
    }
}
