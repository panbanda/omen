//! Rust Result mutation operator.
//!
//! This operator mutates Result-related code:
//! - `Result::Ok(x)` -> `Result::Err(Default::default())`
//! - `Ok(x)` -> `Err(Default::default())`
//! - `Err(e)` -> `Ok(Default::default())`
//! - `.unwrap()` -> `.expect("...")`

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::super::operator::MutationOperator;
use super::super::super::Mutant;
use super::super::walk_and_collect_mutants;

/// Rust Result mutation operator.
///
/// Mutates Result constructors and method calls to test error handling.
pub struct ResultOperator;

impl MutationOperator for ResultOperator {
    fn name(&self) -> &'static str {
        "RustResult"
    }

    fn description(&self) -> &'static str {
        "Rust Result Mutation - replaces Ok with Err and vice versa, mutates unwrap calls"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "call_expression" => self
                .handle_call_expression(&node, result, mutant_id_prefix, &mut counter)
                .unwrap_or_default(),
            "macro_invocation" => self
                .handle_macro_invocation(&node, result, mutant_id_prefix, &mut counter)
                .unwrap_or_default(),
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Rust)
    }
}

impl ResultOperator {
    /// Handle call expressions for Result mutations.
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

        // Check for Ok(x) or Result::Ok(x)
        if function_text == "Ok" || function_text.ends_with("::Ok") {
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
                "Err(Default::default())",
                format!("Replace {} with Err(Default::default())", original),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for Err(e) or Result::Err(e)
        if function_text == "Err" || function_text.ends_with("::Err") {
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
                "Ok(Default::default())",
                format!("Replace {} with Ok(Default::default())", original),
                (node.start_byte(), node.end_byte()),
            ));
        }

        // Check for method calls like .unwrap()
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

        if function_text.ends_with(".unwrap_err") {
            // .unwrap_err() -> .expect_err("mutated")
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

            let receiver_end = function_text.len() - ".unwrap_err".len();
            let receiver = &function_text[..receiver_end];

            mutants.push(Mutant::new(
                id,
                result.path.clone(),
                self.name(),
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                original,
                format!("{}.expect_err(\"mutated\")", receiver),
                "Replace .unwrap_err() with .expect_err(\"mutated\")".to_string(),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if function_text.ends_with(".unwrap_or") {
            // .unwrap_or(default) -> .unwrap()
            *counter += 1;
            let id = format!("{}-{}", prefix, counter);
            let start = function_node.start_position();
            let original = node.utf8_text(&result.source).ok()?;

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

    /// Handle macro invocations.
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

        // Check for Ok! or Err! macros (custom macros wrapping Ok/Err)
        if macro_text == "Ok" {
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
                "Err(Default::default())",
                format!("Replace {} with Err(Default::default())", original),
                (node.start_byte(), node.end_byte()),
            ));
        }

        if macro_text == "Err" {
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
                "Ok(Default::default())",
                format!("Replace {} with Ok(Default::default())", original),
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
    fn test_result_operator_name() {
        let op = ResultOperator;
        assert_eq!(op.name(), "RustResult");
    }

    #[test]
    fn test_result_operator_description() {
        let op = ResultOperator;
        assert!(op.description().contains("Result"));
    }

    #[test]
    fn test_supports_only_rust() {
        let op = ResultOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::JavaScript));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::Java));
    }

    #[test]
    fn test_ok_to_err() {
        let code = b"fn main() { let x = Ok(42); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ok_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.starts_with("Ok"))
            .collect();
        assert!(!ok_mutants.is_empty(), "Should find Ok mutations");
        assert!(
            ok_mutants.iter().any(|m| m.replacement.starts_with("Err")),
            "Should have Err replacement"
        );
    }

    #[test]
    fn test_result_ok_to_err() {
        let code = b"fn main() { let x = Result::Ok(42); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ok_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("Ok"))
            .collect();
        assert!(!ok_mutants.is_empty(), "Should find Result::Ok mutations");
    }

    #[test]
    fn test_err_to_ok() {
        let code = b"fn main() { let x = Err(\"error\"); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let err_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.starts_with("Err"))
            .collect();
        assert!(!err_mutants.is_empty(), "Should find Err mutations");
        assert!(
            err_mutants.iter().any(|m| m.replacement.starts_with("Ok")),
            "Should have Ok replacement"
        );
    }

    #[test]
    fn test_unwrap_to_expect() {
        let code = b"fn main() { let x = res.unwrap(); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let unwrap_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("unwrap()"))
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
    fn test_unwrap_err_to_expect_err() {
        let code = b"fn main() { let e = res.unwrap_err(); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants.iter().find(|m| m.original.contains("unwrap_err"));
        assert!(mutant.is_some(), "Should find unwrap_err mutation");
        assert!(
            mutant.unwrap().replacement.contains("expect_err"),
            "Should have expect_err replacement"
        );
    }

    #[test]
    fn test_unwrap_or_to_unwrap() {
        let code = b"fn main() { let x = res.unwrap_or(0); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants.iter().find(|m| m.original.contains("unwrap_or"));
        assert!(mutant.is_some(), "Should find unwrap_or mutation");
    }

    #[test]
    fn test_unwrap_or_else_to_unwrap() {
        let code = b"fn main() { let x = res.unwrap_or_else(|_| 0); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants
            .iter()
            .find(|m| m.original.contains("unwrap_or_else"));
        assert!(mutant.is_some(), "Should find unwrap_or_else mutation");
    }

    #[test]
    fn test_unwrap_or_default_to_unwrap() {
        let code = b"fn main() { let x = res.unwrap_or_default(); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        let mutant = mutants
            .iter()
            .find(|m| m.original.contains("unwrap_or_default"));
        assert!(mutant.is_some(), "Should find unwrap_or_default mutation");
    }

    #[test]
    fn test_mutant_position() {
        let code = b"fn main() {\n    let x = Ok(1);\n}";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            assert_eq!(mutant.line, 2, "Mutant should be on line 2");
        }
    }

    #[test]
    fn test_mutant_byte_range() {
        let code = b"let x = Ok(42);";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");

        if let Some(mutant) = mutants.first() {
            let (start, end) = mutant.byte_range;
            let slice = &code[start..end];
            assert!(
                std::str::from_utf8(slice).unwrap().contains("Ok"),
                "Byte range should cover Ok expression"
            );
        }
    }

    #[test]
    fn test_no_mutations_for_non_result_code() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(
            mutants.is_empty(),
            "Should not find mutations in non-Result code"
        );
    }

    #[test]
    fn test_multiple_mutations_in_function() {
        let code = b"fn main() { let a = Ok(1); let b = Err(\"e\"); let c = res.unwrap(); }";
        let result = parse_rust(code);
        let op = ResultOperator;

        let mutants = op.generate_mutants(&result, "test");
        assert!(mutants.len() >= 3, "Should find at least 3 mutations");
    }
}
