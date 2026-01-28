//! CRR (Constant Replacement) mutation operator.
//!
//! This operator replaces constant/literal values with different values:
//! - 0 -> 1
//! - 1 -> 0
//! - n -> n + 1 (for other integers)
//! - n -> n - 1 (for other integers)
//! - true -> false
//! - false -> true

use crate::core::Language;
use crate::parser::ParseResult;

use super::super::operator::MutationOperator;
use super::super::Mutant;
use super::walk_and_collect_mutants;

/// CRR (Constant Replacement) operator.
///
/// Replaces literal values with boundary cases and related values.
pub struct LiteralOperator;

impl MutationOperator for LiteralOperator {
    fn name(&self) -> &'static str {
        "CRR"
    }

    fn description(&self) -> &'static str {
        "Constant Replacement - replaces literal values with boundary cases"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let literal_types = get_literal_node_types(result.language);
        let boolean_types = get_boolean_node_types(result.language);
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| {
            let kind = node.kind();
            let mut mutants = Vec::new();

            if literal_types.contains(&kind) {
                if let Ok(text) = node.utf8_text(&result.source) {
                    if let Some(replacements) = generate_literal_replacements(text, result.language)
                    {
                        for replacement in replacements {
                            counter += 1;
                            let id = format!("{}-{}", mutant_id_prefix, counter);
                            let start = node.start_position();
                            mutants.push(Mutant::new(
                                id,
                                result.path.clone(),
                                self.name(),
                                (start.row + 1) as u32,
                                (start.column + 1) as u32,
                                text,
                                replacement.clone(),
                                format!("Replace {} with {}", text, replacement),
                                (node.start_byte(), node.end_byte()),
                            ));
                        }
                    }
                }
            }

            if boolean_types.contains(&kind) {
                if let Ok(text) = node.utf8_text(&result.source) {
                    if let Some(replacement) = generate_boolean_replacement(text, result.language) {
                        counter += 1;
                        let id = format!("{}-{}", mutant_id_prefix, counter);
                        let start = node.start_position();
                        mutants.push(Mutant::new(
                            id,
                            result.path.clone(),
                            self.name(),
                            (start.row + 1) as u32,
                            (start.column + 1) as u32,
                            text,
                            replacement.clone(),
                            format!("Replace {} with {}", text, replacement),
                            (node.start_byte(), node.end_byte()),
                        ));
                    }
                }
            }

            mutants
        })
    }

    fn supports_language(&self, _lang: Language) -> bool {
        true // Supports all languages
    }
}

/// Get node types for integer/float literals.
fn get_literal_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["integer_literal", "float_literal"],
        Language::Go => &["int_literal", "float_literal"],
        Language::Python => &["integer", "float"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &["number"],
        Language::Java | Language::CSharp => &[
            "integer_literal",
            "decimal_integer_literal",
            "hex_integer_literal",
            "binary_integer_literal",
            "octal_integer_literal",
            "decimal_floating_point_literal",
            "real_literal",
        ],
        Language::C | Language::Cpp => &["number_literal"],
        Language::Ruby => &["integer", "float"],
        Language::Php => &["integer", "float"],
        Language::Bash => &[], // Bash doesn't have typed literals
    }
}

/// Get node types for boolean literals.
fn get_boolean_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["boolean_literal"],
        Language::Go => &["true", "false"],
        Language::Python => &["true", "false"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["true", "false"]
        }
        Language::Java | Language::CSharp => &["true", "false"],
        Language::C | Language::Cpp => &["true", "false"],
        Language::Ruby => &["true", "false"],
        Language::Php => &["boolean"],
        Language::Bash => &[],
    }
}

/// Generate replacement values for a numeric literal.
fn generate_literal_replacements(text: &str, _lang: Language) -> Option<Vec<String>> {
    // Try to parse as integer
    if let Ok(n) = text.parse::<i64>() {
        let mut replacements = Vec::new();

        match n {
            0 => {
                replacements.push("1".to_string());
                replacements.push("-1".to_string());
            }
            1 => {
                replacements.push("0".to_string());
                replacements.push("2".to_string());
            }
            -1 => {
                replacements.push("0".to_string());
                replacements.push("-2".to_string());
            }
            _ => {
                // For other values, use n+1, n-1, and 0
                replacements.push(format!("{}", n + 1));
                replacements.push(format!("{}", n - 1));
                replacements.push("0".to_string());
            }
        }

        return Some(replacements);
    }

    // Try to parse as float
    if let Ok(n) = text.parse::<f64>() {
        let mut replacements = Vec::new();

        if n == 0.0 {
            replacements.push("1.0".to_string());
            replacements.push("-1.0".to_string());
        } else if n == 1.0 {
            replacements.push("0.0".to_string());
            replacements.push("2.0".to_string());
        } else {
            replacements.push("0.0".to_string());
            replacements.push(format!("{}", -n));
        }

        return Some(replacements);
    }

    None
}

