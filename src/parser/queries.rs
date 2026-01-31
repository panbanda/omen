//! Tree-sitter queries for various analyses.

use crate::core::Language;

/// Get decision point node types for cyclomatic complexity.
pub fn get_decision_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Go => &[
            "if_statement",
            "for_statement",
            "select_statement",
            "type_switch_statement",
            "expression_switch_statement",
            // Each case is an independent path per McCabe's methodology,
            // consistent with C/C++ (case_statement) and Ruby (when).
            "expression_case",
        ],
        Language::Rust => &[
            "if_expression",
            "match_expression",
            "for_expression",
            // while_expression covers both `while cond` and `while let pat = expr`
            // in tree-sitter-rust 0.23+, which uses let_condition as a child node
            // rather than a separate while_let_expression node type.
            "while_expression",
            "loop_expression",
        ],
        Language::Python => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "with_statement",
            "try_statement",
            "elif_clause",
            "except_clause",
            "comprehension",
            "conditional_expression",
        ],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &[
            "if_statement",
            "for_statement",
            "for_in_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "ternary_expression",
            "catch_clause",
            // Each case is an independent path per McCabe's methodology,
            // consistent with C/C++ (case_statement) and Ruby (when).
            "switch_case",
        ],
        Language::Java | Language::CSharp => &[
            "if_statement",
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "switch_expression",
            "catch_clause",
            "conditional_expression",
        ],
        Language::C | Language::Cpp => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "case_statement",
            "conditional_expression",
        ],
        Language::Ruby => &[
            "if",
            "unless",
            "while",
            "until",
            "for",
            "case",
            "when",
            "rescue",
            "elsif",
            "conditional",
        ],
        Language::Php => &[
            "if_statement",
            "for_statement",
            "foreach_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "catch_clause",
            "elseif_clause",
        ],
        Language::Bash => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "case_statement",
            "elif_clause",
        ],
    }
}

/// Get nesting node types for cognitive complexity.
pub fn get_nesting_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Ruby => &["if", "unless", "while", "until", "for", "case", "begin"],
        Language::Go => &[
            "if_statement",
            "for_statement",
            "select_statement",
            "type_switch_statement",
            "expression_switch_statement",
        ],
        Language::Python => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "with_statement",
            "try_statement",
        ],
        Language::Rust => &[
            "if_expression",
            "match_expression",
            "for_expression",
            "while_expression",
            "loop_expression",
        ],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &[
            "if_statement",
            "for_statement",
            "for_in_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "try_statement",
        ],
        Language::Java | Language::CSharp => &[
            "if_statement",
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
            "switch_expression",
            "try_statement",
        ],
        Language::C | Language::Cpp => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
        ],
        Language::Php => &[
            "if_statement",
            "for_statement",
            "foreach_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "try_statement",
        ],
        Language::Bash => &[
            "if_statement",
            "for_statement",
            "while_statement",
            "case_statement",
        ],
    }
}

/// Get flat (non-nesting) node types for cognitive complexity.
///
/// Per SonarSource cognitive complexity spec, these constructs add +1 to complexity
/// but do NOT increment the nesting level. This includes:
/// - else/elif/elseif clauses (already inside a nesting if)
/// - catch/except/rescue clauses (already inside a nesting try)
/// - break/continue/goto (flow-breaking statements)
///
/// Reference: https://www.sonarsource.com/docs/CognitiveComplexity.pdf
pub fn get_flat_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Ruby => &["elsif", "else", "when", "rescue", "break", "next", "redo"],
        Language::Python => &[
            "else_clause",
            "elif_clause",
            "except_clause", // Python's catch equivalent
            "break_statement",
            "continue_statement",
        ],
        Language::Go => &["else_clause"],
        Language::Rust => &[
            "else_clause",
            // match_arm contributes to flat complexity (each arm is +1 like case)
            "break_expression",
            "continue_expression",
        ],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => &[
            "else_clause",
            "catch_clause",
            "switch_case",
            "break_statement",
            "continue_statement",
        ],
        Language::Java | Language::CSharp => &[
            "else_clause",
            "catch_clause",
            "break_statement",
            "continue_statement",
        ],
        Language::C | Language::Cpp => &[
            "else_clause",
            "case_statement",
            "catch_clause",
            "break_statement",
            "continue_statement",
            "goto_statement",
        ],
        Language::Php => &[
            "else_clause",
            "elseif_clause",
            "catch_clause",
            "break_statement",
            "continue_statement",
        ],
        Language::Bash => &["elif_clause", "else_clause"],
    }
}

