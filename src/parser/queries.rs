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
        ],
        Language::Rust => &[
            "if_expression",
            "match_expression",
            "for_expression",
            "while_expression",
            "loop_expression",
            "if_let_expression",
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
        _ => &[
            "if_statement",
            "if_expression",
            "while_statement",
            "while_expression",
            "for_statement",
            "for_expression",
            "switch_statement",
            "match_expression",
            "try_statement",
        ],
    }
}

/// Get flat (non-nesting) node types for cognitive complexity.
pub fn get_flat_node_types(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Ruby => &["elsif", "else", "when", "rescue", "break", "next", "redo"],
        _ => &[
            "else_clause",
            "elif_clause",
            "elseif_clause",
            "break_statement",
            "continue_statement",
            "goto_statement",
        ],
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

/// Check if a node type represents a logical operator.
pub fn is_logical_operator(node_type: &str) -> bool {
    matches!(node_type, "&&" | "||" | "and" | "or")
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

    /// Security debt markers.
    pub const SECURITY: &[&str] = &["SECURITY", "VULN", "UNSAFE", "XXX", "INSECURE"];

    /// All marker categories with their severity weights.
    pub fn all_categories() -> &'static [(&'static str, &'static [&'static str], f64)] {
        &[
            ("security", SECURITY, 4.0),
            ("defect", DEFECT, 2.0),
            ("design", DESIGN, 1.0),
            ("performance", PERFORMANCE, 1.0),
            ("requirement", REQUIREMENT, 0.25),
            ("test", TEST, 0.25),
        ]
    }
}
