//! SDL (Statement Deletion) mutation operator.
//!
//! This operator deletes or skips statements:
//! - Delete entire statements (assignments, function calls, expressions)
//! - Skip control flow statements (if, while, for bodies)
//! - Replace statement with empty block or comment equivalent
//!
//! Kill probability estimate: ~65-75%
//! Statement deletion mutations are typically well-caught by tests because
//! removing a statement often causes observable changes in program behavior.
//! However, dead code and logging statements may survive.

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::walk_and_collect_mutants;

/// SDL (Statement Deletion) operator.
///
/// Generates mutations by deleting statements or replacing them with no-ops.
pub struct StatementOperator;

impl MutationOperator for StatementOperator {
    fn name(&self) -> &'static str {
        "SDL"
    }

    fn description(&self) -> &'static str {
        "Statement Deletion - removes or skips statements"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let statement_types = get_statement_node_types(result.language);
        let control_flow_types = get_control_flow_node_types(result.language);
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            let kind = node.kind();
            let mut mutants = Vec::new();

            if statement_types.contains(&kind) {
                if let Ok(text) = node.utf8_text(&result.source) {
                    if !text.trim().is_empty() && is_deletable_statement(text, result.language) {
                        counter += 1;
                        let id = format!("{}-{}", mutant_id_prefix, counter);
                        let start = node.start_position();
                        let replacement = get_empty_replacement(result.language);

                        mutants.push(Mutant::new(
                            id,
                            result.path.clone(),
                            self.name(),
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            text,
                            replacement.clone(),
                            format!("Delete statement: {}", truncate_description(text)),
                            (node.start_byte(), node.end_byte()),
                        ));
                    }
                }
            }

            if control_flow_types.contains(&kind) {
                if let Some(body_node) = find_control_flow_body(&node, result.language) {
                    if let Ok(body_text) = body_node.utf8_text(&result.source) {
                        if !body_text.trim().is_empty() {
                            counter += 1;
                            let id = format!("{}-{}", mutant_id_prefix, counter);
                            let start = body_node.start_position();
                            let replacement = get_empty_block_replacement(result.language);

                            mutants.push(Mutant::new(
                                id,
                                result.path.clone(),
                                self.name(),
                                (start.row + 1) as u32,
                                (start.column + 1) as u32,
                                body_text,
                                replacement.clone(),
                                format!("Skip {} body", kind),
                                (body_node.start_byte(), body_node.end_byte()),
                            ));
                        }
                    }
                }
            }

            mutants
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Get node types that represent deletable statements.
fn get_statement_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &[
            "expression_statement",
            "let_declaration",
            "assignment_expression",
        ],
        Language::Go => &[
            "expression_statement",
            "short_var_declaration",
            "assignment_statement",
            "inc_statement",
            "dec_statement",
        ],
        Language::Python => &["expression_statement", "assignment", "augmented_assignment"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &[
            "expression_statement",
            "variable_declaration",
            "lexical_declaration",
            "assignment_expression",
        ],
        Language::Java => &[
            "expression_statement",
            "local_variable_declaration",
            "assignment_expression",
        ],
        Language::CSharp => &[
            "expression_statement",
            "local_declaration_statement",
            "assignment_expression",
        ],
        Language::C | Language::Cpp => &[
            "expression_statement",
            "declaration",
            "assignment_expression",
        ],
        Language::Ruby => &["expression_statement", "assignment"],
        Language::Php => &["expression_statement", "assignment_expression"],
        Language::Bash => &["command", "variable_assignment"],
    }
}

/// Get node types for control flow statements.
fn get_control_flow_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &[
            "if_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
        ],
        Language::Go => &["if_statement", "for_statement"],
        Language::Python => &["if_statement", "while_statement", "for_statement"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "for_in_statement",
        ],
        Language::Java | Language::CSharp => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "enhanced_for_statement",
        ],
        Language::C | Language::Cpp => &["if_statement", "while_statement", "for_statement"],
        Language::Ruby => &["if", "while", "for"],
        Language::Php => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "foreach_statement",
        ],
        Language::Bash => &["if_statement", "while_statement", "for_statement"],
    }
}

