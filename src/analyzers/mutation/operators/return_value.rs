//! RVR (Return Value Replacement) mutation operator.
//!
//! This operator replaces return values with alternative values:
//! - `return x` -> `return 0`, `return 1`, `return -1`
//! - `return true` -> `return false`, vice versa
//! - `return Some(x)` -> `return None` (Rust)
//! - `return Ok(x)` -> `return Err(Default::default())` (Rust)
//! - `return nil` -> `return errors.New("mutant")` (Go)
//! - `return null` -> `return new Object()` (Java/C#/JS)
//!
//! Kill probability estimate: ~70-85%
//! Return value mutations are highly effective because they directly affect
//! function outputs. Tests that verify return values will catch these mutations.
//! Lower kill rates occur with unused return values or void-like functions.

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::{mutant_from_node, walk_and_collect_mutants};

/// RVR (Return Value Replacement) operator.
///
/// Generates mutations by replacing return values with boundary cases.
pub struct ReturnValueOperator;

impl MutationOperator for ReturnValueOperator {
    fn name(&self) -> &'static str {
        "RVR"
    }

    fn description(&self) -> &'static str {
        "Return Value Replacement - replaces return values with boundary cases"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let return_types = get_return_node_types(result.language);
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            if !return_types.contains(&node.kind()) {
                return Vec::new();
            }
            let text = match node.utf8_text(&result.source) {
                Ok(t) => t,
                Err(_) => return Vec::new(),
            };
            let replacements = generate_return_replacements(text, result.language);
            replacements
                .into_iter()
                .map(|replacement| {
                    counter += 1;
                    let id = format!("{}-{}", mutant_id_prefix, counter);
                    mutant_from_node(
                        id,
                        result.path.clone(),
                        self.name(),
                        &node,
                        text,
                        replacement.clone(),
                        format!("Replace return with {}", truncate_replacement(&replacement)),
                    )
                })
                .collect()
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true
    }
}

/// Get node types for return statements.
fn get_return_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["return_expression"],
        Language::Go => &["return_statement"],
        Language::Python => &["return_statement"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["return_statement"]
        }
        Language::Java => &["return_statement"],
        Language::CSharp => &["return_statement"],
        Language::C | Language::Cpp => &["return_statement"],
        Language::Ruby => &["return"],
        Language::Php => &["return_statement"],
        Language::Bash => &["return_statement"],
    }
}

/// Generate replacement values for a return statement.
fn generate_return_replacements(text: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();
    let text = text.trim();

    // Extract the return value (after 'return' keyword)
    let return_value = extract_return_value(text, lang);

    // Generate replacements based on the return value type
    if let Some(value) = return_value {
        let value = value.trim();

        // Boolean replacements
        if is_boolean_value(value, lang) {
            replacements.extend(generate_boolean_replacements(value, text, lang));
        }
        // Numeric replacements
        else if is_numeric_value(value) {
            replacements.extend(generate_numeric_replacements(value, text, lang));
        }
        // Rust Option/Result replacements
        else if lang == Language::Rust {
            replacements.extend(generate_rust_option_result_replacements(value, text));
        }
        // Null/nil replacements
        else if is_null_value(value, lang) {
            replacements.extend(generate_null_replacements(text, lang));
        }
        // String replacements
        else if is_string_value(value) {
            replacements.extend(generate_string_replacements(text, lang));
        }
        // Default: replace with common boundary values
        else {
            replacements.extend(generate_default_replacements(text, lang));
        }
    } else {
        // Empty return (e.g., `return;` or `return`)
        replacements.extend(generate_empty_return_replacements(lang));
    }

    replacements
}

/// Extract the return value from a return statement.
fn extract_return_value(text: &str, lang: Language) -> Option<&str> {
    let text = text.trim();

    // Handle different return syntaxes
    let prefix = match lang {
        Language::Ruby => "return ",
        _ => "return ",
    };

    if let Some(rest) = text.strip_prefix(prefix) {
        // Remove trailing semicolon if present
        let rest = rest.trim_end_matches(';').trim();
        if rest.is_empty() {
            None
        } else {
            Some(rest)
        }
    } else if text == "return" {
        None
    } else {
        // Some languages allow implicit returns
        None
    }
}

/// Check if a value is a boolean.
fn is_boolean_value(value: &str, lang: Language) -> bool {
    match lang {
        Language::Python => matches!(value, "True" | "False"),
        _ => matches!(value.to_lowercase().as_str(), "true" | "false"),
    }
}

