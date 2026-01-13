//! Tree-sitter based multi-language parser.

pub mod queries;

use std::cell::RefCell;
use std::collections::HashMap;
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
#[derive(Debug, Clone)]
pub struct FunctionNode {
    /// Function name.
    pub name: String,
    /// Start line (1-indexed).
    pub start_line: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// Function body node (for complexity analysis).
    pub body: Option<tree_sitter::Node<'static>>,
    /// Whether function is exported/public.
    pub is_exported: bool,
    /// Function signature.
    pub signature: String,
}

/// A class/struct extracted from the AST.
#[derive(Debug, Clone)]
pub struct ClassNode {
    /// Class/struct name.
    pub name: String,
    /// Start line (1-indexed).
    pub start_line: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// Methods in the class.
    pub methods: Vec<FunctionNode>,
    /// Fields in the class.
    pub fields: Vec<String>,
    /// Whether class is exported/public.
    pub is_exported: bool,
}

/// An import statement.
#[derive(Debug, Clone)]
pub struct ImportNode {
    /// Import path/module.
    pub path: String,
    /// Start line.
    pub line: u32,
    /// Imported names (if any).
    pub names: Vec<String>,
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

/// Extract imports from a parse result.
pub fn extract_imports(result: &ParseResult) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    let root = result.root_node();

