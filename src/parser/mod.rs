//! Tree-sitter based multi-language parser.

pub mod queries;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use crate::core::{Error, Language, Result, SourceFile};

/// Thread-safe parser pool for multi-language parsing.
pub struct Parser {
    /// Cached parsers per language.
    parsers: Mutex<HashMap<Language, TsParser>>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {
            parsers: Mutex::new(HashMap::new()),
        }
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

        let tree = {
            let mut parsers = self.parsers.lock();
            let parser = parsers.entry(lang).or_insert_with(|| {
                let mut p = TsParser::new();
                p.set_language(&ts_lang).expect("Language should be valid");
                p
            });

            parser.parse(content, None).ok_or_else(|| Error::Parse {
                path: path.to_path_buf(),
                message: "Failed to parse file".to_string(),
            })?
        };

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
}