/// Check if a value is numeric.
fn is_numeric_value(value: &str) -> bool {
    // Try parsing as integer or float
    value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok()
}

/// Check if a value is null/nil.
fn is_null_value(value: &str, lang: Language) -> bool {
    match lang {
        Language::Go => value == "nil",
        Language::Python => value == "None",
        Language::Ruby => value == "nil",
        Language::Rust => value == "None" || value.starts_with("Err("),
        _ => matches!(value, "null" | "NULL" | "nullptr"),
    }
}

/// Check if a value is a string literal.
fn is_string_value(value: &str) -> bool {
    (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('`') && value.ends_with('`'))
}

/// Generate boolean replacements.
fn generate_boolean_replacements(value: &str, original: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();

    let (true_val, false_val) = match lang {
        Language::Python => ("True", "False"),
        _ => ("true", "false"),
    };

    if value.to_lowercase() == "true" {
        replacements.push(original.replace(value, false_val));
    } else {
        replacements.push(original.replace(value, true_val));
    }

    replacements
}

/// Generate numeric replacements.
fn generate_numeric_replacements(value: &str, original: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();

    if let Ok(n) = value.parse::<i64>() {
        match n {
            0 => {
                replacements.push(original.replace(value, "1"));
                replacements.push(original.replace(value, "-1"));
            }
            1 => {
                replacements.push(original.replace(value, "0"));
                replacements.push(original.replace(value, "-1"));
            }
            -1 => {
                replacements.push(original.replace(value, "0"));
                replacements.push(original.replace(value, "1"));
            }
            _ => {
                replacements.push(original.replace(value, "0"));
                replacements.push(original.replace(value, "1"));
                replacements.push(original.replace(value, "-1"));
            }
        }
    } else if value.parse::<f64>().is_ok() {
        let zero = match lang {
            Language::Rust | Language::Go => "0.0",
            _ => "0",
        };
        let one = match lang {
            Language::Rust | Language::Go => "1.0",
            _ => "1",
        };
        replacements.push(original.replace(value, zero));
        replacements.push(original.replace(value, one));
    }

    replacements
}

/// Generate Rust-specific Option/Result replacements.
fn generate_rust_option_result_replacements(value: &str, original: &str) -> Vec<String> {
    let mut replacements = Vec::new();

    // Some(x) -> None
    if value.starts_with("Some(") {
        replacements.push(original.replace(value, "None"));
    }
    // None -> Some(Default::default())
    else if value == "None" {
        replacements.push(original.replace(value, "Some(Default::default())"));
    }
    // Ok(x) -> Err(Default::default())
    else if value.starts_with("Ok(") {
        replacements.push(original.replace(value, "Err(Default::default())"));
    }
    // Err(x) -> Ok(Default::default())
    else if value.starts_with("Err(") {
        replacements.push(original.replace(value, "Ok(Default::default())"));
    }

    replacements
}

/// Generate replacements for null/nil values.
fn generate_null_replacements(original: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();

    match lang {
        Language::Go => {
            // nil -> errors.New("mutant") or &struct{}{}
            replacements.push(original.replace("nil", "errors.New(\"mutant\")"));
        }
        Language::Python => {
            // None -> 0 or ""
            replacements.push(original.replace("None", "0"));
            replacements.push(original.replace("None", "\"\""));
        }
        Language::Ruby => {
            // nil -> 0 or ""
            replacements.push(original.replace("nil", "0"));
            replacements.push(original.replace("nil", "\"\""));
        }
        Language::Java | Language::CSharp => {
            // null -> new Object() or ""
            replacements.push(original.replace("null", "\"\""));
        }
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            // null -> undefined or {} or ""
            replacements.push(original.replace("null", "undefined"));
            replacements.push(original.replace("null", "{}"));
        }
        Language::Cpp => {
            // nullptr -> 0
            replacements.push(original.replace("nullptr", "0"));
        }
        _ => {}
    }

    replacements
}

/// Generate string replacements.
fn generate_string_replacements(original: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();

    // Replace with empty string
    let empty_string = match lang {
        Language::Go => "\"\"".to_string(),
        _ => "\"\"".to_string(),
    };

    // Find and replace the string literal
    if let Some(start) = original.find('"') {
        if let Some(end) = original[start + 1..].find('"') {
            let string_literal = &original[start..start + end + 2];
            replacements.push(original.replace(string_literal, &empty_string));
        }
    }

    replacements
}

