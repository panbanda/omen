//! Rust borrow mutation operator.
//!
//! This operator mutates borrow-related code:
//! - `&x` -> `&mut x`
//! - `&mut x` -> `&x`
//! - `.clone()` -> remove clone
//! - `.to_owned()` -> remove to_owned

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::super::operator::MutationOperator;
use super::super::super::Mutant;

/// Rust borrow mutation operator.
///
/// Mutates borrow expressions and clone/to_owned calls to test ownership handling.
pub struct BorrowOperator;

impl MutationOperator for BorrowOperator {
    fn name(&self) -> &'static str {
        "RustBorrow"
    }

    fn description(&self) -> &'static str {
        "Rust Borrow Mutation - toggles mutability and removes clone/to_owned calls"
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
                // Match reference expressions: &x, &mut x
                "reference_expression" => {
                    if let Some(mutant_list) = self.handle_reference_expression(
                        &node,
                        result,
                        mutant_id_prefix,
                        &mut counter,
                    ) {
                        mutants.extend(mutant_list);
                    }
                }
                // Match call expressions for .clone(), .to_owned()
                "call_expression" => {
                    if let Some(mutant_list) =
                        self.handle_call_expression(&node, result, mutant_id_prefix, &mut counter)
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

impl BorrowOperator {
    /// Handle reference expressions for borrow mutations.
    fn handle_reference_expression(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut usize,
    ) -> Option<Vec<Mutant>> {
        let mut mutants = Vec::new();
        let original = node.utf8_text(&result.source).ok()?;
        let start = node.start_position();

        // Check if this is a mutable reference
        let has_mutable = node
            .children(&mut node.walk())
            .any(|child| child.kind() == "mutable_specifier");

        // Get the value being referenced
        let value_node = node.child_by_field_name("value")?;
        let value_text = value_node.utf8_text(&result.source).ok()?;

        if has_mutable {
            // &mut x -> &x
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("&{}", value_text),
                format!("Replace &mut {} with &{}", value_text, value_text),
                (node.start_byte(), node.end_byte()),
            ));
        } else {
            // &x -> &mut x
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("&mut {}", value_text),
                format!("Replace &{} with &mut {}", value_text, value_text),
                (node.start_byte(), node.end_byte()),
            ));
        }

        Some(mutants)
    }

    /// Handle call expressions for clone/to_owned mutations.
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

        // Check for .clone() method
        if function_text.ends_with(".clone") {
            let receiver_end = function_text.len() - ".clone".len();
            let receiver = &function_text[..receiver_end];

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
                receiver.to_string(),
                format!("Remove .clone() from {}", receiver),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for .to_owned() method
        if function_text.ends_with(".to_owned") {
            let receiver_end = function_text.len() - ".to_owned".len();
            let receiver = &function_text[..receiver_end];

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
                receiver.to_string(),
                format!("Remove .to_owned() from {}", receiver),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for .to_string() method (similar to clone for strings)
        if function_text.ends_with(".to_string") {
            let receiver_end = function_text.len() - ".to_string".len();
            let receiver = &function_text[..receiver_end];

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
                receiver.to_string(),
                format!("Remove .to_string() from {}", receiver),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for .into() method (ownership transfer)
        if function_text.ends_with(".into") {
            let receiver_end = function_text.len() - ".into".len();
            let receiver = &function_text[..receiver_end];

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
                receiver.to_string(),
                format!("Remove .into() from {}", receiver),
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
    fn test_borrow_operator_name() {
        let op = BorrowOperator;
        assert_eq!(op.name(), "RustBorrow");
    }

    #[test]
    fn test_borrow_operator_description() {
        let op = BorrowOperator;
        assert!(op.description().contains("Borrow"));
    }

    #[test]
    fn test_supports_only_rust() {
        let op = BorrowOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::JavaScript));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::Java));
    }

    #[test]
    fn test_immutable_to_mutable_ref() {
        let code = b"fn main() { let x = &y; }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ref_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.starts_with('&') && !m.original.starts_with("&mut"))
            .collect();
        assert!(
            !ref_mutants.is_empty(),
            "Should find immutable ref mutations"
        );
        assert!(
            ref_mutants
                .iter()
                .any(|m| m.replacement.starts_with("&mut")),
            "Should have &mut replacement"
        );
    }

    #[test]
    fn test_mutable_to_immutable_ref() {
        let code = b"fn main() { let x = &mut y; }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ref_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("&mut"))
            .collect();
        assert!(!ref_mutants.is_empty(), "Should find mutable ref mutations");
        assert!(
            ref_mutants
                .iter()
                .any(|m| m.replacement.starts_with('&') && !m.replacement.contains("mut")),
            "Should have & replacement without mut"
        );
    }

    #[test]
    fn test_remove_clone() {
        let code = b"fn main() { let x = y.clone(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let clone_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("clone"))
            .collect();
        assert!(!clone_mutants.is_empty(), "Should find clone mutations");
        assert!(
            clone_mutants
                .iter()
                .any(|m| !m.replacement.contains("clone")),
            "Should remove clone"
        );
    }

    #[test]
    fn test_remove_to_owned() {
        let code = b"fn main() { let x = s.to_owned(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let to_owned_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("to_owned"))
            .collect();
        assert!(
            !to_owned_mutants.is_empty(),
            "Should find to_owned mutations"
        );
        assert!(
            to_owned_mutants
                .iter()
                .any(|m| !m.replacement.contains("to_owned")),
            "Should remove to_owned"
        );
    }

    #[test]
    fn test_remove_to_string() {
        let code = b"fn main() { let x = s.to_string(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants.iter().find(|m| m.original.contains("to_string"));
        assert!(mutant.is_some(), "Should find to_string mutation");
    }

    #[test]
    fn test_remove_into() {
        let code = b"fn main() { let x = s.into(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants.iter().find(|m| m.original.contains("into"));
        assert!(mutant.is_some(), "Should find into mutation");
    }

    #[test]
    fn test_mutant_position() {
        let code = b"fn main() {\n    let x = &y;\n}";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            assert_eq!(mutant.line, 2, "Mutant should be on line 2");
        }
    }

    #[test]
    fn test_mutant_byte_range() {
        let code = b"let x = &y;";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            let (start, end) = mutant.byte_range;
            let slice = &code[start..end];
            assert!(
                std::str::from_utf8(slice).unwrap().contains('&'),
                "Byte range should cover reference expression"
            );
        }
    }

    #[test]
    fn test_no_mutations_for_non_borrow_code() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(
            mutants.is_empty(),
            "Should not find mutations in non-borrow code"
        );
    }

    #[test]
    fn test_multiple_mutations_in_function() {
        let code = b"fn main() { let a = &x; let b = &mut y; let c = z.clone(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.len() >= 3, "Should find at least 3 mutations");
    }

    #[test]
    fn test_chained_method_calls() {
        let code = b"fn main() { let x = s.trim().to_owned(); }";
        let result = parse_rust(code);
        let op = BorrowOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(
            !mutants.is_empty(),
            "Should find mutations in chained calls"
        );
    }
}