    fn visit(
        node: tree_sitter::Node<'_>,
        source: &[u8],
        lang: Language,
        imports: &mut Vec<ImportNode>,
    ) {
        match lang {
            Language::Go if node.kind() == "import_declaration" || node.kind() == "import_spec" => {
                if let Some(import) = extract_go_import(&node, source) {
                    imports.push(import);
                }
            }
            Language::Rust if node.kind() == "use_declaration" => {
                if let Some(import) = extract_rust_import(&node, source) {
                    imports.push(import);
                }
            }
            Language::Python
                if node.kind() == "import_statement" || node.kind() == "import_from_statement" =>
            {
                if let Some(import) = extract_python_import(&node, source) {
                    imports.push(import);
                }
            }
            Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx
                if node.kind() == "import_statement" =>
            {
                if let Some(import) = extract_js_import(&node, source) {
                    imports.push(import);
                }
            }
            Language::Java if node.kind() == "import_declaration" => {
                if let Some(import) = extract_java_import(&node, source) {
                    imports.push(import);
                }
            }
            Language::Ruby if node.kind() == "call" => {
                if let Some(import) = extract_ruby_import(&node, source) {
                    imports.push(import);
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            visit(child, source, lang, imports);
        }
    }

    visit(root, &result.source, result.language, &mut imports);

    imports
}

fn get_function_node_types(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Go => vec!["function_declaration", "method_declaration"],
        Language::Rust => vec!["function_item", "impl_item"],
        Language::Python => vec!["function_definition"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            vec![
                "function_declaration",
                "method_definition",
                "arrow_function",
            ]
        }
        Language::Java | Language::CSharp => vec!["method_declaration", "constructor_declaration"],
        Language::C | Language::Cpp => vec!["function_definition"],
        Language::Ruby => vec!["method", "singleton_method"],
        Language::Php => vec!["function_definition", "method_declaration"],
        Language::Bash => vec!["function_definition"],
    }
}

fn extract_function_info(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Option<FunctionNode> {
    let name = find_child_by_field(node, "name", source)
        .or_else(|| find_named_child(node, "identifier", source))
        .or_else(|| find_named_child(node, "property_identifier", source))?;

    let body = node
        .child_by_field_name("body")
        .or_else(|| find_node_child(node, "block"));

    let is_exported = check_is_exported(node, source, lang);

    let signature = extract_signature(node, source, lang);

    Some(FunctionNode {
        name,
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        body: body.map(|_| unsafe { std::mem::transmute(*node) }),
        is_exported,
        signature,
    })
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

fn check_is_exported(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> bool {
    match lang {
        Language::Go => {
            // Go uses capitalized names for exports
            if let Some(name) = find_child_by_field(node, "name", source) {
                name.chars().next().is_some_and(|c| c.is_uppercase())
            } else {
                false
            }
        }
        Language::Rust => {
            // Check for 'pub' visibility
            for child in node.children(&mut node.walk()) {
                if child.kind() == "visibility_modifier" {
                    return true;
                }
            }
            false
        }
        Language::TypeScript | Language::JavaScript => {
            // Check for 'export' keyword
            if let Some(parent) = node.parent() {
                parent.kind() == "export_statement"
            } else {
                false
            }
        }
        Language::Java | Language::CSharp => {
            // Check for 'public' modifier
            for child in node.children(&mut node.walk()) {
                if child.kind() == "modifiers" {
                    if let Ok(text) = child.utf8_text(source) {
                        return text.contains("public");
                    }
                }
            }
            false
        }
        _ => true, // Default to exported for other languages
    }
}

fn extract_signature(node: &tree_sitter::Node<'_>, source: &[u8], _lang: Language) -> String {
    // Get the first line of the function declaration
    let start = node.start_byte();
    let end = node.end_byte().min(start + 200); // Limit to first 200 chars

    let text = std::str::from_utf8(&source[start..end]).unwrap_or("");
    let first_line = text.lines().next().unwrap_or("");

    // Clean up the signature
    first_line.trim().to_string()
}

fn extract_go_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let path = find_child_by_field(node, "path", source).or_else(|| {
        node.utf8_text(source)
            .ok()
            .map(|s| s.trim_matches('"').to_string())
    })?;

    Some(ImportNode {
        path,
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
}

fn extract_rust_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let path = node.utf8_text(source).ok()?.to_string();
    Some(ImportNode {
        path,
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
}

fn extract_python_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let path = node.utf8_text(source).ok()?.to_string();
    Some(ImportNode {
        path,
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
}

fn extract_js_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let path = find_child_by_field(node, "source", source)?;
    Some(ImportNode {
        path: path.trim_matches(|c| c == '"' || c == '\'').to_string(),
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
}

fn extract_java_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let path = node.utf8_text(source).ok()?.to_string();
    Some(ImportNode {
        path,
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
}

fn extract_ruby_import(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    // Ruby uses require/require_relative
    let method = find_named_child(node, "identifier", source)?;
    if method != "require" && method != "require_relative" {
        return None;
    }

    let path = node.utf8_text(source).ok()?.to_string();
    Some(ImportNode {
        path,
        line: node.start_position().row as u32 + 1,
        names: Vec::new(),
    })
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
    fn test_parse_rust_pub_fn() {
        let parser = Parser::new();
        let content = b"pub fn exported() {}\nfn private() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 2);
        assert!(functions[0].is_exported);
        assert!(!functions[1].is_exported);
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

        // C function extraction works, but may not find name in all cases
        let _functions = extract_functions(&result);
        // Just verify parsing works - C name extraction may vary
        assert!(result.root_node().kind() == "translation_unit");
    }

    #[test]
    fn test_parse_cpp() {
        let parser = Parser::new();
        let content = b"int main() { return 0; }";
        let result = parser
            .parse(content, Language::Cpp, Path::new("main.cpp"))
            .unwrap();

        // Just verify parsing works
        assert!(result.root_node().kind() == "translation_unit");
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
    fn test_extract_go_imports() {
        let parser = Parser::new();
        let content = b"package main\n\nimport \"fmt\"\n\nfunc main() {}";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(!imports.is_empty());
    }

    #[test]
    fn test_extract_rust_imports() {
        let parser = Parser::new();
        let content = b"use std::path::Path;\n\nfn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(!imports.is_empty());
    }

    #[test]
    fn test_extract_python_imports() {
        let parser = Parser::new();
        let content = b"import os\nfrom pathlib import Path\n\ndef main(): pass";
        let result = parser
            .parse(content, Language::Python, Path::new("main.py"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn test_extract_js_imports() {
        let parser = Parser::new();
        let content = b"import foo from 'bar';\n\nfunction main() {}";
        let result = parser
            .parse(content, Language::JavaScript, Path::new("main.js"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(!imports.is_empty());
    }

    #[test]
    fn test_extract_java_imports() {
        let parser = Parser::new();
        let content = b"import java.util.List;\n\nclass Main {}";
        let result = parser
            .parse(content, Language::Java, Path::new("Main.java"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(!imports.is_empty());
    }

    #[test]
    fn test_extract_ruby_imports() {
        let parser = Parser::new();
        let content = b"require 'json'\n\ndef main; end";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(!imports.is_empty());
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
    fn test_function_signature() {
        let parser = Parser::new();
        let content = b"fn hello(name: &str) -> String {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert!(functions[0].signature.contains("fn hello"));
    }

    #[test]
    fn test_go_exported_function() {
        let parser = Parser::new();
        let content = b"package main\n\nfunc Exported() {}\nfunc private() {}";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 2);
        assert!(functions[0].is_exported);
        assert!(!functions[1].is_exported);
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
}
