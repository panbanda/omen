//! Go-specific error handling mutation operators.
//!
//! This operator mutates Go error handling patterns:
//! - `err != nil` -> `err == nil` (invert error check)
//! - `if err != nil { return err }` -> `if err != nil { }` (swallow error)

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// GER (Go Error Replacement) operator.
///
/// Mutates Go error handling patterns to test error handling coverage.
pub struct GoErrorOperator;

impl MutationOperator for GoErrorOperator {
    fn name(&self) -> &'static str {
        "GER"
    }

    fn description(&self) -> &'static str {
        "Go Error Replacement - mutates error handling patterns"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "binary_expression" => self
                .try_mutate_error_check(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            "if_statement" => self
                .try_mutate_error_return(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Go)
    }
}

impl GoErrorOperator {
    /// Try to mutate `err != nil` to `err == nil` or vice versa.
    fn try_mutate_error_check(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Check if this is a comparison with nil
        let mut has_nil = false;
        let mut has_err_like = false;
        let mut operator_node = None;

        for child in node.children(&mut node.walk()) {
            let kind = child.kind();
            if kind == "nil" {
                has_nil = true;
            } else if kind == "identifier" {
                if let Ok(text) = child.utf8_text(&result.source) {
                    // Common error variable names
                    if text == "err" || text.ends_with("Err") || text.ends_with("Error") {
                        has_err_like = true;
                    }
                }
            } else if kind == "!=" || kind == "==" {
                operator_node = Some(child);
            }
        }

        if has_nil && has_err_like {
            if let Some(op) = operator_node {
                if let Ok(op_text) = op.utf8_text(&result.source) {
                    let replacement = if op_text == "!=" { "==" } else { "!=" };
                    *counter += 1;
                    let id = format!("{}-{}", prefix, counter);
                    let start = op.start_position();

                    return Some(Mutant::new(
                        id,
                        result.path.clone(),
                        self.name(),
                        (start.row + 1) as u32,
                        (start.column + 1) as u32,
                        op_text,
                        replacement,
                        format!("Invert error check: {} -> {}", op_text, replacement),
                        (op.start_byte(), op.end_byte()),
                    ));
                }
            }
        }

        None
    }

    /// Try to mutate `if err != nil { return err }` to `if err != nil { }`.
    fn try_mutate_error_return(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Check if condition is an error check
        let condition = node.child_by_field_name("condition")?;
        if condition.kind() != "binary_expression" {
            return None;
        }

        // Check for nil comparison
        let mut has_nil = false;
        let mut has_err_like = false;
        for child in condition.children(&mut condition.walk()) {
            if child.kind() == "nil" {
                has_nil = true;
            } else if child.kind() == "identifier" {
                if let Ok(text) = child.utf8_text(&result.source) {
                    if text == "err" || text.ends_with("Err") || text.ends_with("Error") {
                        has_err_like = true;
                    }
                }
            }
        }

        if !has_nil || !has_err_like {
            return None;
        }

        // Find the consequence block
        let consequence = node.child_by_field_name("consequence")?;
        if consequence.kind() != "block" {
            return None;
        }

        // Check if block contains a return statement with error
        let mut has_error_return = false;
        for child in consequence.children(&mut consequence.walk()) {
            if child.kind() == "return_statement" {
                if let Ok(text) = child.utf8_text(&result.source) {
                    if text.contains("err") {
                        has_error_return = true;
                        break;
                    }
                }
            }
        }

        if !has_error_return {
            return None;
        }

        // Get the block content to replace with empty block
        let block_start = consequence.start_byte();
        let block_end = consequence.end_byte();

        *counter += 1;
        let id = format!("{}-{}", prefix, counter);
        let start = consequence.start_position();

        let original = std::str::from_utf8(&result.source[block_start..block_end])
            .unwrap_or("")
            .to_string();

        Some(Mutant::new(
            id,
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            original,
            "{ }",
            "Swallow error: remove error return",
            (block_start, block_end),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::test_utils::parse_go;
    #[test]
    fn test_go_error_operator_name() {
        let op = GoErrorOperator;
        assert_eq!(op.name(), "GER");
    }

    #[test]
    fn test_go_error_operator_description() {
        let op = GoErrorOperator;
        assert!(op.description().contains("error"));
    }

    #[test]
    fn test_supports_only_go() {
        let op = GoErrorOperator;
        assert!(op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::JavaScript));
    }

    #[test]
    fn test_mutate_err_not_nil_to_eq_nil() {
        let code = b"package main\n\nfunc foo() error {\n\tif err != nil {\n\t\treturn err\n\t}\n\treturn nil\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find at least one mutation for the error check
        let err_check_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "!=" && m.replacement == "==")
            .collect();
        assert!(
            !err_check_mutants.is_empty(),
            "Should mutate err != nil to err == nil"
        );
    }

    #[test]
    fn test_mutate_err_eq_nil_to_not_nil() {
        let code = b"package main\n\nfunc foo() {\n\tif err == nil {\n\t\tdoSomething()\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        let err_check_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "==" && m.replacement == "!=")
            .collect();
        assert!(
            !err_check_mutants.is_empty(),
            "Should mutate err == nil to err != nil"
        );
    }

    #[test]
    fn test_swallow_error_return() {
        let code = b"package main\n\nfunc foo() error {\n\tif err != nil {\n\t\treturn err\n\t}\n\treturn nil\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutation to swallow error
        let swallow_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "{ }").collect();
        assert!(
            !swallow_mutants.is_empty(),
            "Should create swallow error mutation"
        );
    }

    #[test]
    fn test_no_mutation_for_non_error_variable() {
        let code = b"package main\n\nfunc foo() {\n\tif x != nil {\n\t\treturn\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should not mutate non-error variables
        let err_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("error"))
            .collect();
        assert!(
            err_mutants.is_empty(),
            "Should not mutate non-error variable checks"
        );
    }

    #[test]
    fn test_mutant_has_correct_operator_name() {
        let code = b"package main\n\nfunc foo() {\n\tif err != nil {\n\t\treturn err\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "GER");
        }
    }

    #[test]
    fn test_mutant_byte_range_valid() {
        let code = b"package main\n\nfunc foo() {\n\tif err != nil {\n\t\treturn err\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end, "Byte range should be valid");
            assert!(end <= code.len(), "Byte range should be within source");
        }
    }

    #[test]
    fn test_error_suffix_variable() {
        let code = b"package main\n\nfunc foo() {\n\tif parseError != nil {\n\t\treturn parseError\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty(), "Should recognize Error suffix");
    }

    #[test]
    fn test_err_suffix_variable() {
        let code =
            b"package main\n\nfunc foo() {\n\tif decodeErr != nil {\n\t\treturn decodeErr\n\t}\n}";
        let result = parse_go(code);
        let op = GoErrorOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty(), "Should recognize Err suffix");
    }
}
