//! Built-in mutation operators.
//!
//! Operators are named using standard mutation testing conventions:
//! - CRR: Constant Replacement (LiteralOperator)
//! - ROR: Relational Operator Replacement
//! - AOR: Arithmetic Operator Replacement
//! - COR: Conditional Operator Replacement
//! - UOR: Unary Operator Replacement
//! - SDL: Statement Deletion
//! - RVR: Return Value Replacement
//! - BVO: Boundary Value Operator
//! - BOR: Bitwise Operator Replacement
//! - ASR: Assignment Operator Replacement

use crate::parser::ParseResult;

use super::Mutant;

mod arithmetic;
mod assignment;
mod bitwise;
mod boundary;
mod conditional;
pub mod go;
mod literal;
pub mod python;
mod relational;
mod return_value;
pub mod ruby;
pub mod rust;
mod statement;
pub mod typescript;
mod unary;

pub use arithmetic::ArithmeticOperator;
pub use assignment::AssignmentOperator;
pub use bitwise::BitwiseOperator;
pub use boundary::BoundaryOperator;
pub use conditional::ConditionalOperator;
pub use go::{GoErrorOperator, GoNilOperator};
pub use literal::LiteralOperator;
pub use python::{PythonComprehensionOperator, PythonIdentityOperator};
pub use relational::RelationalOperator;
pub use return_value::ReturnValueOperator;
pub use ruby::{RubyNilOperator, RubySymbolOperator};
pub use rust::{BorrowOperator, OptionOperator, ResultOperator};
pub use statement::StatementOperator;
pub use typescript::{TypeScriptEqualityOperator, TypeScriptOptionalOperator};
pub use unary::UnaryOperator;

use super::operator::OperatorRegistry;

/// Build a list of replacement operators by returning all `ops` except `current`.
pub fn replacements_excluding_self(ops: &[&str], current: &str) -> Vec<String> {
    ops.iter()
        .filter(|&&o| o != current)
        .map(|&o| o.to_string())
        .collect()
}

/// Create a single `Mutant` from a tree-sitter node.
///
/// Centralizes the common pattern of extracting position and byte range from a node
/// and constructing a `Mutant`. Used by operators that build mutants from individual
/// AST nodes (literal, return value, statement, boundary, etc.).
pub fn mutant_from_node(
    id: String,
    path: std::path::PathBuf,
    operator_name: &'static str,
    node: &tree_sitter::Node,
    original: impl Into<String>,
    replacement: impl Into<String>,
    description: impl Into<String>,
) -> Mutant {
    let start = node.start_position();
    Mutant::new(
        id,
        path,
        operator_name,
        (start.row + 1) as u32,
        (start.column + 1) as u32,
        original,
        replacement,
        description,
        (node.start_byte(), node.end_byte()),
    )
}

