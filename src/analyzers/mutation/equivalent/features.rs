//! Feature extraction for mutant equivalence detection.
//!
//! Extracts features from mutants that can be used to determine if they
//! are likely equivalent (i.e., cannot be killed by any test).

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::Mutant;

/// Features extracted from a mutant for equivalence analysis.
#[derive(Debug, Clone, Default)]
pub struct MutantFeatures {
    /// The mutation operator type (e.g., "CRR", "ROR", "AOR").
    pub operator_type: String,
    /// Whether the mutant is in dead/unreachable code.
    pub in_dead_code: bool,
    /// Whether the mutant is in a logging/debug statement.
    pub in_logging: bool,
    /// Whether the mutation affects a return value.
    pub affects_return: bool,
    /// Change in code complexity (positive = more complex).
    pub complexity_delta: i32,
    /// Depth of the mutated node in the AST.
    pub ast_depth: u32,
    /// Number of sibling nodes at the same level.
    pub sibling_count: u32,
}

impl MutantFeatures {
    /// Extract features from a mutant using its parse context.
    pub fn extract(mutant: &Mutant, parse_result: &ParseResult) -> Self {
        let root = parse_result.root_node();

        // Find the node at the mutant's byte range
        let (start_byte, _end_byte) = mutant.byte_range;
        let node = root.descendant_for_byte_range(start_byte, start_byte + 1);

        let (ast_depth, sibling_count, in_dead_code, in_logging, affects_return) = match node {
            Some(n) => {
                let depth = calculate_depth(&n);
                let siblings = calculate_siblings(&n);
                let dead = is_in_dead_code(&n, parse_result.language);
                let logging =
                    is_in_logging_statement(&n, &parse_result.source, parse_result.language);
                let returns = affects_return_value(&n);
                (depth, siblings, dead, logging, returns)
            }
            None => (0, 0, false, false, false),
        };

        let complexity_delta = calculate_complexity_delta(&mutant.original, &mutant.replacement);

        Self {
            operator_type: mutant.operator.clone(),
            in_dead_code,
            in_logging,
            affects_return,
            complexity_delta,
            ast_depth,
            sibling_count,
        }
    }
}

/// Calculate the depth of a node in the AST.
fn calculate_depth(node: &tree_sitter::Node<'_>) -> u32 {
    let mut depth = 0u32;
    let mut current = *node;
    while let Some(parent) = current.parent() {
        depth += 1;
        current = parent;
    }
    depth
}

/// Calculate the number of siblings a node has.
fn calculate_siblings(node: &tree_sitter::Node<'_>) -> u32 {
    match node.parent() {
        Some(parent) => parent.child_count().saturating_sub(1) as u32,
        None => 0,
    }
}

/// Check if a node is in dead/unreachable code.
fn is_in_dead_code(node: &tree_sitter::Node<'_>, lang: Language) -> bool {
    let mut current = *node;

    while let Some(parent) = current.parent() {
        let parent_kind = parent.kind();

        // Check for return statement - code after return is dead
        if is_after_return(&current, &parent, lang) {
            return true;
        }

        // Check for dead branches (if false, while false, etc.)
        if is_in_dead_branch(&parent, lang) {
            return true;
        }

        // Check for unreachable code markers
        if is_unreachable_marker(parent_kind, lang) {
            return true;
        }

        current = parent;
    }

    false
}

/// Check if node is after a return statement in the same block.
fn is_after_return(
    node: &tree_sitter::Node<'_>,
    parent: &tree_sitter::Node<'_>,
    lang: Language,
) -> bool {
    let block_kinds = get_block_kinds(lang);
    if !block_kinds.contains(&parent.kind()) {
        return false;
    }

    let mut found_return = false;
    for child in parent.children(&mut parent.walk()) {
        if is_return_statement(child.kind(), lang) {
            found_return = true;
        } else if found_return
            && child.start_byte() <= node.start_byte()
            && node.start_byte() < child.end_byte()
        {
            return true;
        } else if found_return && child.start_byte() > node.start_byte() {
            // Node comes before any return statement
            return false;
        }
    }

    false
}

