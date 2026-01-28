//! TypeScript-specific optional chaining and nullish coalescing mutation operators.
//!
//! This operator mutates TypeScript/JavaScript optional operators:
//! - `?.` -> `.` (remove optional chaining)
//! - `??` -> `||` (nullish coalescing to logical or)
//! - `x!` -> `x` (remove non-null assertion)

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// TOR (TypeScript Optional Replacement) operator.
///
/// Mutates optional chaining, nullish coalescing, and non-null assertions.
pub struct TypeScriptOptionalOperator;

impl MutationOperator for TypeScriptOptionalOperator {
    fn name(&self) -> &'static str {
        "TOR"
    }

    fn description(&self) -> &'static str {
        "TypeScript Optional Replacement - mutates optional chaining and nullish coalescing"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "optional_chain_expression" | "member_expression" => self
                .try_mutate_optional_chain(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            "binary_expression" => self
                .try_mutate_nullish_coalescing(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
            "non_null_expression" => self
                .try_mutate_non_null_assertion(&node, result, mutant_id_prefix, &mut counter)
                .map(|m| vec![m])
                .unwrap_or_default(),
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

impl TypeScriptOptionalOperator {
    /// Try to mutate optional chaining: `?.` -> `.`
    fn try_mutate_optional_chain(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        // Look for ?. in the expression
        if !node_text.contains("?.") {
            return None;
        }

        // Find the position of ?. in the source
        let node_start = node.start_byte();
        let node_end = node.end_byte();
        let source_slice = &result.source[node_start..node_end];

        // Find ?. position within the node
        let opt_chain_pos = source_slice.windows(2).position(|w| w == b"?.")?;

        let chain_start = node_start + opt_chain_pos;
        let chain_end = chain_start + 2;

        *counter += 1;
        let id = format!("{}-{}", prefix, counter);
        let start = node.start_position();

        Some(Mutant::new(
            id,
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + opt_chain_pos + 1) as u32,
            "?.",
            ".",
            "Remove optional chaining: ?. -> .",
            (chain_start, chain_end),
        ))
    }

    /// Try to mutate nullish coalescing: `??` -> `||`
    fn try_mutate_nullish_coalescing(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Look for ?? operator child
        for child in node.children(&mut node.walk()) {
            if child.kind() == "??" {
                if let Ok(op_text) = child.utf8_text(&result.source) {
                    if op_text == "??" {
                        *counter += 1;
                        let id = format!("{}-{}", prefix, counter);
                        let start = child.start_position();

                        return Some(Mutant::new(
                            id,
                            result.path.clone(),
                            self.name(),
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            "??",
                            "||",
                            "Replace nullish coalescing with logical or: ?? -> ||",
                            (child.start_byte(), child.end_byte()),
                        ));
                    }
                }
            }
        }

        None
    }

    /// Try to mutate non-null assertion: `x!` -> `x`
    fn try_mutate_non_null_assertion(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        let node_text = node.utf8_text(&result.source).ok()?;

        // The non-null expression includes the inner expression and the !
        // We want to replace the whole thing with just the inner expression
        if !node_text.ends_with('!') {
            return None;
        }

        // Get the inner expression (everything except the !)
        let inner_text = &node_text[..node_text.len() - 1];

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
            inner_text,
            "Remove non-null assertion: x! -> x",
            (node.start_byte(), node.end_byte()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use std::path::Path;

    fn parse_ts(code: &[u8]) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code, Language::TypeScript, Path::new("test.ts"))
            .unwrap()
    }

    #[test]
    fn test_ts_optional_operator_name() {
        let op = TypeScriptOptionalOperator;
        assert_eq!(op.name(), "TOR");
    }

    #[test]
    fn test_ts_optional_operator_description() {
        let op = TypeScriptOptionalOperator;
        assert!(op.description().contains("optional"));
    }

    #[test]
    fn test_supports_typescript_variants() {
        let op = TypeScriptOptionalOperator;
        assert!(op.supports_language(Language::TypeScript));
        assert!(op.supports_language(Language::JavaScript));
        assert!(op.supports_language(Language::Tsx));
        assert!(op.supports_language(Language::Jsx));
    }

    #[test]
    fn test_does_not_support_other_languages() {
        let op = TypeScriptOptionalOperator;
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Python));
    }

    #[test]
    fn test_mutate_optional_chain() {
        let code = b"const x = obj?.prop;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let chain_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "?." && m.replacement == ".")
            .collect();
        assert!(!chain_mutants.is_empty(), "Should mutate ?. to .");
    }

    #[test]
    fn test_mutate_nullish_coalescing() {
        let code = b"const x = a ?? b;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nc_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original == "??" && m.replacement == "||")
            .collect();
        assert!(!nc_mutants.is_empty(), "Should mutate ?? to ||");
    }

    #[test]
    fn test_mutate_non_null_assertion() {
        let code = b"const x = value!;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nna_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("non-null assertion"))
            .collect();
        assert!(!nna_mutants.is_empty(), "Should mutate x! to x");
    }

    #[test]
    fn test_mutant_operator_is_tor() {
        let code = b"const x = obj?.prop;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "TOR");
        }
    }

    #[test]
    fn test_nested_optional_chain() {
        let code = b"const x = obj?.nested?.prop;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let chain_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "?.").collect();
        // Should find mutations for the optional chains
        assert!(
            !chain_mutants.is_empty(),
            "Should find optional chain mutations"
        );
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"const x = obj?.prop;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_optional_call() {
        // Optional calls may be parsed as call_expression with optional member access
        let code = b"const x = obj?.method();";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let chain_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "?.").collect();
        assert!(!chain_mutants.is_empty(), "Should mutate optional call");
    }

    #[test]
    fn test_no_mutation_for_regular_member_access() {
        let code = b"const x = obj.prop;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let chain_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "?.").collect();
        assert!(
            chain_mutants.is_empty(),
            "Should not mutate regular member access"
        );
    }

    #[test]
    fn test_nullish_coalescing_vs_logical_or() {
        let code = b"const x = a || b;";
        let result = parse_ts(code);
        let op = TypeScriptOptionalOperator;

        let mutants = op.generate_mutants(&result, "test");

        let nc_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "??").collect();
        assert!(
            nc_mutants.is_empty(),
            "Should not mutate regular logical or"
        );
    }
}
