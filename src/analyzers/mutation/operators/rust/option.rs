//! Rust Option mutation operator.
//!
//! This operator mutates Option-related code:
//! - `Option::Some(x)` -> `Option::None`
//! - `Some(x)` -> `None`
//! - `.unwrap()` -> `.expect("...")`
//! - `.unwrap_or(default)` -> `.unwrap()`

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::super::operator::MutationOperator;
use super::super::super::Mutant;

/// Rust Option mutation operator.
///
/// Mutates Option constructors and method calls to test error handling.
pub struct OptionOperator;

impl MutationOperator for OptionOperator {
    fn name(&self) -> &'static str {
        "RustOption"
    }

    fn description(&self) -> &'static str {
        "Rust Option Mutation - replaces Some with None and mutates unwrap calls"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let root = result.root_node();

        let mut counter = 0;
        let mut cursor = root.walk();

        loop {
            let node = cursor.node();
            let kind = node.kind();

            match kind {
                // Match call expressions like Some(x), Option::Some(x), .unwrap(), etc.
                "call_expression" => {
                    if let Some(mutant_list) =
                        self.handle_call_expression(&node, result, mutant_id_prefix, &mut counter)
                    {
                        mutants.extend(mutant_list);
                    }
                }
                // Match macro invocations like Some!(x) (rare but possible)
                "macro_invocation" => {
                    if let Some(mutant_list) =
                        self.handle_macro_invocation(&node, result, mutant_id_prefix, &mut counter)
                    {
                        mutants.extend(mutant_list);
                    }
                }
                _ => {}
            }

            // Tree traversal
            if cursor.goto_first_child() {
                continue;
            }

            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return mutants;
                }
            }
        }
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Rust)
    }
}