/// Check if parent is a branch that will never execute.
fn is_in_dead_branch(parent: &tree_sitter::Node<'_>, lang: Language) -> bool {
    let kind = parent.kind();

    // Check for `if false` or `while false` patterns
    if is_conditional_kind(kind, lang) {
        if let Some(condition) = parent.child_by_field_name("condition") {
            let condition_kind = condition.kind();
            // Literal false
            if condition_kind == "false" || condition_kind == "False" {
                return true;
            }
        }
    }

    false
}

fn is_conditional_kind(kind: &str, _lang: Language) -> bool {
    matches!(
        kind,
        "if_statement" | "if_expression" | "while_statement" | "while_expression"
    )
}

fn is_return_statement(kind: &str, _lang: Language) -> bool {
    matches!(kind, "return_statement" | "return_expression" | "return")
}

fn get_block_kinds(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["block", "match_arm"],
        Language::Go => &["block"],
        Language::Python => &["block", "suite"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["statement_block"]
        }
        Language::Java | Language::CSharp => &["block"],
        Language::C | Language::Cpp => &["compound_statement"],
        Language::Ruby => &["do_block", "block"],
        Language::Php => &["compound_statement"],
        Language::Bash => &["compound_statement"],
    }
}

fn is_unreachable_marker(kind: &str, _lang: Language) -> bool {
    matches!(kind, "unreachable" | "panic" | "todo")
}

/// Check if a node is inside a logging statement.
fn is_in_logging_statement(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> bool {
    let mut current = *node;

    while let Some(parent) = current.parent() {
        if is_logging_call(&parent, source, lang) {
            return true;
        }
        current = parent;
    }

    false
}

/// Check if a node is a logging function call.
fn is_logging_call(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> bool {
    let call_kinds = get_call_kinds(lang);
    if !call_kinds.contains(&node.kind()) {
        return false;
    }

    // Get the function name being called
    let callee = get_callee_name(node, source, lang);
    if let Some(name) = callee {
        return is_logging_function(&name, lang);
    }

    false
}

fn get_call_kinds(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["call_expression", "macro_invocation"],
        Language::Go => &["call_expression"],
        Language::Python => &["call"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["call_expression"]
        }
        Language::Java | Language::CSharp => &["method_invocation", "invocation_expression"],
        Language::C | Language::Cpp => &["call_expression"],
        Language::Ruby => &["call", "method_call"],
        Language::Php => &["function_call_expression", "method_call_expression"],
        Language::Bash => &["command"],
    }
}

fn get_callee_name(node: &tree_sitter::Node<'_>, source: &[u8], _lang: Language) -> Option<String> {
    // For Rust macros, get the macro name
    if node.kind() == "macro_invocation" {
        if let Some(macro_node) = node.child_by_field_name("macro") {
            return macro_node.utf8_text(source).ok().map(|s| s.to_string());
        }
    }

    // For function calls, try to get the function name
    if let Some(fn_node) = node.child_by_field_name("function") {
        // Handle member expressions (obj.method)
        if fn_node.kind() == "member_expression" || fn_node.kind() == "field_expression" {
            if let Some(prop) = fn_node.child_by_field_name("property") {
                return prop.utf8_text(source).ok().map(|s| s.to_string());
            }
            if let Some(field) = fn_node.child_by_field_name("field") {
                return field.utf8_text(source).ok().map(|s| s.to_string());
            }
        }
        return fn_node.utf8_text(source).ok().map(|s| s.to_string());
    }

    // Try first child for some languages
    if node.child_count() > 0 {
        let first = node.child(0)?;
        if first.kind() == "identifier" || first.kind() == "scoped_identifier" {
            return first.utf8_text(source).ok().map(|s| s.to_string());
        }
    }

    // For Python calls
    if let Some(fn_node) = node.child_by_field_name("function") {
        return fn_node.utf8_text(source).ok().map(|s| s.to_string());
    }

    None
}

/// Check if a function name is a logging function.
fn is_logging_function(name: &str, lang: Language) -> bool {
    let lower_name = name.to_lowercase();

    // Universal patterns
    if lower_name.contains("log")
        || lower_name.contains("debug")
        || lower_name.contains("trace")
        || lower_name.contains("warn")
        || lower_name.contains("error")
        || lower_name.contains("info")
    {
        return true;
    }

    // Language-specific patterns
    match lang {
        Language::Rust => {
            matches!(
                name,
                "println" | "print" | "eprintln" | "eprint" | "dbg" | "tracing"
            )
        }
        Language::Go => matches!(name, "Println" | "Printf" | "Print" | "Fatalf" | "Fatal"),
        Language::Python => matches!(name, "print" | "pprint" | "logging"),
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            name.starts_with("console.")
                || matches!(name, "console" | "log" | "warn" | "error" | "info")
        }
        Language::Java => matches!(name, "println" | "print" | "printf"),
        Language::Ruby => matches!(name, "puts" | "print" | "p" | "pp"),
        Language::Php => matches!(name, "echo" | "print_r" | "var_dump"),
        _ => false,
    }
}