/// Find the body node of a control flow statement.
fn find_control_flow_body<'a>(
    node: &tree_sitter::Node<'a>,
    lang: Language,
) -> Option<tree_sitter::Node<'a>> {
    let body_kinds: &[&str] = match lang {
        Language::Rust => &["block"] as &[&str],
        Language::Go => &["block"] as &[&str],
        Language::Python => &["block"] as &[&str],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["statement_block"] as &[&str]
        }
        Language::Java | Language::CSharp => &["block"] as &[&str],
        Language::C | Language::Cpp => &["compound_statement"] as &[&str],
        Language::Ruby => &["then", "do"] as &[&str],
        Language::Php => &["compound_statement"] as &[&str],
        Language::Bash => &["compound_statement"] as &[&str],
    };

    node.children(&mut node.walk())
        .find(|child| body_kinds.contains(&child.kind()))
}

/// Check if a statement is deletable (not a control structure itself).
fn is_deletable_statement(text: &str, lang: Language) -> bool {
    let text = text.trim();

    // Skip empty statements
    if text.is_empty() || text == ";" || text == "{}" {
        return false;
    }

    // Skip single-line comments
    if text.starts_with("//") || text.starts_with("#") {
        return false;
    }

    // Skip statements that are just blocks
    if text.starts_with('{') && text.ends_with('}') && text.len() < 5 {
        return false;
    }

    // Language-specific filters
    match lang {
        Language::Python => {
            // Skip pass statements and docstrings
            !text.starts_with("pass") && !text.starts_with("\"\"\"") && !text.starts_with("'''")
        }
        Language::Rust => {
            // Skip unsafe blocks and attribute macros by themselves
            !text.starts_with("unsafe") && !text.starts_with("#[")
        }
        _ => true,
    }
}

/// Get the empty replacement for a statement (language-specific).
fn get_empty_replacement(lang: Language) -> String {
    match lang {
        Language::Python => "pass".to_string(),
        Language::Rust => "{}".to_string(),
        Language::Ruby => "nil".to_string(),
        Language::Bash => ":".to_string(),
        _ => ";".to_string(),
    }
}

/// Get the empty block replacement for control flow bodies.
fn get_empty_block_replacement(lang: Language) -> String {
    match lang {
        Language::Python => "pass".to_string(),
        Language::Rust => "{ }".to_string(),
        Language::Go => "{ }".to_string(),
        Language::Ruby => "end".to_string(),
        _ => "{ }".to_string(),
    }
}

