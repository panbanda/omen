//! TypeScript-specific equality mutation operators.
//!
//! This operator mutates TypeScript/JavaScript equality operators:
//! - `===` -> `==` (strict to loose equality)
//! - `!==` -> `!=` (strict to loose inequality)
//! - `==` -> `===` (loose to strict equality)
//! - `!=` -> `!==` (loose to strict inequality)

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// TER (TypeScript Equality Replacement) operator.
///
/// Mutates between strict and loose equality operators.
pub struct TypeScriptEqualityOperator;

impl MutationOperator for TypeScriptEqualityOperator {
    fn name(&self) -> &'static str {
        "TER"
    }

    fn description(&self) -> &'static str {
        "TypeScript Equality Replacement - mutates between strict and loose equality"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "binary_expression" => {
                self.try_mutate_equality(&node, result, mutant_id_prefix, &mut counter)
            }
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(
            lang,
            Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx
        )
    }
}

impl TypeScriptEqualityOperator {
    /// Try to mutate equality operators.
    fn try_mutate_equality(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();

        for child in node.children(&mut node.walk()) {
            let kind = child.kind();
            if !is_equality_operator(kind) {
                continue;
            }

            if let Ok(op_text) = child.utf8_text(&result.source) {
                let replacements = get_equality_replacements(op_text);
                for replacement in replacements {
                    *counter += 1;
                    let id = format!("{}-{}", prefix, counter);
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

        mutants
    }
}

/// Check if a node kind is an equality operator.
fn is_equality_operator(kind: &str) -> bool {
    matches!(kind, "===" | "!==" | "==" | "!=")
}

/// Get replacement operators for an equality operator.
fn get_equality_replacements(op: &str) -> Vec<String> {
    match op {
        "===" => vec!["==".to_string()],
        "!==" => vec!["!=".to_string()],
        "==" => vec!["===".to_string()],
        "!=" => vec!["!==".to_string()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::test_utils::{parse_js, parse_ts};
    #[test]
    fn test_ts_equality_operator_name() {
        let op = TypeScriptEqualityOperator;
        assert_eq!(op.name(), "TER");
    }

    #[test]
    fn test_ts_equality_operator_description() {
        let op = TypeScriptEqualityOperator;
        assert!(op.description().contains("equality"));
    }

    #[test]
    fn test_supports_typescript_variants() {
        let op = TypeScriptEqualityOperator;
        assert!(op.supports_language(Language::TypeScript));
        assert!(op.supports_language(Language::JavaScript));
        assert!(op.supports_language(Language::Tsx));
        assert!(op.supports_language(Language::Jsx));
    }

    #[test]
    fn test_does_not_support_other_languages() {
        let op = TypeScriptEqualityOperator;
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::Java));
    }

    #[test]
    fn test_mutate_strict_eq_to_loose_eq() {
        let code = b"const x = a === b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        let eq_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "===" && m.replacement == "==")
            .collect();
        assert!(!eq_mutants.is_empty(), "Should mutate === to ==");
    }

    #[test]
    fn test_mutate_strict_ne_to_loose_ne() {
        let code = b"const x = a !== b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ne_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "!==" && m.replacement == "!=")
            .collect();
        assert!(!ne_mutants.is_empty(), "Should mutate !== to !=");
    }

    #[test]
    fn test_mutate_loose_eq_to_strict_eq() {
        let code = b"const x = a == b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        let eq_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "==" && m.replacement == "===")
            .collect();
        assert!(!eq_mutants.is_empty(), "Should mutate == to ===");
    }

    #[test]
    fn test_mutate_loose_ne_to_strict_ne() {
        let code = b"const x = a != b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ne_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "!=" && m.replacement == "!==")
            .collect();
        assert!(!ne_mutants.is_empty(), "Should mutate != to !==");
    }

    #[test]
    fn test_mutant_operator_is_ter() {
        let code = b"const x = a === b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "TER");
        }
    }

    #[test]
    fn test_works_with_javascript() {
        let code = b"const x = a === b;";
        let result = parse_js(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty(), "Should work with JavaScript");
    }

    #[test]
    fn test_multiple_equality_operators() {
        let code = b"const x = a === b && c !== d;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        let eq_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "===").collect();
        let ne_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "!==").collect();

        assert!(!eq_mutants.is_empty(), "Should find === mutation");
        assert!(!ne_mutants.is_empty(), "Should find !== mutation");
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"const x = a === b;";
        let result = parse_ts(code);
        let op = TypeScriptEqualityOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_get_equality_replacements() {
        assert_eq!(get_equality_replacements("==="), vec!["==".to_string()]);
        assert_eq!(get_equality_replacements("!=="), vec!["!=".to_string()]);
        assert_eq!(get_equality_replacements("=="), vec!["===".to_string()]);
        assert_eq!(get_equality_replacements("!="), vec!["!==".to_string()]);
        assert!(get_equality_replacements("+").is_empty());
    }
}
