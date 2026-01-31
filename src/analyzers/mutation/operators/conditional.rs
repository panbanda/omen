//! COR (Conditional Operator Replacement) mutation operator.
//!
//! This operator replaces conditional/logical operators:
//! - `&&` -> `||`, `true`
//! - `||` -> `&&`, `false`
//!
//! Kill probability estimate: ~85%
//! These mutations are highly likely to be caught by tests since they
//! fundamentally change boolean logic flow. Surviving mutants typically
//! indicate missing branch coverage or redundant conditions.

use crate::core::Language;
use crate::parser::queries::get_boolean_expression_types;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::generate_binary_operator_mutants;

/// COR (Conditional Operator Replacement) operator.
///
/// Replaces logical AND/OR operators with their counterparts and constant values.
pub struct ConditionalOperator;

impl MutationOperator for ConditionalOperator {
    fn name(&self) -> &'static str {
        "COR"
    }

    fn description(&self) -> &'static str {
        "Conditional Operator Replacement - replaces logical operators (&&, ||)"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let lang = result.language;
        let node_types = get_boolean_expression_types(lang);
        generate_binary_operator_mutants(
            result,
            mutant_id_prefix,
            self.name(),
            node_types,
            |kind| is_conditional_operator(kind, lang),
            |op| get_conditional_replacements(op, lang),
        )
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Check if a node kind represents a conditional/logical operator.
fn is_conditional_operator(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Python => matches!(kind, "and" | "or"),
        _ => matches!(kind, "&&" | "||"),
    }
}

/// Get replacement operators for a conditional operator.
fn get_conditional_replacements(op: &str, lang: Language) -> Vec<String> {
    match op {
        "&&" => vec!["||".to_string(), "true".to_string()],
        "||" => vec!["&&".to_string(), "false".to_string()],
        // Python uses 'and'/'or' keywords
        "and" => vec!["or".to_string(), get_true_literal(lang).to_string()],
        "or" => vec!["and".to_string(), get_false_literal(lang).to_string()],
        _ => Vec::new(),
    }
}

/// Get the true literal for a language.
fn get_true_literal(lang: Language) -> &'static str {
    match lang {
        Language::Python => "True",
        Language::Ruby => "true",
        _ => "true",
    }
}