/// Truncate a description for display.
fn truncate_description(text: &str) -> String {
    let text = text.trim().replace('\n', " ");
    if text.len() > 40 {
        format!("{}...", &text[..37])
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_statement_operator_name() {
        let op = StatementOperator;
        assert_eq!(op.name(), "SDL");
    }

    #[test]
    fn test_statement_operator_description() {
        let op = StatementOperator;
        assert!(op.description().contains("Statement Deletion"));
    }

    #[test]
    fn test_statement_operator_supports_all_languages() {
        let op = StatementOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::JavaScript));
        assert!(op.supports_language(Language::Java));
        assert!(op.supports_language(Language::CSharp));
        assert!(op.supports_language(Language::C));
        assert!(op.supports_language(Language::Cpp));
        assert!(op.supports_language(Language::Ruby));
        assert!(op.supports_language(Language::Php));
    }

    #[test]
    fn test_generate_mutants_rust_expression_statement() {
        let code = b"fn main() { println!(\"hello\"); }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find the println statement
        assert!(!mutants.is_empty());
        assert!(mutants.iter().any(|m| m.original.contains("println")));
    }

    #[test]
    fn test_generate_mutants_rust_let_declaration() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find the let declaration
        let let_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("let"))
            .collect();
        assert!(!let_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_rust_if_body() {
        let code = b"fn main() { if true { do_something(); } }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutation for the if body
        let body_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("Skip if"))
            .collect();
        assert!(!body_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_go_assignment() {
        let code = b"package main\n\nfunc main() { x := 10\n x = 20 }";
        let result = parse_code(code, Language::Go);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find assignments
        assert!(!mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_python_assignment() {
        let code = b"x = 42\ny = x + 1";
        let result = parse_code(code, Language::Python);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find assignments, replacement should be 'pass'
        assert!(!mutants.is_empty());
        assert!(mutants.iter().all(|m| m.replacement == "pass"));
    }

    #[test]
    fn test_generate_mutants_js_expression() {
        let code = b"function foo() { console.log('test'); }";
        let result = parse_code(code, Language::JavaScript);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find the console.log statement
        assert!(!mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_java_statement() {
        let code = b"class Foo { void bar() { System.out.println(\"test\"); } }";
        let result = parse_code(code, Language::Java);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find the println statement
        let println_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("System.out"))
            .collect();
        assert!(!println_mutants.is_empty());
    }

    #[test]
    fn test_is_deletable_statement_filters_empty() {
        assert!(!is_deletable_statement("", Language::Rust));
        assert!(!is_deletable_statement(";", Language::Rust));
        assert!(!is_deletable_statement("{}", Language::Rust));
    }

    #[test]
    fn test_is_deletable_statement_filters_comments() {
        assert!(!is_deletable_statement("// comment", Language::Rust));
        assert!(!is_deletable_statement("# comment", Language::Python));
    }

    #[test]
    fn test_is_deletable_statement_filters_pass() {
        assert!(!is_deletable_statement("pass", Language::Python));
    }

    #[test]
    fn test_get_empty_replacement_by_language() {
        assert_eq!(get_empty_replacement(Language::Python), "pass");
        assert_eq!(get_empty_replacement(Language::Rust), "{}");
        assert_eq!(get_empty_replacement(Language::JavaScript), ";");
        assert_eq!(get_empty_replacement(Language::Ruby), "nil");
        assert_eq!(get_empty_replacement(Language::Bash), ":");
    }

    #[test]
    fn test_get_empty_block_replacement_by_language() {
        assert_eq!(get_empty_block_replacement(Language::Python), "pass");
        assert_eq!(get_empty_block_replacement(Language::Rust), "{ }");
        assert_eq!(get_empty_block_replacement(Language::Go), "{ }");
    }

    #[test]
    fn test_truncate_description_short() {
        let result = truncate_description("short text");
        assert_eq!(result, "short text");
    }

    #[test]
    fn test_truncate_description_long() {
        let long_text =
            "this is a very long statement that should be truncated for display purposes";
        let result = truncate_description(long_text);
        assert!(result.len() <= 43);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_mutant_byte_range_correct() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_mutant_positions_are_one_indexed() {
        let code = b"fn main() {\n    let x = 42;\n}";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(mutant.line >= 1);
            assert!(mutant.column >= 1);
        }
    }

    #[test]
    fn test_control_flow_while_body() {
        let code = b"fn main() { while x > 0 { x -= 1; } }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        let while_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("Skip while"))
            .collect();
        assert!(!while_mutants.is_empty());
    }

    #[test]
    fn test_control_flow_for_body() {
        let code = b"fn main() { for i in 0..10 { println!(\"{}\", i); } }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        let for_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("Skip for"))
            .collect();
        assert!(!for_mutants.is_empty());
    }

    #[test]
    fn test_multiple_statements_generate_multiple_mutants() {
        let code = b"fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n}";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate at least one mutant per let statement
        let let_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.original.contains("let"))
            .collect();
        assert!(let_mutants.len() >= 3);
    }

    #[test]
    fn test_mutant_ids_are_unique() {
        let code = b"fn main() {\n    let a = 1;\n    let b = 2;\n}";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ids: Vec<_> = mutants.iter().map(|m| &m.id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_operator_field_is_sdl() {
        let code = b"fn main() { let x = 1; }";
        let result = parse_code(code, Language::Rust);
        let op = StatementOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "SDL");
        }
    }
}