/// Generate default replacements for unknown types.
fn generate_default_replacements(original: &str, lang: Language) -> Vec<String> {
    let mut replacements = Vec::new();

    match lang {
        Language::Rust => {
            replacements.push(original.replace(
                &original["return ".len()..original.len() - 1],
                "Default::default()",
            ));
        }
        Language::Go => {
            // For Go, we can't easily determine the type, so provide common defaults
            replacements.push("return 0".to_string());
            replacements.push("return nil".to_string());
        }
        Language::Python => {
            replacements.push("return None".to_string());
            replacements.push("return 0".to_string());
        }
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            replacements.push("return null".to_string());
            replacements.push("return undefined".to_string());
            replacements.push("return 0".to_string());
        }
        Language::Java | Language::CSharp => {
            replacements.push("return null;".to_string());
            replacements.push("return 0;".to_string());
        }
        Language::C | Language::Cpp => {
            replacements.push("return 0;".to_string());
            replacements.push("return NULL;".to_string());
        }
        Language::Ruby => {
            replacements.push("return nil".to_string());
            replacements.push("return 0".to_string());
        }
        Language::Php => {
            replacements.push("return null;".to_string());
            replacements.push("return 0;".to_string());
        }
        Language::Bash => {
            replacements.push("return 0".to_string());
            replacements.push("return 1".to_string());
        }
    }

    replacements
}

/// Generate replacements for empty returns.
fn generate_empty_return_replacements(lang: Language) -> Vec<String> {
    match lang {
        Language::Python => vec!["return 0".to_string(), "return None".to_string()],
        Language::Rust => vec!["return ()".to_string()],
        Language::Go => vec!["return nil".to_string()],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            vec!["return null;".to_string(), "return 0;".to_string()]
        }
        _ => vec!["return 0;".to_string()],
    }
}

