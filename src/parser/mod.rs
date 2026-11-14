//! Tree-sitter based multi-language parser.

pub mod queries;

#[cfg(test)]
mod kind_lint;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use crate::core::{Error, Language, Result, SourceFile};

// Thread-local parser cache to avoid lock contention in parallel parsing.
// Each rayon worker thread gets its own set of parsers.
thread_local! {
    static THREAD_PARSERS: RefCell<HashMap<Language, TsParser>> = RefCell::new(HashMap::new());
}

/// Thread-safe parser for multi-language parsing.
/// Uses thread-local storage to enable lock-free parallel parsing.
pub struct Parser;

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse a file and return the syntax tree.
    pub fn parse_file(&self, path: impl AsRef<Path>) -> Result<ParseResult> {
        let file = SourceFile::load(path)?;
        self.parse_source(&file)
    }

    /// Parse source content.
    pub fn parse_source(&self, file: &SourceFile) -> Result<ParseResult> {
        self.parse(&file.content, file.language, &file.path)
    }

    /// Parse content with explicit language.
    pub fn parse(&self, content: &[u8], lang: Language, path: &Path) -> Result<ParseResult> {
        let ts_lang = get_tree_sitter_language(lang)?;

        let tree = THREAD_PARSERS.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            let parser = parsers.entry(lang).or_insert_with(|| {
                let mut p = TsParser::new();
                p.set_language(&ts_lang).expect("Language should be valid");
                p
            });

            parser.parse(content, None).ok_or_else(|| Error::Parse {
                path: path.to_path_buf(),
                message: "Failed to parse file".to_string(),
            })
        })?;

        Ok(ParseResult {
            tree: Arc::new(tree),
            source: content.to_vec(),
            language: lang,
            path: path.to_path_buf(),
        })
    }
}

/// Result of parsing a source file.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// The parsed syntax tree.
    pub tree: Arc<Tree>,
    /// Original source content.
    pub source: Vec<u8>,
    /// Detected language.
    pub language: Language,
    /// File path.
    pub path: std::path::PathBuf,
}

impl ParseResult {
    /// Get the root node of the tree.
    pub fn root_node(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    /// Get text for a node.
    pub fn node_text(&self, node: &tree_sitter::Node<'_>) -> &str {
        node.utf8_text(&self.source).unwrap_or("")
    }
}

/// Get tree-sitter language for a Language enum value.
pub fn get_tree_sitter_language(lang: Language) -> Result<TsLanguage> {
    let ts_lang = match lang {
        Language::Go => tree_sitter_go::LANGUAGE,
        Language::Rust => tree_sitter_rust::LANGUAGE,
        Language::Python => tree_sitter_python::LANGUAGE,
        Language::TypeScript | Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
        Language::JavaScript | Language::Jsx => tree_sitter_javascript::LANGUAGE,
        Language::Java => tree_sitter_java::LANGUAGE,
        Language::C => tree_sitter_c::LANGUAGE,
        Language::Cpp => tree_sitter_cpp::LANGUAGE,
        Language::CSharp => tree_sitter_c_sharp::LANGUAGE,
        Language::Ruby => tree_sitter_ruby::LANGUAGE,
        Language::Php => tree_sitter_php::LANGUAGE_PHP,
        Language::Bash => tree_sitter_bash::LANGUAGE,
    };
    Ok(ts_lang.into())
}

/// A function extracted from the AST.
///
/// `name` is for display. `binding_name` is the name callers can actually
/// reach the function by, which is not always the same thing: in
/// `const h = function named() {}` callers use `h`, while `named` is only
/// visible inside the body. Anything that indexes symbols -- dead code, the
/// repo map, outlines, semantic search -- must key off `binding_name` and
/// skip functions where it is `None`. Complexity, which measures bodies
/// rather than names, uses every function regardless.
#[derive(Debug, Clone, Default)]
pub struct FunctionNode {
    /// Function name, for display. Falls back to the intrinsic name and then
    /// to `<anonymous>:<line>` when there is no binding.
    pub name: String,
    /// The externally callable name, or `None` for a callback or an IIFE.
    pub binding_name: Option<String>,
    /// Whether this function has a `binding_name`.
    pub is_bound: bool,
    /// Start line (1-indexed).
    pub start_line: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// Byte range of the function body (start, end).
    pub body_byte_range: Option<(usize, usize)>,
}

/// Extract functions from a parse result.
pub fn extract_functions(result: &ParseResult) -> Vec<FunctionNode> {
    let mut functions = Vec::new();
    let root = result.root_node();

    let function_types = get_function_node_types(result.language);

    fn visit(
        node: tree_sitter::Node<'_>,
        source: &[u8],
        lang: Language,
        function_types: &[&str],
        functions: &mut Vec<FunctionNode>,
    ) {
        if function_types.contains(&node.kind()) {
            if let Some(func) = extract_function_info(&node, source, lang) {
                functions.push(func);
            }
        }

        for child in node.children(&mut node.walk()) {
            visit(child, source, lang, function_types, functions);
        }
    }

    visit(
        root,
        &result.source,
        result.language,
        &function_types,
        &mut functions,
    );

    functions
}

/// Get the AST node kinds that represent a function/method definition in
/// `lang`. This is the same list `extract_functions` uses to decide what
/// counts as its own reportable function -- reused by the complexity
/// analyzer to identify function-definition nodes precisely (narrower than
/// a loose `kind.contains("function")` substring match, which can admit
/// non-definition kinds in some grammars).
pub fn get_function_node_types(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Go => vec!["function_declaration", "method_declaration"],
        Language::Rust => vec!["function_item", "impl_item"],
        Language::Python => vec!["function_definition"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            // TypeScript's bodyless `function_signature`, `method_signature`
            // and `abstract_method_signature` are deliberately absent: they
            // declare a type, not an implementation, so there is no body to
            // measure.
            vec![
                "function_declaration",
                "generator_function_declaration",
                "method_definition",
                "arrow_function",
                "function_expression",
                "generator_function",
            ]
        }
        Language::Java | Language::CSharp => vec!["method_declaration", "constructor_declaration"],
        Language::C | Language::Cpp => vec!["function_definition"],
        Language::Ruby => vec!["method", "singleton_method"],
        Language::Php => vec!["function_definition", "method_declaration"],
        Language::Bash => vec!["function_definition"],
    }
}

