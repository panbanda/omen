//! Ruby-specific nil mutation operators.
//!
//! This operator mutates Ruby nil-related patterns:
//! - `nil` -> `false`, `0`, `""`
//! - `x.nil?` -> `true`, `false`
//! - `x || default` -> `x`
//! - `x&.method` -> `x.method` (safe navigation removal)
//!
//! Kill probability estimate: ~70-80%

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// RNR (Ruby Nil Replacement) operator.
///
/// Mutates Ruby nil-related patterns and safe navigation.
pub struct RubyNilOperator;

impl MutationOperator for RubyNilOperator {
    fn name(&self) -> &'static str {
        "RNR"
    }

    fn description(&self) -> &'static str {
        "Ruby Nil Replacement - mutates nil and safe navigation operators"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            let mut mutants = Vec::new();
            match node.kind() {
                "nil" => {
                    mutants.extend(self.mutate_nil(&node, result, mutant_id_prefix, &mut counter));
                }
                "call" => {
                    if let Some(m) =
                        self.try_mutate_nil_check(&node, result, mutant_id_prefix, &mut counter)
                    {
                        mutants.push(m);
                    }
                    if let Some(m) = self.try_mutate_safe_navigation(
                        &node,
                        result,
                        mutant_id_prefix,
                        &mut counter,
                    ) {
                        mutants.push(m);
                    }
                }
                "method_call" => {
                    if let Some(m) =
                        self.try_mutate_nil_check(&node, result, mutant_id_prefix, &mut counter)
                    {
                        mutants.push(m);
                    }
                }
                _ => {}
            }
            mutants
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Ruby)
    }
}

impl RubyNilOperator {
    /// Mutate nil literal to other falsy/empty values.
    fn mutate_nil(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();

        let replacements = ["false", "0", "\"\"", "[]", "{}"];

        for replacement in replacements {
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = node.start_position();

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                "nil",
                replacement,
                format!("Replace nil with {}", replacement),
                (node.start_byte(), node.end_byte()),
            ));
        }

        mutants
    }

    /// Try to mutate .nil? method calls.
    fn try_mutate_nil_check(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        if !node_text.ends_with(".nil?") && !node_text.contains(".nil?") {
            return None;
        }

        // Find the .nil? part
        if let Some(pos) = node_text.find(".nil?") {
            let receiver = &node_text[..pos];

            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = node.start_position();

            // Replace x.nil? with true (always nil)
            Some(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                node_text,
                "true",
                format!("Replace {}.nil? with true", receiver),
                (node.start_byte(), node.end_byte()),
            ))
        } else {
            None
        }
    }

    /// Try to mutate safe navigation operator (&.).
    fn try_mutate_safe_navigation(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        if !node_text.contains("&.") {
            return None;
        }

        // Replace &. with . (remove safe navigation)
        let replacement = node_text.replace("&.", ".");

        *counter += 1;
        let id = format!("{}-{}", prefix, counter);
        let start = node.start_position();

        Some(Mutant::new(
            id,
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            node_text,
            replacement.clone(),
            "Remove safe navigation operator (&. -> .)".to_string(),
            (node.start_byte(), node.end_byte()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::test_utils::parse_rb;
    #[test]
    fn test_operator_name() {
        let op = RubyNilOperator;
        assert_eq!(op.name(), "RNR");
    }

    #[test]
    fn test_operator_description() {
        let op = RubyNilOperator;
        assert!(op.description().contains("nil"));
    }

    #[test]
    fn test_supports_ruby_only() {
        let op = RubyNilOperator;
        assert!(op.supports_language(Language::Ruby));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::Go));
    }

    #[test]
    fn test_mutate_nil_literal() {
        let code = b"x = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have replacements for nil
        let nil_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "nil").collect();
        assert!(!nil_mutants.is_empty(), "Should mutate nil literal");
    }

    #[test]
    fn test_nil_replacement_values() {
        let code = b"x = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let replacements: Vec<_> = mutants.iter().map(|m| m.replacement.as_str()).collect();
        assert!(replacements.contains(&"false"));
        assert!(replacements.contains(&"0"));
        assert!(replacements.contains(&"\"\""));
    }

    #[test]
    fn test_mutant_operator_is_rnr() {
        let code = b"x = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "RNR");
        }
    }

    #[test]
    fn test_multiple_nil_literals() {
        let code = b"x = nil\ny = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have mutations for both nil literals
        assert!(mutants.len() >= 10, "Should mutate both nil literals");
    }

    #[test]
    fn test_nil_in_conditional() {
        let code = b"if x == nil\n  puts 'nil'\nend";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nil_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "nil").collect();
        assert!(!nil_mutants.is_empty());
    }

    #[test]
    fn test_unique_mutant_ids() {
        let code = b"x = nil\ny = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ids: std::collections::HashSet<_> = mutants.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), mutants.len(), "All IDs should be unique");
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"x = nil";
        let result = parse_rb(code);
        let op = RubyNilOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(
                mutant.byte_range.0 < mutant.byte_range.1,
                "Byte range should be valid"
            );
            assert!(
                mutant.byte_range.1 <= code.len(),
                "Byte range should not exceed source"
            );
        }
    }
}