/// Get class/struct node types.
pub fn get_class_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Go => &["type_declaration"],
        Language::Rust => &["struct_item", "enum_item", "impl_item"],
        Language::Python => &["class_definition"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["class_declaration", "class"]
        }
        Language::Java | Language::CSharp => &["class_declaration", "interface_declaration"],
        Language::C => &["struct_specifier"],
        Language::Cpp => &["class_specifier", "struct_specifier"],
        Language::Ruby => &["class", "module"],
        Language::Php => &[
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
        ],
        Language::Bash => &[],
    }
}

/// Get node types for binary arithmetic/bitwise expressions.
///
/// Used by AOR and BOR mutation operators to find binary expression nodes.
pub fn get_binary_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["binary_expression"],
        Language::Go => &["binary_expression"],
        Language::Python => &["binary_operator"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            &["binary_expression"]
        }
        Language::Java | Language::CSharp => &["binary_expression"],
        Language::C | Language::Cpp => &["binary_expression"],
        Language::Ruby => &["binary"],
        Language::Php => &["binary_expression"],
        Language::Bash => &["binary_expression"],
    }
}

/// Get node types for boolean/logical expressions.
///
/// Used by COR mutation operator. Differs from `get_binary_expression_types`
/// because Python uses `boolean_operator` for `and`/`or` rather than `binary_operator`.
pub fn get_boolean_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => &["boolean_operator"],
        _ => get_binary_expression_types(lang),
    }
}

/// Get node types for comparison/relational expressions.
///
/// Used by ROR and BVO mutation operators. Differs from `get_binary_expression_types`
/// because Python uses `comparison_operator` for `<`, `>`, `==`, etc.
pub fn get_comparison_expression_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => &["comparison_operator"],
        _ => get_binary_expression_types(lang),
    }
}