/// Truncate a replacement for display.
fn truncate_replacement(text: &str) -> String {
    let text = text.trim();
    if text.len() > 30 {
        format!("{}...", &text[..27])
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_return_value_operator_name() {
        let op = ReturnValueOperator;
        assert_eq!(op.name(), "RVR");
    }

    #[test]
    fn test_return_value_operator_description() {
        let op = ReturnValueOperator;
        assert!(op.description().contains("Return Value Replacement"));
    }

    #[test]
    fn test_return_value_operator_supports_all_languages() {
        let op = ReturnValueOperator;
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
    fn test_generate_mutants_rust_return_integer() {
        let code = b"fn foo() -> i32 { return 42; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate replacements for numeric return
        assert!(!mutants.is_empty());
        let replacements: Vec<_> = mutants.iter().map(|m| m.replacement.as_str()).collect();
        assert!(replacements.iter().any(|r| r.contains("0")));
        assert!(replacements.iter().any(|r| r.contains("1")));
    }

    #[test]
    fn test_generate_mutants_rust_return_bool() {
        let code = b"fn foo() -> bool { return true; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate false replacement
        assert!(!mutants.is_empty());
        assert!(mutants.iter().any(|m| m.replacement.contains("false")));
    }

    #[test]
    fn test_generate_mutants_rust_return_option_some() {
        let code = b"fn foo() -> Option<i32> { return Some(42); }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate None replacement
        assert!(mutants.iter().any(|m| m.replacement.contains("None")));
    }

    #[test]
    fn test_generate_mutants_rust_return_option_none() {
        let code = b"fn foo() -> Option<i32> { return None; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate Some replacement
        assert!(mutants.iter().any(|m| m.replacement.contains("Some")));
    }

    #[test]
    fn test_generate_mutants_rust_return_result_ok() {
        let code = b"fn foo() -> Result<i32, String> { return Ok(42); }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate Err replacement
        assert!(mutants.iter().any(|m| m.replacement.contains("Err")));
    }

    #[test]
    fn test_generate_mutants_rust_return_result_err() {
        let code = b"fn foo() -> Result<i32, String> { return Err(\"error\".to_string()); }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate Ok replacement
        assert!(mutants.iter().any(|m| m.replacement.contains("Ok")));
    }

    #[test]
    fn test_generate_mutants_python_return_true() {
        let code = b"def foo():\n    return True";
        let result = parse_code(code, Language::Python);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate False replacement (Python casing)
        assert!(mutants.iter().any(|m| m.replacement.contains("False")));
    }

    #[test]
    fn test_generate_mutants_go_return_nil() {
        let code = b"package main\n\nfunc foo() error { return nil }";
        let result = parse_code(code, Language::Go);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate error replacement
        assert!(mutants.iter().any(|m| m.replacement.contains("errors")));
    }

    #[test]
    fn test_generate_mutants_js_return_null() {
        let code = b"function foo() { return null; }";
        let result = parse_code(code, Language::JavaScript);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should generate undefined or {} replacement
        let replacements: Vec<_> = mutants.iter().map(|m| m.replacement.as_str()).collect();
        assert!(
            replacements.iter().any(|r| r.contains("undefined"))
                || replacements.iter().any(|r| r.contains("{}"))
        );
    }

    #[test]
    fn test_extract_return_value() {
        assert_eq!(
            extract_return_value("return 42;", Language::Rust),
            Some("42")
        );
        assert_eq!(
            extract_return_value("return true", Language::Python),
            Some("true")
        );
        assert_eq!(extract_return_value("return;", Language::C), None);
        assert_eq!(extract_return_value("return", Language::Go), None);
    }

    #[test]
    fn test_is_boolean_value() {
        assert!(is_boolean_value("true", Language::Rust));
        assert!(is_boolean_value("false", Language::JavaScript));
        assert!(is_boolean_value("True", Language::Python));
        assert!(is_boolean_value("False", Language::Python));
        assert!(!is_boolean_value("1", Language::Rust));
    }

    #[test]
    fn test_is_numeric_value() {
        assert!(is_numeric_value("42"));
        assert!(is_numeric_value("-1"));
        assert!(is_numeric_value("3.14"));
        assert!(!is_numeric_value("true"));
        assert!(!is_numeric_value("hello"));
    }

    #[test]
    fn test_is_null_value() {
        assert!(is_null_value("nil", Language::Go));
        assert!(is_null_value("None", Language::Python));
        assert!(is_null_value("null", Language::Java));
        assert!(is_null_value("nullptr", Language::Cpp));
        assert!(!is_null_value("0", Language::C));
    }

    #[test]
    fn test_is_string_value() {
        assert!(is_string_value("\"hello\""));
        assert!(is_string_value("'hello'"));
        assert!(is_string_value("`hello`"));
        assert!(!is_string_value("hello"));
        assert!(!is_string_value("42"));
    }

    #[test]
    fn test_mutant_byte_range_correct() {
        let code = b"fn foo() -> i32 { return 42; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_mutant_positions_are_one_indexed() {
        let code = b"fn foo() -> bool {\n    return true;\n}";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert!(mutant.line >= 1);
            assert!(mutant.column >= 1);
        }
    }

    #[test]
    fn test_mutant_ids_are_unique() {
        let code = b"fn foo() -> i32 { return 42; }\nfn bar() -> i32 { return 10; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ids: Vec<_> = mutants.iter().map(|m| &m.id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_operator_field_is_rvr() {
        let code = b"fn foo() -> i32 { return 1; }";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "RVR");
        }
    }

    #[test]
    fn test_truncate_replacement_short() {
        let result = truncate_replacement("return 0");
        assert_eq!(result, "return 0");
    }

    #[test]
    fn test_truncate_replacement_long() {
        let long_text = "return Some(very_long_complex_expression_here)";
        let result = truncate_replacement(long_text);
        assert!(result.len() <= 33);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_generate_return_replacements_zero() {
        let replacements = generate_return_replacements("return 0;", Language::Rust);
        assert!(replacements.iter().any(|r| r.contains("1")));
        assert!(replacements.iter().any(|r| r.contains("-1")));
    }

    #[test]
    fn test_java_return_statement() {
        let code = b"class Foo { int bar() { return 42; } }";
        let result = parse_code(code, Language::Java);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());
    }

    #[test]
    fn test_multiple_returns_generate_multiple_mutants() {
        let code = b"fn foo(x: bool) -> i32 {\n    if x { return 1; }\n    return 0;\n}";
        let result = parse_code(code, Language::Rust);
        let op = ReturnValueOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should have mutants for both return statements
        assert!(mutants.len() >= 2);
    }
}