/// Whether `lang` uses the JavaScript/TypeScript grammar family.
fn is_js_family(lang: Language) -> bool {
    matches!(
        lang,
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx
    )
}

/// The JS-family node kinds whose own `name` field *is* their binding, because
/// the declaration introduces the name into the surrounding scope.
const JS_DECLARATION_KINDS: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
];

/// A JS/TS function's externally visible binding.
struct JsBinding {
    name: String,
}

/// Whether `child` occupies `parent`'s `field` slot.
fn occupies_field(
    parent: &tree_sitter::Node<'_>,
    field: &str,
    child: &tree_sitter::Node<'_>,
) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|slot| slot.id() == child.id())
}

/// Resolve the name a JS/TS function expression is bound to in its enclosing
/// scope, e.g. `f` in `const f = () => {}`.
///
/// The walk is deliberately shallow and guarded: it ascends only through
/// wrappers that do not change what the expression is bound to, and then
/// inspects exactly one parent, requiring that the function occupies that
/// parent's value slot. Searching further up would name the callback in
/// `const f = xs.map(x => x)` after `f`.
fn resolve_js_binding(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<JsBinding> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression" => current = parent,
            _ => break,
        }
    }

    let parent = current.parent()?;
    let name = match parent.kind() {
        "variable_declarator" if occupies_field(&parent, "value", &current) => {
            let name_node = parent.child_by_field_name("name")?;
            // A destructuring pattern binds no single name.
            if name_node.kind() != "identifier" {
                return None;
            }
            node_text(&name_node, source)?
        }
        // An object-literal property is only reachable through its object,
        // and usage collection tracks bare identifiers rather than property
        // access -- so binding these would make every entry of a mock or
        // config object read as "no references found".
        "pair" => return None,
        // TypeScript and JavaScript name the class-field slot differently.
        "public_field_definition" if occupies_field(&parent, "value", &current) => {
            static_property_name(&parent.child_by_field_name("name")?, source)?
        }
        "field_definition" if occupies_field(&parent, "value", &current) => {
            static_property_name(&parent.child_by_field_name("property")?, source)?
        }
        "assignment_expression" if occupies_field(&parent, "right", &current) => {
            let left = parent.child_by_field_name("left")?;
            match left.kind() {
                "identifier" => node_text(&left, source)?,
                "member_expression" => node_text(&left.child_by_field_name("property")?, source)?,
                // `obj[k] = ...` has no static name.
                _ => return None,
            }
        }
        // Only a default export fills the `value` slot. TypeScript's
        // `export = () => {}` attaches the function as an unnamed child, so
        // the field guard correctly excludes it.
        "export_statement" if occupies_field(&parent, "value", &current) => "default".to_string(),
        _ => return None,
    };

    Some(JsBinding { name })
}