/// Helper function for generating mutants from binary operators.
///
/// All binary operator mutators (AOR, ROR, COR, BOR, ASR) share the same tree traversal
/// pattern. This function abstracts that pattern so individual operators only provide:
/// - `node_types`: Which AST node types contain binary operations (varies by language)
/// - `is_target_operator`: Predicate to identify target operator tokens
/// - `get_replacements`: Function to get replacement operators
pub fn generate_binary_operator_mutants<F, G>(
    result: &ParseResult,
    mutant_id_prefix: &str,
    operator_name: &'static str,
    node_types: &[&str],
    is_target_operator: F,
    get_replacements: G,
) -> Vec<Mutant>
where
    F: Fn(&str) -> bool,
    G: Fn(&str) -> Vec<String>,
{
    let mut mutants = Vec::new();
    let root = result.root_node();

    let mut counter = 0;
    let mut cursor = root.walk();

    loop {
        let node = cursor.node();
        let kind = node.kind();

        if node_types.contains(&kind) {
            // Find the operator child
            for child in node.children(&mut node.walk()) {
                if is_target_operator(child.kind()) {
                    if let Ok(op_text) = child.utf8_text(&result.source) {
                        let replacements = get_replacements(op_text);
                        for replacement in replacements {
                            counter += 1;
                            let id = format!("{}-{}", mutant_id_prefix, counter);
                            let start = child.start_position();
                            mutants.push(Mutant::new(
                                id,
                                result.path.clone(),
                                operator_name,
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
            }
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

/// Walk the AST and collect mutants by calling a visitor on each node.
///
/// This is the general-purpose tree traversal used by language-specific operators
/// (Go error/nil, Rust Option/Result, Python identity, etc.) that match on
/// specific node kinds and delegate to handler methods.
pub fn walk_and_collect_mutants<F>(result: &ParseResult, mut visitor: F) -> Vec<Mutant>
where
    F: FnMut(tree_sitter::Node<'_>) -> Vec<Mutant>,
{
    let mut mutants = Vec::new();
    let root = result.root_node();
    let mut cursor = root.walk();

    loop {
        let node = cursor.node();
        mutants.extend(visitor(node));

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

/// Create a registry with default operators (CRR, ROR, AOR).
///
/// These are the most commonly used operators that work across all languages
/// and provide good mutation coverage with reasonable execution time.
pub fn default_registry() -> OperatorRegistry {
    let mut registry = OperatorRegistry::new();
    registry.register(Box::new(LiteralOperator));
    registry.register(Box::new(RelationalOperator));
    registry.register(Box::new(ArithmeticOperator));
    registry
}

/// Create a registry with all available operators.
///
/// Includes language-specific operators and all mutation categories.
/// Use this for thorough mutation testing when execution time is not a concern.
pub fn full_registry() -> OperatorRegistry {
    let mut registry = default_registry();
    registry.register(Box::new(ConditionalOperator));
    registry.register(Box::new(UnaryOperator));
    registry.register(Box::new(BoundaryOperator));
    registry.register(Box::new(BitwiseOperator));
    registry.register(Box::new(AssignmentOperator));
    registry.register(Box::new(StatementOperator));
    registry.register(Box::new(ReturnValueOperator));
    // Language-specific operators
    rust::register_rust_operators(&mut registry);
    register_go_operators(&mut registry);
    register_typescript_operators(&mut registry);
    register_python_operators(&mut registry);
    register_ruby_operators(&mut registry);
    registry
}

/// Register Go-specific operators.
pub fn register_go_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(GoErrorOperator));
    registry.register(Box::new(GoNilOperator));
}

/// Register TypeScript/JavaScript-specific operators.
pub fn register_typescript_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(TypeScriptEqualityOperator));
    registry.register(Box::new(TypeScriptOptionalOperator));
}

/// Register Python-specific operators.
pub fn register_python_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(PythonIdentityOperator));
    registry.register(Box::new(PythonComprehensionOperator));
}

/// Register Ruby-specific operators.
pub fn register_ruby_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(RubyNilOperator));
    registry.register(Box::new(RubySymbolOperator));
}

/// Create a registry optimized for fast execution.
///
/// Uses only the most effective operators and excludes those that tend
/// to produce many equivalent mutants.
pub fn fast_registry() -> OperatorRegistry {
    let mut registry = OperatorRegistry::new();
    registry.register(Box::new(RelationalOperator));
    registry.register(Box::new(ArithmeticOperator));
    // Literal operator excluded in fast mode as it tends to produce
    // more equivalent mutants
    registry
}

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::core::Language;
    use crate::parser::{ParseResult, Parser};
    use std::path::Path;

    /// Parse source code for testing. Accepts any language.
    pub fn parse_code(code: &[u8], lang: Language) -> ParseResult {
        let parser = Parser::new();
        parser.parse(code, lang, Path::new("test.rs")).unwrap()
    }

    pub fn parse_rust(code: &[u8]) -> ParseResult {
        parse_code(code, Language::Rust)
    }

    pub fn parse_go(code: &[u8]) -> ParseResult {
        parse_code(code, Language::Go)
    }

    pub fn parse_py(code: &[u8]) -> ParseResult {
        parse_code(code, Language::Python)
    }

    pub fn parse_ts(code: &[u8]) -> ParseResult {
        parse_code(code, Language::TypeScript)
    }

    pub fn parse_js(code: &[u8]) -> ParseResult {
        parse_code(code, Language::JavaScript)
    }

    pub fn parse_rb(code: &[u8]) -> ParseResult {
        parse_code(code, Language::Ruby)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language;

    use super::test_utils::parse_code;

    #[test]
    fn test_generate_binary_operator_mutants_basic() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);

        let mutants = generate_binary_operator_mutants(
            &result,
            "test",
            "TEST",
            &["binary_expression"],
            |kind| kind == "+",
            |_op| vec!["-".to_string(), "*".to_string()],
        );

        assert_eq!(mutants.len(), 2);
        assert!(mutants.iter().all(|m| m.operator == "TEST"));
        assert!(mutants.iter().all(|m| m.original == "+"));
        let replacements: Vec<_> = mutants.iter().map(|m| m.replacement.as_str()).collect();
        assert!(replacements.contains(&"-"));
        assert!(replacements.contains(&"*"));
    }

    #[test]
    fn test_generate_binary_operator_mutants_no_matches() {
        let code = b"fn empty() {}";
        let result = parse_code(code, Language::Rust);

        let mutants = generate_binary_operator_mutants(
            &result,
            "test",
            "TEST",
            &["binary_expression"],
            |kind| kind == "+",
            |_op| vec!["-".to_string()],
        );

        assert!(mutants.is_empty());
    }

    #[test]
    fn test_generate_binary_operator_mutants_unique_ids() {
        let code = b"fn calc(a: i32, b: i32, c: i32) -> i32 { a + b + c }";
        let result = parse_code(code, Language::Rust);

        let mutants = generate_binary_operator_mutants(
            &result,
            "test",
            "TEST",
            &["binary_expression"],
            |kind| kind == "+",
            |_op| vec!["-".to_string()],
        );

        let mut ids: Vec<_> = mutants.iter().map(|m| &m.id).collect();
        let len_before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), len_before, "IDs should be unique");
    }

    #[test]
    fn test_generate_binary_operator_mutants_byte_range() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);

        let mutants = generate_binary_operator_mutants(
            &result,
            "test",
            "TEST",
            &["binary_expression"],
            |kind| kind == "+",
            |_op| vec!["-".to_string()],
        );

        assert!(!mutants.is_empty());
        let mutant = &mutants[0];
        let (start, end) = mutant.byte_range;
        assert_eq!(&code[start..end], b"+");
    }

    #[test]
    fn test_default_registry_has_three_operators() {
        let registry = default_registry();
        assert_eq!(registry.operators().len(), 3);
    }

    #[test]
    fn test_default_registry_operator_names() {
        let registry = default_registry();
        let names: Vec<&str> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"CRR"));
        assert!(names.contains(&"ROR"));
        assert!(names.contains(&"AOR"));
    }

    #[test]
    fn test_full_registry_includes_default() {
        let full = full_registry();
        let default = default_registry();
        assert!(full.operators().len() >= default.operators().len());
    }

    #[test]
    fn test_fast_registry_is_subset() {
        let fast = fast_registry();
        let default = default_registry();
        assert!(fast.operators().len() <= default.operators().len());
    }

    #[test]
    fn test_fast_registry_has_ror_and_aor() {
        let registry = fast_registry();
        let names: Vec<&str> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"ROR"));
        assert!(names.contains(&"AOR"));
    }

    #[test]
    fn test_walk_and_collect_mutants_basic() {
        let code = b"fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_code(code, Language::Rust);

        let mutants = walk_and_collect_mutants(&result, |node| {
            let mut found = Vec::new();
            if node.kind() == "binary_expression" {
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "+" {
                        if let Ok(op_text) = child.utf8_text(&result.source) {
                            let start = child.start_position();
                            found.push(Mutant::new(
                                "test-1",
                                result.path.clone(),
                                "TEST",
                                (start.row + 1) as u32,
                                (start.column + 1) as u32,
                                op_text,
                                "-",
                                "Replace + with -",
                                (child.start_byte(), child.end_byte()),
                            ));
                        }
                    }
                }
            }
            found
        });

        assert_eq!(mutants.len(), 1);
        assert_eq!(mutants[0].original, "+");
        assert_eq!(mutants[0].replacement, "-");
    }

    #[test]
    fn test_walk_and_collect_mutants_empty() {
        let code = b"fn empty() {}";
        let result = parse_code(code, Language::Rust);

        let mutants = walk_and_collect_mutants(&result, |_node| Vec::new());
        assert!(mutants.is_empty());
    }
}
