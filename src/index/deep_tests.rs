//! The index reads source with very deep nesting on a small stack.

use std::path::Path;

use crate::core::Language;
use crate::parser::deep::{self, nest, on_small_stack, DEPTH};
use crate::parser::Parser;

use super::extract::{collect_outermost, extract_file, find_descendant, FileSymbols};

fn extract(language: Language, file: &str, source: &str) -> FileSymbols {
    let parsed = Parser::new()
        .parse(source.as_bytes(), language, Path::new(file))
        .unwrap();
    extract_file(&parsed, file)
}

fn symbol_names(facts: &FileSymbols) -> Vec<String> {
    facts
        .symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect()
}

const ECMA: [(Language, &str); 4] = [
    (Language::JavaScript, "a.js"),
    (Language::Jsx, "a.jsx"),
    (Language::TypeScript, "a.ts"),
    (Language::Tsx, "a.tsx"),
];

#[test]
fn extract_file_reads_each_deep_source() {
    on_small_stack(|| {
        for (source, parsed) in deep::parsed_deep_sources() {
            extract_file(parsed, &source.file);
        }
    });
}

#[test]
fn find_descendant_reads_deep_nesting() {
    let found = on_small_stack(|| {
        let parsed = deep::parsed_deep_sources()
            .iter()
            .find(|(source, _)| source.file == "parens.go")
            .map(|(_, parsed)| parsed)
            .unwrap();
        let literal = find_descendant(parsed.root_node(), &["int_literal"]).unwrap();
        parsed.node_text(&literal).to_string()
    });
    assert_eq!(found, "1");
}

#[test]
fn collect_outermost_reads_deep_nesting() {
    let count = on_small_stack(|| {
        let parsed = deep::parsed_deep_sources()
            .iter()
            .find(|(source, _)| source.file == "parens.go")
            .map(|(_, parsed)| parsed)
            .unwrap();
        let mut found = Vec::new();
        collect_outermost(parsed.root_node(), &["int_literal"], &mut found);
        found.len()
    });
    assert_eq!(count, 1);
}

/// `is_name_chain` reads the receiver of a call such as `a.b.b.b()`.
#[test]
fn a_call_on_a_deep_chain_of_names() {
    on_small_stack(|| {
        for (source, parsed) in deep::parsed_deep_sources() {
            if source.file.starts_with("members.") || source.file.starts_with("Members.") {
                extract_file(parsed, &source.file);
            }
        }
    });
}

/// `use_tree` reads each level of `use a::{a::{...b}}`.
#[test]
fn a_deeply_nested_rust_use_list() {
    let imports = on_small_stack(|| {
        let source = format!("use {};\n", nest("a::{", "b", "}"));
        let facts = extract(Language::Rust, "a.rs", &source);
        facts
            .imports
            .iter()
            .map(|import| import.local_name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(imports, [Some("b".to_string())]);
}

/// `c::function_name` reads each pointer declarator above the name.
#[test]
fn a_c_function_below_deep_pointers() {
    let names = on_small_stack(|| {
        let source = format!("int {}f(void) {{ return 0; }}\n", "*".repeat(DEPTH));
        [(Language::C, "a.c"), (Language::Cpp, "a.cpp")].map(|(language, file)| {
            let parsed = Parser::new()
                .parse(source.as_bytes(), language, Path::new(file))
                .unwrap();
            let definition = parsed.root_node().named_child(0).unwrap();
            let declarator = definition.child_by_field_name("declarator").unwrap();
            super::lang::c::function_name(declarator, &["identifier"])
                .map(|name| parsed.node_text(&name).to_string())
        })
    });
    assert_eq!(names, [Some("f".to_string()), Some("f".to_string())]);
}

/// `collect_base_names` reads each node of a base class clause.
#[test]
fn a_cpp_base_class_with_deep_nesting() {
    on_small_stack(|| {
        let source = format!(
            "struct A : [[deprecated({})]] B {{}};\n",
            nest("(", "1", ")")
        );
        extract(Language::Cpp, "a.cpp", &source);
    });
}

/// `declared_types_of` reads the body of the function of `x.reset()`.
#[test]
fn a_cpp_smart_pointer_call_in_a_deep_function() {
    on_small_stack(|| {
        let source = format!(
            "void f() {{ int y = {}; T x; x.reset(); }}\n",
            nest("(", "1", ")")
        );
        extract(Language::Cpp, "a.cpp", &source);
    });
}

/// `collect_supertypes` reads each node of an `extends` clause.
#[test]
fn an_ecma_class_that_extends_a_deep_expression() {
    on_small_stack(|| {
        let source = format!("class A extends {} {{}}\n", nest("(", "B", ")"));
        for (language, file) in ECMA {
            extract(language, file, &source);
        }
    });
}

/// `pattern_names` reads each level of a parameter pattern.
#[test]
fn an_ecma_parameter_with_a_deep_pattern() {
    on_small_stack(|| {
        let source = format!("function f({}) {{}}\n", nest("[", "x", "]"));
        for (language, file) in ECMA {
            extract(language, file, &source);
        }
    });
}

/// `pattern_binds` reads each level of a declaration pattern.
#[test]
fn an_ecma_declaration_with_a_deep_pattern() {
    on_small_stack(|| {
        let source = format!(
            "function f() {{ const {} = y; window.x = 1; }}\n",
            nest("[", "window", "]")
        );
        for (language, file) in ECMA {
            extract(language, file, &source);
        }
    });
}

/// `collect_type_names` reads each node of a supertype.
#[test]
fn a_java_class_that_extends_a_deeply_annotated_type() {
    on_small_stack(|| {
        let source = format!("class A extends @X({}) B {{}}\n", nest("(", "1", ")"));
        extract(Language::Java, "A.java", &source);
    });
}

/// `find_descendant` reads the receiver of a Go method.
#[test]
fn a_go_method_with_a_deep_receiver() {
    let names = on_small_stack(|| {
        let source = format!("package p\nfunc (r {}) f() {{}}\n", nest("(", "T", ")"));
        symbol_names(&extract(Language::Go, "a.go", &source))
    });
    assert_eq!(names, ["f"]);
}

/// `find_descendant` reads the name of a Python type alias.
#[test]
fn a_python_type_alias_with_a_deep_name() {
    on_small_stack(|| {
        let source = format!("type {} = int\n", nest("(", "X", ")"));
        extract(Language::Python, "a.py", &source);
    });
}