/// A property key usable as a static name, or `None` for anything callers
/// cannot reach with plain `obj.name` syntax.
///
/// Computed keys (`[makeName()]`) are not names at all. String keys are names
/// but can only be called as `obj["k"]()`, which the call-graph consumers do
/// not resolve -- binding one would make the function look dead.
fn static_property_name(key: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    match key.kind() {
        "property_identifier" | "private_property_identifier" => node_text(key, source),
        _ => None,
    }
}

fn node_text(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(|text| text.to_string())
}

/// The name a function-like node is bound to in its enclosing scope, or
/// `None` when it has no external binding.
///
/// Exposed so consumers that track a call graph can name the function a call
/// was made from, which for `const f = () => { ... }` lives on the binding
/// rather than on the function node.
pub fn function_binding_name(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Option<String> {
    if is_js_family(lang) {
        resolve_js_binding(node, source)
            .map(|binding| binding.name)
            .or_else(|| {
                JS_DECLARATION_KINDS
                    .contains(&node.kind())
                    .then(|| find_child_by_field(node, "name", source))?
            })
    } else {
        find_child_by_field(node, "name", source)
    }
}

fn extract_function_info(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Option<FunctionNode> {
    let start_line = node.start_position().row as u32 + 1;

    let (name, binding_name) = if is_js_family(lang) {
        // The function's own name, where the grammar gives it one. Note this
        // must not fall back to a bare `identifier` child the way the other
        // languages do: an arrow function's single unparenthesised parameter
        // is exactly such a child, and matching it names the function after
        // its own parameter.
        let intrinsic = find_child_by_field(node, "name", source);
        let binding = resolve_js_binding(node, source);

        let binding_name = binding.as_ref().map(|b| b.name.clone()).or_else(|| {
            if !JS_DECLARATION_KINDS.contains(&node.kind()) {
                return None;
            }
            // `class C { [makeName()]() {} }` declares no static name.
            if node.kind() == "method_definition" {
                static_property_name(&node.child_by_field_name("name")?, source)
            } else {
                intrinsic.clone()
            }
        });

        let name = binding_name
            .clone()
            .or_else(|| intrinsic.clone())
            .unwrap_or_else(|| format!("<anonymous>:{start_line}"));

        (name, binding_name)
    } else {
        let name = find_child_by_field(node, "name", source)
            .or_else(|| find_named_child(node, "identifier", source))
            .or_else(|| find_named_child(node, "property_identifier", source))
            .or_else(|| {
                // C and C++ place the function name inside a declarator chain:
                // function_definition -> declarator (function_declarator) -> declarator (identifier)
                // Walk the declarator chain until we find an identifier.
                if matches!(lang, Language::C | Language::Cpp) {
                    extract_c_function_name(node, source)
                } else {
                    None
                }
            })?;

        (name.clone(), Some(name))
    };

    Some(FunctionNode {
        name,
        is_bound: binding_name.is_some(),
        binding_name,
        start_line,
        end_line: node.end_position().row as u32 + 1,
        body_byte_range: function_body_byte_range(node),
    })
}

/// Get a function-like node's body node, if present.
///
/// Shared by `extract_function_info` (to populate
/// `FunctionNode::body_byte_range`) and by the complexity analyzer (to
/// re-locate the exact same AST node later by byte range). Using the same
/// lookup in both places guarantees the two stay in sync.
fn function_body_node<'a>(node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
    node.child_by_field_name("body")
        .or_else(|| find_node_child(node, "block"))
}

/// Get the byte range of a function-like node's body, if present.
///
/// This uniquely identifies the specific function/method definition a
/// `FunctionNode` was extracted from, even when several functions share a
/// start line -- e.g. multiple functions on one line in minified/generated
/// code, or a nested function whose declaration starts on the same line as
/// its enclosing function. Line numbers alone cannot disambiguate those
/// cases; byte ranges always can.
pub fn function_body_byte_range(node: &tree_sitter::Node<'_>) -> Option<(usize, usize)> {
    function_body_node(node).map(|b| (b.start_byte(), b.end_byte()))
}

/// Recursively walk a C/C++ declarator chain to find the function name.
///
/// In tree-sitter-c/cpp, `function_definition` has a `declarator` field that
/// points to a `function_declarator`.  That node in turn has a `declarator`
/// field that is the actual `identifier`.  This function recurses through
/// `function_declarator` and `pointer_declarator` nodes until it reaches an
/// `identifier`.
fn extract_c_function_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    fn recurse_declarator(decl: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match decl.kind() {
            "identifier" => decl.utf8_text(source).ok().map(|s| s.to_string()),
            "function_declarator"
            | "pointer_declarator"
            | "reference_declarator"
            | "abstract_declarator" => {
                // Try the named `declarator` field first, then first named child
                if let Some(inner) = decl.child_by_field_name("declarator") {
                    return recurse_declarator(inner, source);
                }
                for child in decl.children(&mut decl.walk()) {
                    if child.is_named() {
                        if let Some(name) = recurse_declarator(child, source) {
                            return Some(name);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    // Walk the `declarator` field of the function_definition
    if let Some(decl) = node.child_by_field_name("declarator") {
        return recurse_declarator(decl, source);
    }
    None
}

fn find_child_by_field(node: &tree_sitter::Node<'_>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

fn find_named_child(node: &tree_sitter::Node<'_>, kind: &str, source: &[u8]) -> Option<String> {
    for child in node.children(&mut node.walk()) {
        if child.kind() == kind {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
    }
    None
}

fn find_node_child<'a>(node: &tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    node.children(&mut node.walk())
        .find(|&child| child.kind() == kind)
}

/// Find byte ranges of Rust `#[cfg(test)]` modules using tree-sitter.
///
/// Returns ranges covering each module's body so that other analyzers can
/// skip Rust test code without fragile brace-counting heuristics. Shared by
/// `analyzers::tdg` (critical-defect detection) and `analyzers::duplicates`
/// (clone detection) so both exclude the same `#[cfg(test)]` code.
///
/// Performs a full depth-first walk so nested test modules are detected, and
/// correctly excludes `#[cfg(not(test))]` / `#[cfg(feature = "contest")]`
/// false matches.
pub fn cfg_test_module_ranges(root: &tree_sitter::Node<'_>, source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    collect_cfg_test_modules(root, source.as_bytes(), &mut ranges);
    ranges
}

fn collect_cfg_test_modules(
    node: &tree_sitter::Node,
    source: &[u8],
    ranges: &mut Vec<Range<usize>>,
) {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let child = cursor.node();
        match child.kind() {
            "mod_item" => {
                if has_cfg_test_attribute(&child, source) {
                    ranges.push(child.start_byte()..child.end_byte());
                } else {
                    collect_cfg_test_modules(&child, source, ranges);
                }
            }
            // mod body: recurse into declaration_list to find nested modules
            "declaration_list" => {
                collect_cfg_test_modules(&child, source, ranges);
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn has_cfg_test_attribute(node: &tree_sitter::Node, source: &[u8]) -> bool {
    let Some(prev) = node.prev_sibling() else {
        return false;
    };
    if prev.kind() != "attribute_item" {
        return false;
    }
    let Ok(text) = prev.utf8_text(source) else {
        return false;
    };
    // Match #[cfg(test)] but not #[cfg(not(test))] or #[cfg(feature = "contest")]
    text.contains("cfg(test)") && !text.contains("not(test)") && !text.contains("not(all(test")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_default() {
        let parser = Parser;
        let content = b"fn main() {}";
        let result = parser.parse(content, Language::Rust, Path::new("main.rs"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_go() {
        let parser = Parser::new();
        let content = b"package main\n\nfunc main() {\n\tprintln(\"Hello\")\n}\n";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();

        assert_eq!(result.language, Language::Go);
        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "main");
    }

    #[test]
    fn test_parse_rust() {
        let parser = Parser::new();
        let content = b"fn main() {\n    println!(\"Hello\");\n}\n";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        assert_eq!(result.language, Language::Rust);
        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "main");
    }

    #[test]
    fn test_parse_python() {
        let parser = Parser::new();
        let content = b"def hello():\n    print(\"Hello\")\n";
        let result = parser
            .parse(content, Language::Python, Path::new("main.py"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "hello");
    }

    #[test]
    fn test_parse_typescript() {
        let parser = Parser::new();
        let content = b"function greet() { console.log('hi'); }";
        let result = parser
            .parse(content, Language::TypeScript, Path::new("main.ts"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "greet");
    }

    #[test]
    fn test_parse_javascript() {
        let parser = Parser::new();
        let content = b"function test() { return 42; }";
        let result = parser
            .parse(content, Language::JavaScript, Path::new("main.js"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "test");
    }

    #[test]
    fn test_parse_java() {
        let parser = Parser::new();
        let content = b"public class Main { public void hello() {} }";
        let result = parser
            .parse(content, Language::Java, Path::new("Main.java"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_c() {
        let parser = Parser::new();
        let content = b"int main() { return 0; }";
        let result = parser
            .parse(content, Language::C, Path::new("main.c"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1, "C should extract 1 function");
        assert_eq!(
            functions[0].name, "main",
            "C function name should be 'main'"
        );
    }

    #[test]
    fn test_parse_cpp() {
        let parser = Parser::new();
        let content = b"int main() { return 0; }";
        let result = parser
            .parse(content, Language::Cpp, Path::new("main.cpp"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1, "C++ should extract 1 function");
        assert_eq!(
            functions[0].name, "main",
            "C++ function name should be 'main'"
        );
    }

    #[test]
    fn test_parse_ruby() {
        let parser = Parser::new();
        let content = b"def hello\n  puts 'hi'\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "hello");
    }

    #[test]
    fn test_parse_php() {
        let parser = Parser::new();
        let content = b"<?php function hello() { echo 'hi'; }";
        let result = parser
            .parse(content, Language::Php, Path::new("main.php"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_bash() {
        let parser = Parser::new();
        let content = b"hello() { echo hi; }";
        let result = parser
            .parse(content, Language::Bash, Path::new("script.sh"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_csharp() {
        let parser = Parser::new();
        let content = b"class Main { public void Hello() {} }";
        let result = parser
            .parse(content, Language::CSharp, Path::new("Main.cs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_tsx() {
        let parser = Parser::new();
        let content = b"function Component() { return <div />; }";
        let result = parser
            .parse(content, Language::Tsx, Path::new("component.tsx"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_jsx() {
        let parser = Parser::new();
        let content = b"function App() { return <div />; }";
        let result = parser
            .parse(content, Language::Jsx, Path::new("app.jsx"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
    }

    #[test]
    fn test_parse_result_root_node() {
        let parser = Parser::new();
        let content = b"fn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let root = result.root_node();
        assert_eq!(root.kind(), "source_file");
    }

    #[test]
    fn test_parse_result_node_text() {
        let parser = Parser::new();
        let content = b"fn hello() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let root = result.root_node();
        let text = result.node_text(&root);
        assert!(text.contains("fn hello()"));
    }

    #[test]
    fn test_get_tree_sitter_language_all() {
        assert!(get_tree_sitter_language(Language::Go).is_ok());
        assert!(get_tree_sitter_language(Language::Rust).is_ok());
        assert!(get_tree_sitter_language(Language::Python).is_ok());
        assert!(get_tree_sitter_language(Language::TypeScript).is_ok());
        assert!(get_tree_sitter_language(Language::JavaScript).is_ok());
        assert!(get_tree_sitter_language(Language::Tsx).is_ok());
        assert!(get_tree_sitter_language(Language::Jsx).is_ok());
        assert!(get_tree_sitter_language(Language::Java).is_ok());
        assert!(get_tree_sitter_language(Language::C).is_ok());
        assert!(get_tree_sitter_language(Language::Cpp).is_ok());
        assert!(get_tree_sitter_language(Language::CSharp).is_ok());
        assert!(get_tree_sitter_language(Language::Ruby).is_ok());
        assert!(get_tree_sitter_language(Language::Php).is_ok());
        assert!(get_tree_sitter_language(Language::Bash).is_ok());
    }

    #[test]
    fn test_function_line_numbers() {
        let parser = Parser::new();
        let content = b"fn first() {}\n\nfn second() {\n  println!(\"hi\");\n}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].start_line, 1);
        assert_eq!(functions[1].start_line, 3);
    }

    #[test]
    fn test_java_public_method() {
        let parser = Parser::new();
        let content = b"class Main { public void hello() {} private void secret() {} }";
        let result = parser
            .parse(content, Language::Java, Path::new("Main.java"))
            .unwrap();

        let functions = extract_functions(&result);
        assert!(!functions.is_empty());
    }

    #[test]
    fn test_parser_reuse() {
        let parser = Parser::new();

        // Parse multiple files with same parser instance
        let result1 = parser.parse(b"fn a() {}", Language::Rust, Path::new("a.rs"));
        let result2 = parser.parse(b"fn b() {}", Language::Rust, Path::new("b.rs"));
        let result3 = parser.parse(b"def c(): pass", Language::Python, Path::new("c.py"));

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
    }

    #[test]
    fn test_arrow_function_typescript() {
        let parser = Parser::new();
        let content = b"const greet = () => { console.log('hi'); };";
        let result = parser
            .parse(content, Language::TypeScript, Path::new("main.ts"))
            .unwrap();

        // Arrow functions are in tree but may not have extractable names
        // Just verify parsing works
        assert!(result.root_node().kind() == "program");
    }

    #[test]
    fn test_method_definition_js() {
        let parser = Parser::new();
        let content = b"class Foo { bar() { return 1; } }";
        let result = parser
            .parse(content, Language::JavaScript, Path::new("main.js"))
            .unwrap();

        let functions = extract_functions(&result);
        assert!(!functions.is_empty());
    }

    #[test]
    fn test_empty_file() {
        let parser = Parser::new();
        let content = b"";
        let result = parser.parse(content, Language::Rust, Path::new("empty.rs"));
        assert!(result.is_ok());
        let functions = extract_functions(&result.unwrap());
        assert!(functions.is_empty());
    }

    #[test]
    fn test_parse_result_clone() {
        let parser = Parser::new();
        let content = b"fn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let cloned = result.clone();
        assert_eq!(cloned.language, result.language);
        assert_eq!(cloned.path, result.path);
    }

    #[test]
    fn test_body_byte_range_rust() {
        let parser = Parser::new();
        let content = b"fn hello() {\n    println!(\"hi\");\n}\n";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("println!"));
    }

    #[test]
    fn test_body_byte_range_go() {
        let parser = Parser::new();
        let content = b"package main\n\nfunc hello() {\n\tfmt.Println(\"hi\")\n}\n";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("Println"));
    }

    #[test]
    fn test_body_byte_range_python() {
        let parser = Parser::new();
        let content = b"def hello():\n    print(\"hi\")\n";
        let result = parser
            .parse(content, Language::Python, Path::new("main.py"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("print"));
    }

    #[test]
    fn test_body_byte_range_typescript() {
        let parser = Parser::new();
        let content = b"function greet() { console.log('hi'); }";
        let result = parser
            .parse(content, Language::TypeScript, Path::new("main.ts"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("console.log"));
    }

    #[test]
    fn test_body_byte_range_javascript() {
        let parser = Parser::new();
        let content = b"function test() { return 42; }";
        let result = parser
            .parse(content, Language::JavaScript, Path::new("main.js"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("return 42"));
    }

    #[test]
    fn test_body_byte_range_java() {
        let parser = Parser::new();
        let content = b"public class Main { public void hello() { System.out.println(\"hi\"); } }";
        let result = parser
            .parse(content, Language::Java, Path::new("Main.java"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        let (start, end) = functions[0].body_byte_range.expect("should have body");
        let body_text = std::str::from_utf8(&content[start..end]).unwrap();
        assert!(body_text.contains("System.out"));
    }

    #[test]
    fn test_body_byte_range_c() {
        let parser = Parser::new();
        let content = b"int add(int a, int b) { return a + b; }";
        let result = parser
            .parse(content, Language::C, Path::new("main.c"))
            .unwrap();

        let functions = extract_functions(&result);
        if !functions.is_empty() {
            if let Some((start, end)) = functions[0].body_byte_range {
                let body_text = std::str::from_utf8(&content[start..end]).unwrap();
                assert!(body_text.contains("return"));
            }
        }
    }

    #[test]
    fn test_body_byte_range_cpp() {
        let parser = Parser::new();
        let content = b"int add(int a, int b) { return a + b; }";
        let result = parser
            .parse(content, Language::Cpp, Path::new("main.cpp"))
            .unwrap();

        let functions = extract_functions(&result);
        if !functions.is_empty() {
            if let Some((start, end)) = functions[0].body_byte_range {
                let body_text = std::str::from_utf8(&content[start..end]).unwrap();
                assert!(body_text.contains("return"));
            }
        }
    }

    #[test]
    fn test_body_none_for_declaration_without_body() {
        let parser = Parser::new();
        // Java abstract method has no body
        let content = b"public abstract class Foo { abstract void bar(); }";
        let result = parser
            .parse(content, Language::Java, Path::new("Foo.java"))
            .unwrap();

        let functions = extract_functions(&result);
        for func in &functions {
            if func.name == "bar" {
                assert!(
                    func.body_byte_range.is_none(),
                    "abstract method should have no body"
                );
            }
        }
    }

    #[test]
    fn test_body_byte_range_multiple_functions() {
        let parser = Parser::new();
        let content = b"fn first() { let a = 1; }\nfn second() { let b = 2; }";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 2);

        let (s1, e1) = functions[0]
            .body_byte_range
            .expect("first should have body");
        let body1 = std::str::from_utf8(&content[s1..e1]).unwrap();
        assert!(body1.contains("let a"));

        let (s2, e2) = functions[1]
            .body_byte_range
            .expect("second should have body");
        let body2 = std::str::from_utf8(&content[s2..e2]).unwrap();
        assert!(body2.contains("let b"));
    }

    // --- Issue #495: JS/TS anonymous function naming ---------------------

    /// Extract functions from a JS-family snippet under one language.
    fn js_functions(lang: Language, ext: &str, source: &str) -> Vec<FunctionNode> {
        let parser = Parser::new();
        let result = parser
            .parse(source.as_bytes(), lang, Path::new(&format!("t.{ext}")))
            .expect("parse should succeed");
        extract_functions(&result)
    }

    /// The JS-family languages, with a file extension for each.
    const JS_FAMILY: &[(Language, &str)] = &[
        (Language::JavaScript, "js"),
        (Language::TypeScript, "ts"),
        (Language::Tsx, "tsx"),
        (Language::Jsx, "jsx"),
    ];

    fn find_by_name<'a>(funcs: &'a [FunctionNode], name: &str) -> &'a FunctionNode {
        funcs
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no function named {name}, got {:?}", names(funcs)))
    }

    fn names(funcs: &[FunctionNode]) -> Vec<&str> {
        funcs.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn test_js_binding_names_across_languages() {
        // (source, expected binding names -- every other extracted function must be unbound)
        let cases: &[(&str, &[&str])] = &[
            ("const f = () => {};", &["f"]),
            ("const f = (a, b) => a + b;", &["f"]),
            ("let g = function () {};", &["g"]),
            ("function* gen() {}", &["gen"]),
            ("obj.method = () => {};", &["method"]),
            ("handler = () => {};", &["handler"]),
            ("export default () => {};", &["default"]),
        ];

        for (lang, ext) in JS_FAMILY {
            for (source, expected) in cases {
                let funcs = js_functions(*lang, ext, source);
                let bound: Vec<&str> = funcs
                    .iter()
                    .filter_map(|f| f.binding_name.as_deref())
                    .collect();
                assert_eq!(
                    bound,
                    *expected,
                    "{lang:?}: bindings for {source:?} (all: {:?})",
                    names(&funcs)
                );
            }
        }
    }

    #[test]
    fn test_js_unbound_functions_are_not_named_after_context() {
        // Each of these has exactly one function, and it has no external binding.
        let cases = &[
            "xs.map(x => x + 1);",
            "xs.map((x) => x + 1);",
            "(function () {})();",
            "(() => {})();",
            // An object-literal property is reachable only through its
            // object, and usage collection tracks bare identifiers rather
            // than property access -- so binding these would make every entry
            // of a mock or config object read as "no references found".
            "const o = { m: () => {} };",
            "const o = { [k]: () => {} };",
            "const o = { \"q-k\": () => {} };",
            "obj[k] = () => {};",
        ];

        for (lang, ext) in JS_FAMILY {
            for source in cases {
                let funcs = js_functions(*lang, ext, source);
                assert_eq!(
                    funcs.len(),
                    1,
                    "{lang:?}: {source:?} -> {:?}",
                    names(&funcs)
                );
                let func = &funcs[0];
                assert!(
                    !func.is_bound && func.binding_name.is_none(),
                    "{lang:?}: {source:?} should be unbound, got {:?}",
                    func.binding_name
                );
                assert!(
                    func.name.starts_with("<anonymous>:"),
                    "{lang:?}: {source:?} should be anonymous, got {:?}",
                    func.name
                );
            }
        }
    }

    #[test]
    fn test_js_arrow_is_not_named_after_its_own_parameter() {
        // The pre-fix bug: `x => ...` reported the function under its parameter's name.
        for (lang, ext) in JS_FAMILY {
            let funcs = js_functions(*lang, ext, "xs.map(x => x + 1);");
            assert_eq!(funcs.len(), 1);
            assert_ne!(funcs[0].name, "x", "{lang:?}: named after its parameter");
        }
    }

    #[test]
    fn test_js_callback_inside_a_binding_does_not_steal_the_binding() {
        // The binding walk must not ascend past the callback's own argument slot.
        for (lang, ext) in JS_FAMILY {
            let funcs = js_functions(*lang, ext, "const f = xs.map(x => x);");
            assert_eq!(funcs.len(), 1, "{lang:?}: {:?}", names(&funcs));
            assert!(
                !funcs[0].is_bound,
                "{lang:?}: callback wrongly bound to {:?}",
                funcs[0].binding_name
            );
        }
    }

    #[test]
    fn test_js_named_function_expression_binds_to_its_variable() {
        // Callers invoke `h`; `named` is only visible inside the body.
        for (lang, ext) in JS_FAMILY {
            let funcs = js_functions(*lang, ext, "const h = function named() {};");
            assert_eq!(funcs.len(), 1, "{lang:?}: {:?}", names(&funcs));
            assert_eq!(funcs[0].binding_name.as_deref(), Some("h"), "{lang:?}");
        }
    }

    #[test]
    fn test_js_named_function_expression_without_binding_is_unbound() {
        // `mapper` is a display name, but nothing outside can call it.
        for (lang, ext) in JS_FAMILY {
            let funcs = js_functions(*lang, ext, "xs.map(function mapper() {});");
            assert_eq!(funcs.len(), 1, "{lang:?}: {:?}", names(&funcs));
            assert_eq!(funcs[0].name, "mapper", "{lang:?}");
            assert!(!funcs[0].is_bound, "{lang:?}: mapper should be unbound");
        }
    }

    #[test]
    fn test_js_computed_names_are_not_bindings() {
        // `[makeName()]` is not a static name, so nothing can address it.
        for (lang, ext) in JS_FAMILY {
            for source in [
                "class A { [makeName()] = () => {} }",
                "class A { [makeName()]() {} }",
            ] {
                let funcs = js_functions(*lang, ext, source);
                assert_eq!(funcs.len(), 1, "{lang:?}: {source:?}");
                assert!(
                    !funcs[0].is_bound,
                    "{lang:?}: {source:?} wrongly bound to {:?}",
                    funcs[0].binding_name
                );
            }
        }
    }

    #[test]
    fn test_js_class_property_arrow_takes_the_field_name() {
        // JS uses `field_definition.property`; TS uses `public_field_definition.name`.
        for (lang, ext) in JS_FAMILY {
            let funcs = js_functions(*lang, ext, "class A { f = () => {} }");
            let f = find_by_name(&funcs, "f");
            assert_eq!(f.binding_name.as_deref(), Some("f"), "{lang:?}");
        }
    }

    #[test]
    fn test_ts_export_assignment_is_not_named_default() {
        // `export = () => {}` is a TS export assignment, not a default export.
        let funcs = js_functions(Language::TypeScript, "ts", "export = () => {};");
        assert_eq!(funcs.len(), 1, "{:?}", names(&funcs));
        assert_ne!(funcs[0].name, "default");
        assert!(!funcs[0].is_bound);
    }

    #[test]
    fn test_non_js_languages_report_declarations_as_bound() {
        let parser = Parser::new();
        let result = parser
            .parse(b"fn main() {}", Language::Rust, Path::new("main.rs"))
            .unwrap();
        let funcs = extract_functions(&result);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].binding_name.as_deref(), Some("main"));
        assert!(funcs[0].is_bound);
    }
}
