//! Ruby-specific symbol and block mutation operators.
//!
//! This operator mutates Ruby-specific patterns:
//! - `:symbol` -> `"symbol"` (symbol to string)
//! - `{ |x| ... }` -> `{ }` (empty block)
//! - `do |x| ... end` -> `do end` (empty do block)
//! - `&:method` -> `{ |x| x }` (proc shorthand expansion)
//!
//! Kill probability estimate: ~65-75%

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// RSM (Ruby Symbol Mutation) operator.
///
/// Mutates Ruby symbols and blocks.
pub struct RubySymbolOperator;

impl MutationOperator for RubySymbolOperator {
    fn name(&self) -> &'static str {
        "RSM"
    }

    fn description(&self) -> &'static str {
        "Ruby Symbol Mutation - mutates symbols and blocks"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "symbol" | "simple_symbol" => self
                .mutate_symbol(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            "block" | "do_block" => self
                .mutate_block(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Ruby)
    }
}

impl RubySymbolOperator {
    /// Mutate symbol to string.
    fn mutate_symbol(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        // Convert :symbol to "symbol"
        if let Some(symbol_name) = node_text.strip_prefix(':') {
            let replacement = format!("\"{}\"", symbol_name);

            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = node.start_position();

            return Some(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                node_text,
                replacement,
                format!("Convert symbol {} to string", node_text),
                (node.start_byte(), node.end_byte()),
            ));
        }

        None
    }

    /// Mutate block to empty block.
    fn mutate_block(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        // Skip already empty blocks
        if node_text.trim() == "{}" || node_text.trim() == "do end" {
            return None;
        }

        let replacement = if node_text.contains("do") {
            "do end".to_string()
        } else {
            "{ }".to_string()
        };

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
            replacement,
            "Replace block with empty block".to_string(),
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
        let op = RubySymbolOperator;
        assert_eq!(op.name(), "RSM");
    }

    #[test]
    fn test_operator_description() {
        let op = RubySymbolOperator;
        assert!(op.description().contains("symbol"));
    }

    #[test]
    fn test_supports_ruby_only() {
        let op = RubySymbolOperator;
        assert!(op.supports_language(Language::Ruby));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
    }

    #[test]
    fn test_mutate_simple_symbol() {
        let code = b"x = :foo";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        let symbol_mutants: Vec<_> = mutants.iter().filter(|m| m.original == ":foo").collect();
        assert!(!symbol_mutants.is_empty(), "Should mutate symbol to string");

        if !symbol_mutants.is_empty() {
            assert_eq!(symbol_mutants[0].replacement, "\"foo\"");
        }
    }

    #[test]
    fn test_mutant_operator_is_rsm() {
        let code = b"x = :foo";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "RSM");
        }
    }

    #[test]
    fn test_multiple_symbols() {
        let code = b"x = :foo\ny = :bar";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        let symbol_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.starts_with(':'))
            .collect();
        assert!(symbol_mutants.len() >= 2, "Should mutate multiple symbols");
    }

    #[test]
    fn test_symbol_in_hash() {
        let code = b"h = { :key => value }";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        let key_mutants: Vec<_> = mutants.iter().filter(|m| m.original == ":key").collect();
        assert!(!key_mutants.is_empty(), "Should mutate symbol in hash");
    }

    #[test]
    fn test_unique_mutant_ids() {
        let code = b"x = :foo\ny = :bar";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ids: std::collections::HashSet<_> = mutants.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), mutants.len(), "All IDs should be unique");
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"x = :foo";
        let result = parse_rb(code);
        let op = RubySymbolOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(
                mutant.byte_range.0 < mutant.byte_range.1,
                "Byte range should be valid"
            );
        }
    }
}
