//! Go-specific nil mutation operators.
//!
//! This operator mutates Go nil-related patterns:
//! - `nil` -> `&Type{}` (non-nil pointer, when possible)
//! - `x == nil` -> `x != nil`
//! - `x != nil` -> `x == nil`

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// GNR (Go Nil Replacement) operator.
///
/// Mutates Go nil comparisons and nil values to test nil handling coverage.
pub struct GoNilOperator;

impl MutationOperator for GoNilOperator {
    fn name(&self) -> &'static str {
        "GNR"
    }

    fn description(&self) -> &'static str {
        "Go Nil Replacement - mutates nil comparisons and values"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "binary_expression" => self
                .try_mutate_nil_comparison(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            "nil" => {
                let skip = node
                    .parent()
                    .is_some_and(|p| p.kind() == "binary_expression");
                if skip {
                    return Vec::new();
                }
                self.try_mutate_nil_literal(&node, result, mutant_id_prefix, &mut counter)
                    .map(|m| vec![m])
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Go)
    }
}

impl GoNilOperator {
    /// Try to mutate nil comparisons: `x == nil` -> `x != nil` or vice versa.
    fn try_mutate_nil_comparison(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Check if this comparison involves nil
        let mut has_nil = false;
        let mut operator_node = None;

        for child in node.children(&mut node.walk()) {
            let kind = child.kind();
            if kind == "nil" {
                has_nil = true;
            } else if kind == "!=" || kind == "==" {
                operator_node = Some(child);
            }
        }

        if !has_nil {
            return None;
        }

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
                    format!("Invert nil check: {} -> {}", op_text, replacement),
                    (op.start_byte(), op.end_byte()),
                ));
            }
        }

        None
    }

    /// Try to mutate standalone nil literal to a non-nil value.
    fn try_mutate_nil_literal(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Try to infer type from context
        let replacement = self.infer_non_nil_replacement(node, result);

        *counter += 1;
        let id = format!("{}-{}", prefix, counter);
        let start = node.start_position();

        Some(Mutant::new(
            id,
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            "nil",
            replacement,
            "Replace nil with non-nil value",
            (node.start_byte(), node.end_byte()),
        ))
    }

    /// Infer a non-nil replacement based on context.
    fn infer_non_nil_replacement(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
    ) -> String {
        // Check parent for context clues
        if let Some(parent) = node.parent() {
            match parent.kind() {
                "return_statement" => {
                    // For returns, use a generic non-nil placeholder
                    return "&struct{}{}".to_string();
                }
                "assignment_statement" | "short_var_declaration" => {
                    // Try to get variable name for context
                    if let Some(left) = parent.child_by_field_name("left") {
                        if let Ok(text) = left.utf8_text(&result.source) {
                            // Common patterns
                            if text.contains("slice")
                                || text.contains("list")
                                || text.contains("items")
                            {
                                return "[]interface{}{}".to_string();
                            }
                            if text.contains("map") {
                                return "map[string]interface{}{}".to_string();
                            }
                            if text.contains("chan") {
                                return "make(chan struct{})".to_string();
                            }
                        }
                    }
                    return "&struct{}{}".to_string();
                }
                "argument_list" | "call_expression" => {
                    // For function arguments, use generic pointer
                    return "&struct{}{}".to_string();
                }
                _ => {}
            }
        }

        // Default: empty struct pointer
        "&struct{}{}".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::test_utils::parse_go;
    #[test]
    fn test_go_nil_operator_name() {
        let op = GoNilOperator;
        assert_eq!(op.name(), "GNR");
    }

    #[test]
    fn test_go_nil_operator_description() {
        let op = GoNilOperator;
        assert!(op.description().contains("nil"));
    }

    #[test]
    fn test_supports_only_go() {
        let op = GoNilOperator;
        assert!(op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::Java));
    }

    #[test]
    fn test_mutate_eq_nil_to_ne_nil() {
        let code = b"package main\n\nfunc foo(x *int) bool {\n\treturn x == nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nil_check_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "==" && m.replacement == "!=")
            .collect();
        assert!(
            !nil_check_mutants.is_empty(),
            "Should mutate x == nil to x != nil"
        );
    }

    #[test]
    fn test_mutate_ne_nil_to_eq_nil() {
        let code = b"package main\n\nfunc foo(x *int) bool {\n\treturn x != nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nil_check_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "!=" && m.replacement == "==")
            .collect();
        assert!(
            !nil_check_mutants.is_empty(),
            "Should mutate x != nil to x == nil"
        );
    }

    #[test]
    fn test_mutate_nil_return() {
        let code = b"package main\n\nfunc foo() *int {\n\treturn nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nil_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "nil").collect();
        assert!(!nil_mutants.is_empty(), "Should mutate standalone nil");
    }

    #[test]
    fn test_mutant_has_correct_operator() {
        let code = b"package main\n\nfunc foo(x *int) bool {\n\treturn x == nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "GNR");
        }
    }

    #[test]
    fn test_mutant_line_number_correct() {
        let code = b"package main\n\nfunc foo(x *int) bool {\n\treturn x == nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        // The nil check is on line 4
        let line4_mutants: Vec<_> = mutants.iter().filter(|m| m.line == 4).collect();
        assert!(
            !line4_mutants.is_empty(),
            "Should find mutants on correct line"
        );
    }

    #[test]
    fn test_no_mutation_for_non_nil_comparison() {
        let code = b"package main\n\nfunc foo(x, y int) bool {\n\treturn x == y\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should not produce any mutants for non-nil comparisons
        assert!(mutants.is_empty(), "Should not mutate non-nil comparisons");
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"package main\n\nfunc foo(x *int) bool {\n\treturn x == nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_nil_in_assignment() {
        let code = b"package main\n\nfunc foo() {\n\tx := nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nil_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "nil").collect();
        assert!(!nil_mutants.is_empty(), "Should mutate nil in assignment");
    }

    #[test]
    fn test_multiple_nil_comparisons() {
        let code = b"package main\n\nfunc foo(x, y *int) bool {\n\treturn x == nil && y != nil\n}";
        let result = parse_go(code);
        let op = GoNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutations for both comparisons
        let eq_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "==").collect();
        let ne_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "!=").collect();

        assert!(!eq_mutants.is_empty(), "Should mutate == nil");
        assert!(!ne_mutants.is_empty(), "Should mutate != nil");
    }
}
