//! Each node kind in a kind table must be a kind of the pinned grammar. A kind
//! that the grammar does not have never matches a node, and the analyzer then
//! reports nothing without an error.

use std::collections::BTreeSet;

use super::{get_function_node_types, get_tree_sitter_language, queries};
use crate::analyzers::{cohesion, repomap};
use crate::core::Language;
use crate::index::extract::{language_spec, NameRule};
use crate::semantic::chunking;

const LANGUAGES: [Language; 14] = [
    Language::Go,
    Language::Rust,
    Language::Python,
    Language::TypeScript,
    Language::JavaScript,
    Language::Tsx,
    Language::Jsx,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::CSharp,
    Language::Ruby,
    Language::Php,
    Language::Bash,
];

/// A new `Language` variant does not compile here until `LANGUAGES` has it.
fn listed(language: Language) -> bool {
    match language {
        Language::Go
        | Language::Rust
        | Language::Python
        | Language::TypeScript
        | Language::JavaScript
        | Language::Tsx
        | Language::Jsx
        | Language::Java
        | Language::C
        | Language::Cpp
        | Language::CSharp
        | Language::Ruby
        | Language::Php
        | Language::Bash => LANGUAGES.contains(&language),
    }
}

/// Kinds that the grammars do not have, as `table/language/kind`. Remove an
/// entry in the change that corrects the table. Do not add an entry.
const KNOWN_UNKNOWN_KINDS: &[&str] = &[
    "call/Php/method_call_expression",
    "call/Ruby/method_call",
    "call/Rust/method_call_expression",
    "decision/CSharp/enhanced_for_statement",
    "decision/Java/conditional_expression",
    "decision/Java/switch_statement",
    "decision/Php/elseif_clause",
    "decision/Python/comprehension",
    "field/JavaScript/public_field_definition",
    "field/TypeScript/field_definition",
    "flat/C/catch_clause",
    "flat/CSharp/else_clause",
    "flat/Go/else_clause",
    "flat/Java/else_clause",
    "flat/Php/elseif_clause",
    "method/JavaScript/public_field_definition",
    "nested_scope/Java/anonymous_method_expression",
    "nested_scope/Java/local_function_statement",
    "nested_scope/Php/anonymous_function_creation_expression",
    "nesting/CSharp/enhanced_for_statement",
];

/// The node kinds of the symbol extraction of the shared index.
fn index_tables(language: Language) -> Vec<(&'static str, Vec<&'static str>)> {
    let spec = language_spec(language);
    vec![
        (
            "index_definition",
            spec.definitions()
                .iter()
                .map(|rule| rule.node_kind)
                .collect(),
        ),
        ("index_hook", spec.hook_kinds()),
        ("index_import", spec.import_kinds()),
        ("index_import_hook", spec.import_hook_kinds()),
        ("index_module", spec.module_kinds()),
        ("index_comment", spec.comment_kinds().to_vec()),
    ]
}

fn tables(language: Language) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut tables = analyzer_tables(language);
    tables.extend(index_tables(language));
    tables
}

fn analyzer_tables(language: Language) -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "decision",
            queries::get_decision_node_types(language).to_vec(),
        ),
        (
            "nesting",
            queries::get_nesting_node_types(language).to_vec(),
        ),
        ("flat", queries::get_flat_node_types(language).to_vec()),
        (
            "nested_scope",
            queries::get_nested_scope_node_types(language).to_vec(),
        ),
        ("class", queries::get_class_node_types(language).to_vec()),
        (
            "binary_expression",
            queries::get_binary_expression_types(language).to_vec(),
        ),
        (
            "boolean_expression",
            queries::get_boolean_expression_types(language).to_vec(),
        ),
        (
            "comparison_expression",
            queries::get_comparison_expression_types(language).to_vec(),
        ),
        (
            "comment",
            queries::get_comment_node_types(language).to_vec(),
        ),
        ("function", get_function_node_types(language)),
        ("call", repomap::get_call_node_kinds(language)),
        ("method", cohesion::get_method_node_types(language)),
        ("field", cohesion::get_field_node_types(language)),
        ("chunk_type", chunking::get_type_node_kinds(language)),
    ]
}

fn unknown_kinds() -> BTreeSet<String> {
    let mut unknown = BTreeSet::new();
    for language in LANGUAGES {
        assert!(listed(language));
        let grammar = get_tree_sitter_language(language).unwrap();
        for (table, kinds) in tables(language) {
            for kind in kinds {
                let named = grammar.id_for_node_kind(kind, true);
                let anonymous = grammar.id_for_node_kind(kind, false);
                if named == 0 && anonymous == 0 {
                    unknown.insert(format!("{table}/{language:?}/{kind}"));
                }
            }
        }
    }
    unknown
}

#[test]
fn each_table_kind_is_a_kind_of_the_grammar() {
    let known: BTreeSet<String> = KNOWN_UNKNOWN_KINDS
        .iter()
        .map(|entry| entry.to_string())
        .collect();
    let unknown = unknown_kinds();
    let new: Vec<&String> = unknown.difference(&known).collect();
    let corrected: Vec<&String> = known.difference(&unknown).collect();
    assert!(
        new.is_empty(),
        "the grammar does not have these kinds:\n{new:#?}"
    );
    assert!(
        corrected.is_empty(),
        "these kinds are now correct, remove them from KNOWN_UNKNOWN_KINDS:\n{corrected:#?}"
    );
}

#[test]
fn each_name_field_of_the_index_is_a_field_of_the_grammar() {
    let mut unknown = BTreeSet::new();
    for language in LANGUAGES {
        let spec = language_spec(language);
        let grammar = get_tree_sitter_language(language).unwrap();
        for rule in spec.definitions() {
            if let NameRule::Field(field) = rule.name {
                if grammar.field_id_for_name(field).is_none() {
                    unknown.insert(format!("{language:?}/{}/{field}", rule.node_kind));
                }
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "the grammar does not have these fields:\n{unknown:#?}"
    );
}

#[test]
fn the_grammar_check_finds_an_unknown_kind() {
    let grammar = get_tree_sitter_language(Language::Rust).unwrap();
    assert_ne!(grammar.id_for_node_kind("function_item", true), 0);
    assert_eq!(grammar.id_for_node_kind("no_such_kind", true), 0);
    assert_eq!(grammar.id_for_node_kind("no_such_kind", false), 0);
}