/// Generate replacement for a boolean literal.
fn generate_boolean_replacement(text: &str, lang: Language) -> Option<String> {
    match text.to_lowercase().as_str() {
        "true" => {
            // Preserve casing based on language
            match lang {
                Language::Python => Some("False".to_string()),
                Language::Ruby => Some("false".to_string()),
                _ => Some("false".to_string()),
            }
        }
        "false" => match lang {
            Language::Python => Some("True".to_string()),
            Language::Ruby => Some("true".to_string()),
            _ => Some("true".to_string()),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::parse_code;
    #[test]
    fn test_literal_operator_name() {
        let op = LiteralOperator;
        assert_eq!(op.name(), "CRR");
    }

    #[test]
    fn test_literal_operator_supports_all_languages() {
        let op = LiteralOperator;
        assert!(op.supports_language(Language::Rust));
        assert!(op.supports_language(Language::Go));
        assert!(op.supports_language(Language::Python));
        assert!(op.supports_language(Language::JavaScript));
    }

    #[test]
    fn test_generate_literal_replacements_zero() {
        let replacements = generate_literal_replacements("0", Language::Rust).unwrap();
        assert!(replacements.contains(&"1".to_string()));
        assert!(replacements.contains(&"-1".to_string()));
    }

    #[test]
    fn test_generate_literal_replacements_one() {
        let replacements = generate_literal_replacements("1", Language::Rust).unwrap();
        assert!(replacements.contains(&"0".to_string()));
        assert!(replacements.contains(&"2".to_string()));
    }

    #[test]
    fn test_generate_literal_replacements_other() {
        let replacements = generate_literal_replacements("42", Language::Rust).unwrap();
        assert!(replacements.contains(&"43".to_string()));
        assert!(replacements.contains(&"41".to_string()));
        assert!(replacements.contains(&"0".to_string()));
    }

    #[test]
    fn test_generate_literal_replacements_negative() {
        let replacements = generate_literal_replacements("-5", Language::Rust).unwrap();
        assert!(replacements.contains(&"-4".to_string()));
        assert!(replacements.contains(&"-6".to_string()));
    }

    #[test]
    fn test_generate_literal_replacements_float() {
        let replacements = generate_literal_replacements("3.14", Language::Rust).unwrap();
        assert!(replacements.contains(&"0.0".to_string()));
        assert!(replacements.contains(&"-3.14".to_string()));
    }

    #[test]
    fn test_generate_boolean_replacement_true() {
        assert_eq!(
            generate_boolean_replacement("true", Language::Rust),
            Some("false".to_string())
        );
        assert_eq!(
            generate_boolean_replacement("True", Language::Python),
            Some("False".to_string())
        );
    }

    #[test]
    fn test_generate_boolean_replacement_false() {
        assert_eq!(
            generate_boolean_replacement("false", Language::Rust),
            Some("true".to_string())
        );
        assert_eq!(
            generate_boolean_replacement("False", Language::Python),
            Some("True".to_string())
        );
    }

    #[test]
    fn test_generate_mutants_rust_integer() {
        let code = b"fn main() { let x = 42; }";
        let result = parse_code(code, Language::Rust);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(!mutants.is_empty());

        // Should find mutations for 42
        let forty_two_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "42").collect();
        assert!(!forty_two_mutants.is_empty());

        // Check that we have the expected replacements
        let replacements: Vec<_> = forty_two_mutants
            .iter()
            .map(|m| m.replacement.as_str())
            .collect();
        assert!(replacements.contains(&"43"));
        assert!(replacements.contains(&"41"));
        assert!(replacements.contains(&"0"));
    }

    #[test]
    fn test_generate_mutants_rust_boolean() {
        let code = b"fn main() { let x = true; }";
        let result = parse_code(code, Language::Rust);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutation for true -> false
        let bool_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "true").collect();
        assert!(!bool_mutants.is_empty());
        assert!(bool_mutants.iter().any(|m| m.replacement == "false"));
    }

    #[test]
    fn test_generate_mutants_go_integer() {
        let code = b"package main\n\nfunc main() { x := 10 }";
        let result = parse_code(code, Language::Go);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");

        let ten_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "10").collect();
        assert!(!ten_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_python_integer() {
        let code = b"x = 100";
        let result = parse_code(code, Language::Python);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");

        let hundred_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "100").collect();
        assert!(!hundred_mutants.is_empty());
    }

    #[test]
    fn test_generate_mutants_js_number() {
        let code = b"const x = 5;";
        let result = parse_code(code, Language::JavaScript);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");

        let five_mutants: Vec<_> = mutants.iter().filter(|m| m.original == "5").collect();
        assert!(!five_mutants.is_empty());
    }

    #[test]
    fn test_mutant_has_correct_position() {
        let code = b"fn main() {\n    let x = 42;\n}";
        let result = parse_code(code, Language::Rust);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");
        let forty_two = mutants.iter().find(|m| m.original == "42").unwrap();

        // 42 should be on line 2 (1-indexed)
        assert_eq!(forty_two.line, 2);
    }

    #[test]
    fn test_mutant_byte_range_is_correct() {
        let code = b"let x = 42;";
        let result = parse_code(code, Language::Rust);
        let op = LiteralOperator;

        let mutants = op.generate_mutants(&result, "test");
        let forty_two = mutants.iter().find(|m| m.original == "42").unwrap();

        // Verify the byte range allows correct replacement
        let (start, end) = forty_two.byte_range;
        assert_eq!(&code[start..end], b"42");
    }
}