/// Check if mutation affects a return value.
fn affects_return_value(node: &tree_sitter::Node<'_>) -> bool {
    let mut current = *node;

    while let Some(parent) = current.parent() {
        let kind = parent.kind();
        if matches!(kind, "return_statement" | "return_expression" | "return") {
            return true;
        }
        // In Rust, the last expression in a block is implicitly returned
        if kind == "block" {
            if let Some(last_child) = parent.child(parent.child_count().saturating_sub(2)) {
                if last_child.start_byte() <= current.start_byte()
                    && current.end_byte() <= last_child.end_byte()
                {
                    // Check if this block is a function body
                    if let Some(grandparent) = parent.parent() {
                        if matches!(
                            grandparent.kind(),
                            "function_item" | "function_definition" | "closure_expression"
                        ) {
                            return true;
                        }
                    }
                }
            }
        }
        current = parent;
    }

    false
}

/// Calculate complexity delta between original and replacement.
fn calculate_complexity_delta(original: &str, replacement: &str) -> i32 {
    let orig_complexity = estimate_complexity(original);
    let repl_complexity = estimate_complexity(replacement);
    repl_complexity - orig_complexity
}

/// Estimate complexity of a code snippet.
fn estimate_complexity(code: &str) -> i32 {
    let mut complexity = 0i32;

    // More operators = more complex
    complexity += code.matches("&&").count() as i32;
    complexity += code.matches("||").count() as i32;
    complexity += code.matches("?").count() as i32;

    // Nesting indicators
    complexity += code.matches('(').count() as i32;

    complexity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use std::path::Path;

    fn parse_rust(code: &str) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code.as_bytes(), Language::Rust, Path::new("test.rs"))
            .unwrap()
    }

    fn parse_js(code: &str) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code.as_bytes(), Language::JavaScript, Path::new("test.js"))
            .unwrap()
    }

    fn parse_python(code: &str) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code.as_bytes(), Language::Python, Path::new("test.py"))
            .unwrap()
    }

    fn create_mutant(
        operator: &str,
        original: &str,
        replacement: &str,
        byte_range: (usize, usize),
    ) -> Mutant {
        Mutant::new(
            "test-1",
            "test.rs",
            operator,
            1,
            1,
            original,
            replacement,
            "test mutation",
            byte_range,
        )
    }

    #[test]
    fn test_extract_basic_features() {
        let code = "fn main() { let x = 42; }";
        let result = parse_rust(code);
        let mutant = create_mutant("CRR", "42", "0", (20, 22));

        let features = MutantFeatures::extract(&mutant, &result);

        assert_eq!(features.operator_type, "CRR");
        assert!(!features.in_dead_code);
        assert!(!features.in_logging);
    }

    #[test]
    fn test_extract_features_in_logging_rust() {
        let code = r#"fn main() { println!("value: {}", 42); }"#;
        let result = parse_rust(code);
        // The 42 is inside the println! macro
        let mutant = create_mutant("CRR", "42", "0", (34, 36));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(features.in_logging);
    }

    #[test]
    fn test_extract_features_in_logging_js() {
        let code = "function foo() { console.log(42); }";
        let result = parse_js(code);
        let mutant = create_mutant("CRR", "42", "0", (29, 31));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(features.in_logging);
    }

    #[test]
    fn test_extract_features_in_logging_python() {
        let code = "def foo():\n    print(42)";
        let result = parse_python(code);
        let mutant = create_mutant("CRR", "42", "0", (21, 23));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(features.in_logging);
    }

    #[test]
    fn test_extract_features_not_in_logging() {
        let code = "fn main() { let x = calculate(42); }";
        let result = parse_rust(code);
        let mutant = create_mutant("CRR", "42", "0", (30, 32));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(!features.in_logging);
    }

    #[test]
    fn test_extract_features_affects_return() {
        let code = "fn get_value() -> i32 { 42 }";
        let result = parse_rust(code);
        let mutant = create_mutant("CRR", "42", "0", (24, 26));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(features.affects_return);
    }

    #[test]
    fn test_extract_features_explicit_return() {
        let code = "fn get_value() -> i32 { return 42; }";
        let result = parse_rust(code);
        let mutant = create_mutant("CRR", "42", "0", (31, 33));

        let features = MutantFeatures::extract(&mutant, &result);

        assert!(features.affects_return);
    }

    #[test]
    fn test_complexity_delta_simple() {
        let mutant = create_mutant("AOR", "+", "-", (10, 11));
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let result = parse_rust(code);

        let features = MutantFeatures::extract(&mutant, &result);

        // Simple operator swap has no complexity change
        assert_eq!(features.complexity_delta, 0);
    }

    #[test]
    fn test_complexity_delta_increase() {
        // Replacing simple with complex
        let delta = calculate_complexity_delta("x", "x && y");
        assert!(delta > 0);
    }

    #[test]
    fn test_complexity_delta_decrease() {
        // Replacing complex with simple
        let delta = calculate_complexity_delta("x && y || z", "x");
        assert!(delta < 0);
    }

    #[test]
    fn test_ast_depth_calculation() {
        let code = "fn main() { if true { let x = 42; } }";
        let result = parse_rust(code);
        // 42 is nested inside: source_file > function_item > block > if_expression > block > let_declaration > integer_literal
        let mutant = create_mutant("CRR", "42", "0", (30, 32));

        let features = MutantFeatures::extract(&mutant, &result);

        // Should have significant depth due to nesting
        assert!(features.ast_depth > 3);
    }

    #[test]
    fn test_sibling_count() {
        let code = "fn main() { let a = 1; let b = 2; let c = 3; }";
        let result = parse_rust(code);
        let mutant = create_mutant("CRR", "2", "0", (31, 32));

        let features = MutantFeatures::extract(&mutant, &result);

        // The integer literal has siblings in its parent context
        assert!(features.sibling_count > 0);
    }

    #[test]
    fn test_is_logging_function_rust() {
        assert!(is_logging_function("println", Language::Rust));
        assert!(is_logging_function("eprintln", Language::Rust));
        assert!(is_logging_function("dbg", Language::Rust));
        assert!(!is_logging_function("calculate", Language::Rust));
    }

    #[test]
    fn test_is_logging_function_js() {
        assert!(is_logging_function("console.log", Language::JavaScript));
        assert!(is_logging_function("console", Language::JavaScript));
        assert!(is_logging_function("log", Language::JavaScript));
        assert!(!is_logging_function("fetch", Language::JavaScript));
    }

    #[test]
    fn test_is_logging_function_python() {
        assert!(is_logging_function("print", Language::Python));
        assert!(is_logging_function("logging", Language::Python));
        assert!(!is_logging_function("calculate", Language::Python));
    }

    #[test]
    fn test_default_features() {
        let features = MutantFeatures::default();

        assert!(features.operator_type.is_empty());
        assert!(!features.in_dead_code);
        assert!(!features.in_logging);
        assert!(!features.affects_return);
        assert_eq!(features.complexity_delta, 0);
        assert_eq!(features.ast_depth, 0);
        assert_eq!(features.sibling_count, 0);
    }
}