impl OptionOperator {
    /// Handle call expressions for Option mutations.
    fn handle_call_expression(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut usize,
    ) -> Option<Vec<Mutant>> {
        let mut mutants = Vec::new();

        // Get the function being called
        let function_node = node.child_by_field_name("function")?;
        let function_text = function_node.utf8_text(&result.source).ok()?;

        // Check for Some(x) or Option::Some(x)
        if function_text == "Some" || function_text.ends_with("::Some") {
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                "None",
                format!("Replace {} with None", original),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for method calls like .unwrap(), .unwrap_or(...)
        if function_text.ends_with(".unwrap") {
            // .unwrap() -> .expect("mutated")
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            // Find the receiver (everything before .unwrap)
            let receiver_end = function_text.len() - ".unwrap".len();
            let receiver = &function_text[..receiver_end];

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("{}.expect(\"mutated\")", receiver),
                "Replace .unwrap() with .expect(\"mutated\")".to_string(),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if function_text.ends_with(".unwrap_or") {
            // .unwrap_or(default) -> .unwrap()
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            // Find the receiver (everything before .unwrap_or)
            let receiver_end = function_text.len() - ".unwrap_or".len();
            let receiver = &function_text[..receiver_end];

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("{}.unwrap()", receiver),
                "Replace .unwrap_or(...) with .unwrap()".to_string(),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if function_text.ends_with(".unwrap_or_else") {
            // .unwrap_or_else(f) -> .unwrap()
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            let receiver_end = function_text.len() - ".unwrap_or_else".len();
            let receiver = &function_text[..receiver_end];

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("{}.unwrap()", receiver),
                "Replace .unwrap_or_else(...) with .unwrap()".to_string(),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if function_text.ends_with(".unwrap_or_default") {
            // .unwrap_or_default() -> .unwrap()
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            let receiver_end = function_text.len() - ".unwrap_or_default".len();
            let receiver = &function_text[..receiver_end];

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("{}.unwrap()", receiver),
                "Replace .unwrap_or_default() with .unwrap()".to_string(),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if mutants.is_empty() {
            None
        } else {
            Some(mutants)
        }
    }

    /// Handle macro invocations (Some! is rare but could exist in custom code).
    fn handle_macro_invocation(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut usize,
    ) -> Option<Vec<Mutant>> {
        let mut mutants = Vec::new();

        // Get the macro name
        let macro_node = node.child_by_field_name("macro")?;
        let macro_text = macro_node.utf8_text(&result.source).ok()?;

        // Check for Some! macro (custom macros wrapping Some)
        if macro_text == "Some" {
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                "None",
                format!("Replace {} with None", original),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if mutants.is_empty() {
            None
        } else {
            Some(mutants)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use std::path::Path;

    fn parse_rust(code: &[u8]) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code, Language::Rust, Path::new("test.rs"))
            .unwrap()
    }

    #[test]
    fn test_option_operator_name() {
        let op = OptionOperator;
        assert_eq!(op.name(), "RustOption");
    }

    #[test]
    fn test_option_operator_description() {
        let op = OptionOperator;
        assert!(op.description().contains("Option"));
    }

    #[test]
    fn test_supports_only_rust() {
        let op = OptionOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::JavaScript));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::Java));
    }

    #[test]
    fn test_some_to_none() {
        let code = b"fn main() { let x = Some(42); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let some_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.starts_with("Some"))
            .collect();
        assert!(!some_mutants.is_empty(), "Should find Some mutations");
        assert!(
            some_mutants.iter().any(|m| m.replacement == "None"),
            "Should have None replacement"
        );
    }

    #[test]
    fn test_option_some_to_none() {
        let code = b"fn main() { let x = Option::Some(42); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let some_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("Some"))
            .collect();
        assert!(
            !some_mutants.is_empty(),
            "Should find Option::Some mutations"
        );
    }

    #[test]
    fn test_unwrap_to_expect() {
        let code = b"fn main() { let x = opt.unwrap(); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let unwrap_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("unwrap"))
            .collect();
        assert!(!unwrap_mutants.is_empty(), "Should find unwrap mutations");
        assert!(
            unwrap_mutants
                .iter()
                .any(|m| m.replacement.contains("expect")),
            "Should have expect replacement"
        );
    }

    #[test]
    fn test_unwrap_or_to_unwrap() {
        let code = b"fn main() { let x = opt.unwrap_or(0); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let unwrap_or_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("unwrap_or"))
            .collect();
        assert!(
            !unwrap_or_mutants.is_empty(),
            "Should find unwrap_or mutations"
        );
        assert!(
            unwrap_or_mutants
                .iter()
                .any(|m| m.replacement.ends_with(".unwrap()")),
            "Should have .unwrap() replacement"
        );
    }

    #[test]
    fn test_unwrap_or_else_to_unwrap() {
        let code = b"fn main() { let x = opt.unwrap_or_else(|| 0); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants
            .iter()
            .find(|m| m.original.contains("unwrap_or_else"));
        assert!(mutant.is_some(), "Should find unwrap_or_else mutation");
    }

    #[test]
    fn test_unwrap_or_default_to_unwrap() {
        let code = b"fn main() { let x = opt.unwrap_or_default(); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants
            .iter()
            .find(|m| m.original.contains("unwrap_or_default"));
        assert!(mutant.is_some(), "Should find unwrap_or_default mutation");
    }

    #[test]
    fn test_mutant_position() {
        let code = b"fn main() {\n    let x = Some(1);\n}";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            assert_eq!(mutant.line, 2, "Mutant should be on line 2");
        }
    }

    #[test]
    fn test_mutant_byte_range() {
        let code = b"let x = Some(42);";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            let (start, end) = mutant.byte_range;
            let slice = &code[start..end];
            assert!(
                std::str::from_utf8(slice).unwrap().contains("Some"),
                "Byte range should cover Some expression"
            );
        }
    }

    #[test]
    fn test_no_mutations_for_non_option_code() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(
            mutants.is_empty(),
            "Should not find mutations in non-Option code"
        );
    }

    #[test]
    fn test_multiple_mutations_in_function() {
        let code = b"fn main() { let a = Some(1); let b = opt.unwrap(); }";
        let result = parse_rust(code);
        let op = OptionOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.len() >= 2, "Should find at least 2 mutations");
    }
}