/// Check if a node type represents a logical operator.
pub fn is_logical_operator(node_type: &str) -> bool {
    matches!(node_type, "&&" | "||" | "and" | "or")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language must have an explicit arm in get_nesting_node_types (no catch-all).
    #[test]
    fn test_nesting_node_types_per_language() {
        let all_languages = [
            Language::Go,
            Language::Rust,
            Language::Python,
            Language::TypeScript,
            Language::JavaScript,
            Language::Tsx,
            Language::Jsx,
            Language::Java,
            Language::CSharp,
            Language::C,
            Language::Cpp,
            Language::Ruby,
            Language::Php,
            Language::Bash,
        ];
        for lang in all_languages {
            let types = get_nesting_node_types(lang);
            assert!(!types.is_empty(), "{lang} should have nesting node types");
        }
    }

    /// Every language must have an explicit arm in get_flat_node_types (no catch-all).
    #[test]
    fn test_flat_node_types_per_language() {
        let all_languages = [
            Language::Go,
            Language::Rust,
            Language::Python,
            Language::TypeScript,
            Language::JavaScript,
            Language::Tsx,
            Language::Jsx,
            Language::Java,
            Language::CSharp,
            Language::C,
            Language::Cpp,
            Language::Ruby,
            Language::Php,
            Language::Bash,
        ];
        for lang in all_languages {
            let types = get_flat_node_types(lang);
            assert!(!types.is_empty(), "{lang} should have flat node types");
        }
    }

    #[test]
    fn test_nesting_types_language_specific() {
        // Rust uses _expression suffix, not _statement
        let rust_types = get_nesting_node_types(Language::Rust);
        assert!(rust_types.contains(&"if_expression"));
        assert!(!rust_types.contains(&"if_statement"));

        // Go uses _statement suffix
        let go_types = get_nesting_node_types(Language::Go);
        assert!(go_types.contains(&"if_statement"));
        assert!(go_types.contains(&"select_statement"));

        // Ruby uses bare keywords
        let ruby_types = get_nesting_node_types(Language::Ruby);
        assert!(ruby_types.contains(&"if"));
        assert!(ruby_types.contains(&"unless"));

        // C/C++ should include do_statement
        let c_types = get_nesting_node_types(Language::C);
        assert!(c_types.contains(&"do_statement"));

        // PHP should include foreach_statement
        let php_types = get_nesting_node_types(Language::Php);
        assert!(php_types.contains(&"foreach_statement"));
    }

    #[test]
    fn test_flat_types_language_specific() {
        // Rust uses break_expression / continue_expression, not _statement
        let rust_types = get_flat_node_types(Language::Rust);
        assert!(rust_types.contains(&"break_expression"));
        assert!(rust_types.contains(&"continue_expression"));
        assert!(!rust_types.contains(&"break_statement"));

        // Go only has else_clause (no break/continue as flat complexity)
        let go_types = get_flat_node_types(Language::Go);
        assert!(go_types.contains(&"else_clause"));
        assert_eq!(go_types.len(), 1);

        // Ruby has elsif and rescue
        let ruby_types = get_flat_node_types(Language::Ruby);
        assert!(ruby_types.contains(&"elsif"));
        assert!(ruby_types.contains(&"rescue"));

        // C/C++ should include goto_statement
        let c_types = get_flat_node_types(Language::C);
        assert!(c_types.contains(&"goto_statement"));

        // PHP should include elseif_clause
        let php_types = get_flat_node_types(Language::Php);
        assert!(php_types.contains(&"elseif_clause"));

        // Bash should have elif_clause
        let bash_types = get_flat_node_types(Language::Bash);
        assert!(bash_types.contains(&"elif_clause"));
    }
}

/// SATD (Self-Admitted Technical Debt) comment markers.
pub mod satd {
    /// Design debt markers.
    pub const DESIGN: &[&str] = &["HACK", "KLUDGE", "SMELL", "WORKAROUND", "UGLY", "REFACTOR"];

    /// Defect debt markers.
    pub const DEFECT: &[&str] = &["BUG", "FIXME", "BROKEN", "FAILS", "ERROR"];

    /// Requirement debt markers.
    pub const REQUIREMENT: &[&str] = &["TODO", "FEAT", "IMPLEMENT", "NEED", "TBD"];

    /// Test debt markers.
    pub const TEST: &[&str] = &["FAILING", "SKIP", "DISABLED", "IGNORE", "PENDING"];

    /// Performance debt markers.
    pub const PERFORMANCE: &[&str] = &["SLOW", "OPTIMIZE", "PERF", "BOTTLENECK", "INEFFICIENT"];

    /// Security debt markers. omen:ignore
    pub const SECURITY: &[&str] = &["SECURITY", "VULN", "UNSAFE", "XXX", "INSECURE"];

    /// Documentation debt markers.
    pub const DOCUMENTATION: &[&str] = &["DOC", "UNDOCUMENTED", "DOCUMENT", "NODOC", "UNDOC"];

    /// All marker categories with their severity weights.
    pub fn all_categories() -> &'static [(&'static str, &'static [&'static str], f64)] {
        &[
            ("security", SECURITY, 4.0),
            ("defect", DEFECT, 2.0),
            ("design", DESIGN, 1.0),
            ("performance", PERFORMANCE, 1.0),
            ("documentation", DOCUMENTATION, 0.5),
            ("requirement", REQUIREMENT, 0.25),
            ("test", TEST, 0.25),
        ]
    }
}