/// Get the false literal for a language.
fn get_false_literal(lang: Language) -> &'static str {
    match lang {
        Language::Python => "False",
        Language::Ruby => "false",
        _ => "false",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_conditional_operator_name() {
        let op = ConditionalOperator;
        assert_eq!(op.name(), "COR");
    }

    #[test]
    fn test_conditional_operator_description() {
        let op = ConditionalOperator;
        assert!(op.description().contains("Conditional"));
        assert!(op.description().contains("&&"));
    }

    #[test]
    fn test_conditional_operator_supports_all_languages() {
        let op = ConditionalOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::JavaScript));
        assert!(op.supports_language(Language::Java));
        assert!(op.supports_language(Language::Cpp));
    }

    #[test]
    fn test_is_conditional_operator_and() {
        assert!(is_conditional_operator("&&", Language::Rust));
        assert!(is_conditional_operator("&&", Language::JavaScript));
        assert!(!is_conditional_operator("&&", Language::Python));
    }

    #[test]
    fn test_is_conditional_operator_or() {
        assert!(is_conditional_operator("||", Language::Rust));
        assert!(is_conditional_operator("||", Language::Go));
        assert!(!is_conditional_operator("||", Language::Python));
    }

    #[test]
    fn test_is_conditional_operator_python() {
        assert!(is_conditional_operator("and", Language::Python));
        assert!(is_conditional_operator("or", Language::Python));
        assert!(!is_conditional_operator("&&", Language::Python));
    }

    #[test]
    fn test_is_conditional_operator_rejects_other_operators() {
        assert!(!is_conditional_operator("+", Language::Rust));
        assert!(!is_conditional_operator("<", Language::Rust));
        assert!(!is_conditional_operator("==", Language::Rust));
        assert!(!is_conditional_operator("!", Language::Rust));
    }

    #[test]
    fn test_get_conditional_replacements_and() {
        let replacements = get_conditional_replacements("&&", Language::Rust);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"||".to_string()));
        assert!(replacements.contains(&"true".to_string()));
    }

    #[test]
    fn test_get_conditional_replacements_or() {
        let replacements = get_conditional_replacements("||", Language::Rust);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"&&".to_string()));
        assert!(replacements.contains(&"false".to_string()));
    }

    #[test]
    fn test_get_conditional_replacements_python_and() {
        let replacements = get_conditional_replacements("and", Language::Python);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"or".to_string()));
        assert!(replacements.contains(&"True".to_string()));
    }

    #[test]
    fn test_get_conditional_replacements_python_or() {
        let replacements = get_conditional_replacements("or", Language::Python);
        assert_eq!(replacements.len(), 2);
        assert!(replacements.contains(&"and".to_string()));
        assert!(replacements.contains(&"False".to_string()));
    }

    #[test]
    fn test_get_conditional_replacements_unknown_operator() {
        let replacements = get_conditional_replacements("+", Language::Rust);
        assert!(replacements.is_empty());
    }

    #[test]
    fn test_generate_mutants_rust_and() {
        let code = b"fn check(a: bool, b: bool) -> bool { a && b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        // The `||` replacement targets just the operator token.
        let op_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "&&").collect();
        assert!(!op_mutants.is_empty());
        assert!(op_mutants.iter().any(|m| m.replacement == "||"));

        // The `true` replacement targets the entire binary expression.
        let expr_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "true").collect();
        assert!(!expr_mutants.is_empty());
        assert!(expr_mutants[0].original.contains("&&"));
    }

    #[test]
    fn test_generate_mutants_rust_or() {
        let code = b"fn check(a: bool, b: bool) -> bool { a || b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let op_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "||").collect();
        assert!(!op_mutants.is_empty());
        assert!(op_mutants.iter().any(|m| m.replacement == "&&"));

        let expr_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "false").collect();
        assert!(!expr_mutants.is_empty());
        assert!(expr_mutants[0].original.contains("||"));
    }

    #[test]
    fn test_generate_mutants_go_and() {
        let code = b"package main\n\nfunc check(a, b bool) bool { return a && b }";
        let result = parse_code(code, Language::Go);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let and_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "&&").collect();
        assert!(!and_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_javascript_or() {
        let code = b"function check(a, b) { return a || b; }";
        let result = parse_code(code, Language::JavaScript);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let or_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "||").collect();
        assert!(!or_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_python_and() {
        let code = b"def check(a, b): return a and b";
        let result = parse_code(code, Language::Python);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let op_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "and").collect();
        assert!(!op_mutants.is_empty());
        assert!(op_mutants.iter().any(|m| m.replacement == "or"));

        let expr_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "True").collect();
        assert!(!expr_mutants.is_empty());
        assert!(expr_mutants[0].original.contains("and"));
    }

    #[test]
    fn test_generate_mutants_python_or() {
        let code = b"def check(a, b): return a or b";
        let result = parse_code(code, Language::Python);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let op_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "or").collect();
        assert!(!op_mutants.is_empty());
        assert!(op_mutants.iter().any(|m| m.replacement == "and"));

        let expr_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "False").collect();
        assert!(!expr_mutants.is_empty());
        assert!(expr_mutants[0].original.contains("or"));
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"fn check(a: bool, b: bool) -> bool { a && b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "COR");
        }
    }

    #[test]
    fn test_mutant_has_correct_position() {
        let code = b"fn check(a: bool, b: bool) -> bool {\n    a && b\n}";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");
        let and_mutant = mutants.iter().find(|m| m.original == "&&").unwrap();

        // && should be on line 2 (1-indexed)
        assert_eq!(and_mutant.line, 2);
    }

    #[test]
    fn test_mutant_byte_range_is_correct() {
        let code = b"fn f() { a && b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");
        let and_mutant = mutants.iter().find(|m| m.original == "&&").unwrap();

        let (start, end) = and_mutant.byte_range;
        assert_eq!(&code[start..end], b"&&");
    }

    #[test]
    fn test_generate_mutants_multiple_operators() {
        let code = b"fn check(a: bool, b: bool, c: bool) -> bool { (a && b) || c }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have mutants for both && and ||
        let and_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "&&").collect();
        let or_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "||").collect();

        assert!(!and_mutants.is_empty());
        assert!(!or_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_no_conditionals() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        // No conditional operators in the code
        assert!(mutants.is_empty());
    }

    #[test]
    fn test_mutant_description_is_meaningful() {
        let code = b"fn f() { a && b }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(mutant.description.contains("Replace"));
            assert!(mutant.description.contains(&mutant.original));
            assert!(mutant.description.contains(&mutant.replacement));
        }
    }

    #[test]
    fn test_mutant_id_is_unique() {
        let code = b"fn f() { (a && b) || (c && d) }";
        let result = parse_code(code, Language::Rust);
        let op = ConditionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mut ids: Vec<_> = mutants.iter().map(|m| &m.id).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();

        assert_eq!(ids.len(), original_len, "All mutant IDs should be unique");
    }

    #[test]
    fn test_get_true_literal_by_language() {
        assert_eq!(get_true_literal(Language::Rust), "true");
        assert_eq!(get_true_literal(Language::Python), "True");
        assert_eq!(get_true_literal(Language::Ruby), "true");
        assert_eq!(get_true_literal(Language::JavaScript), "true");
    }

    #[test]
    fn test_get_false_literal_by_language() {
        assert_eq!(get_false_literal(Language::Rust), "false");
        assert_eq!(get_false_literal(Language::Python), "False");
        assert_eq!(get_false_literal(Language::Ruby), "false");
        assert_eq!(get_false_literal(Language::JavaScript), "false");
    }
}
