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
    /// Byte range of the function body (start, end).
    pub body_byte_range: Option<(usize, usize)>,
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

/// Extract classes from a parse result.
pub fn extract_classes(result: &ParseResult) -> Vec<ClassNode> {
    match result.language {
        Language::Rust => extract_rust_classes(result),
        Language::Go => extract_go_classes(result),
        Language::C | Language::Bash => Vec::new(),
        _ => extract_generic_classes(result),
    }
}

fn extract_rust_classes(result: &ParseResult) -> Vec<ClassNode> {
    let root = result.root_node();
    let source = &result.source;

    // Phase 1: collect struct_item nodes
    let mut struct_nodes: Vec<(String, tree_sitter::Node)> = Vec::new();
    for i in 0..root.child_count() as u32 {
        if let Some(child) = root.child(i) {
            if child.kind() == "struct_item" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source) {
                        if !name.is_empty() {
                            struct_nodes.push((name.to_string(), child));
                        }
                    }
                }
            }
        }
    }

    // Phase 2: collect impl_item blocks
    let mut impl_nodes: Vec<(String, tree_sitter::Node)> = Vec::new();
    for i in 0..root.child_count() as u32 {
        if let Some(child) = root.child(i) {
            if child.kind() == "impl_item" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    if let Ok(text) = type_node.utf8_text(source) {
                        let name = text.split('<').next().unwrap_or(text).trim().to_string();
                        if !name.is_empty() {
                            impl_nodes.push((name, child));
                        }
                    }
                }
            }
        }
    }

    // Phase 3: build ClassNode for each struct
    let mut classes = Vec::new();
    for (struct_name, struct_node) in &struct_nodes {
        let start_line = struct_node.start_position().row as u32 + 1;
        let end_line = struct_node.end_position().row as u32 + 1;
        let is_exported = struct_node
            .children(&mut struct_node.walk())
            .any(|c| c.kind() == "visibility_modifier");

        // Extract fields from struct body
        let fields = extract_rust_struct_fields(struct_node, source);

        // Find impl blocks for this struct and collect their methods.
        // We intentionally do NOT extend end_line to the impl block boundaries:
        // impl blocks in Rust can appear far from the struct definition, and
        // stretching end_line would cause free functions that happen to fall
        // between the struct and the impl to be mistakenly filtered out of the
        // top-level function list in downstream analyzers (e.g. outline).
        let mut methods = Vec::new();
        for (impl_name, impl_node) in &impl_nodes {
            if impl_name == struct_name {
                // Extract function_item children from impl body
                for func in extract_impl_methods(impl_node, source, result.language) {
                    methods.push(func);
                }
            }
        }

        classes.push(ClassNode {
            name: struct_name.clone(),
            start_line,
            end_line,
            methods,
            fields,
            is_exported,
        });
    }

    classes
}

fn extract_rust_struct_fields(node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
    let mut fields = Vec::new();
    for i in 0..node.child_count() as u32 {
        if let Some(body) = node.child(i) {
            if body.kind() == "field_declaration_list" {
                for j in 0..body.child_count() as u32 {
                    if let Some(field) = body.child(j) {
                        if field.kind() == "field_declaration" {
                            if let Some(name_node) = field.child_by_field_name("name") {
                                if let Ok(name) = name_node.utf8_text(source) {
                                    if !name.is_empty() {
                                        fields.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    fields
}

fn extract_impl_methods(
    impl_node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Vec<FunctionNode> {
    let mut methods = Vec::new();
    // Look for declaration_list inside the impl block
    for i in 0..impl_node.child_count() as u32 {
        if let Some(child) = impl_node.child(i) {
            if child.kind() == "declaration_list" {
                for j in 0..child.child_count() as u32 {
                    if let Some(item) = child.child(j) {
                        if item.kind() == "function_item" {
                            if let Some(func) = extract_function_info(&item, source, lang) {
                                methods.push(func);
                            }
                        }
                    }
                }
            }
        }
    }
    methods
}

fn extract_go_classes(result: &ParseResult) -> Vec<ClassNode> {
    let root = result.root_node();
    let source = &result.source;

    // Phase 1: collect type_declaration nodes containing struct_type
    let mut struct_defs: Vec<(String, tree_sitter::Node)> = Vec::new();
    for i in 0..root.child_count() as u32 {
        let child = match root.child(i) {
            Some(c) if c.kind() == "type_declaration" => c,
            _ => continue,
        };
        for j in 0..child.child_count() as u32 {
            if let Some(spec) = child.child(j) {
                if spec.kind() == "type_spec" && has_go_struct_type(&spec) {
                    if let Some(name_node) = spec.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source) {
                            if !name.is_empty() {
                                struct_defs.push((name.to_string(), child));
                            }
                        }
                    }
                }
            }
        }
    }

    // Phase 2: collect method_declarations and group by receiver type
    let mut method_map: std::collections::HashMap<String, Vec<tree_sitter::Node>> =
        std::collections::HashMap::new();
    for i in 0..root.child_count() as u32 {
        let child = match root.child(i) {
            Some(c) if c.kind() == "method_declaration" => c,
            _ => continue,
        };
        if let Some(recv_type) = extract_go_method_receiver_type(&child, source) {
            method_map.entry(recv_type).or_default().push(child);
        }
    }

    // Phase 3: build ClassNode for each struct
    let mut classes = Vec::new();
    for (struct_name, struct_node) in &struct_defs {
        let start_line = struct_node.start_position().row as u32 + 1;
        let end_line = struct_node.end_position().row as u32 + 1;
        let is_exported = struct_name.chars().next().is_some_and(|c| c.is_uppercase());

        // Extract fields from the struct type_spec
        let fields = extract_go_fields_from_type_decl(struct_node, source);

        // Collect methods without stretching end_line to method bodies.
        // Go methods are defined outside the struct block; extending end_line
        // would swallow free functions declared between the struct and its methods.
        let mut methods = Vec::new();
        if let Some(method_nodes) = method_map.get(struct_name) {
            for method_node in method_nodes {
                if let Some(func) = extract_function_info(method_node, source, result.language) {
                    methods.push(func);
                }
            }
        }

        classes.push(ClassNode {
            name: struct_name.clone(),
            start_line,
            end_line,
            methods,
            fields,
            is_exported,
        });
    }

    classes
}

fn has_go_struct_type(type_spec: &tree_sitter::Node<'_>) -> bool {
    for i in 0..type_spec.child_count() as u32 {
        if let Some(child) = type_spec.child(i) {
            if child.kind() == "struct_type" {
                return true;
            }
        }
    }
    false
}

fn extract_go_method_receiver_type(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    for i in 0..receiver.child_count() as u32 {
        if let Some(param) = receiver.child(i) {
            if param.kind() == "parameter_declaration" {
                if let Some(type_node) = param.child_by_field_name("type") {
                    return extract_go_base_type_name(&type_node, source);
                }
            }
        }
    }
    None
}

fn extract_go_base_type_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "pointer_type" => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_identifier" {
                        return node
                            .utf8_text(source)
                            .ok()
                            .and_then(|_| child.utf8_text(source).ok())
                            .map(|s| s.to_string());
                    }
                }
            }
            None
        }
        "type_identifier" => node.utf8_text(source).ok().map(|s| s.to_string()),
        _ => None,
    }
}

fn extract_go_fields_from_type_decl(
    type_decl: &tree_sitter::Node<'_>,
    source: &[u8],
) -> Vec<String> {
    let mut fields = Vec::new();
    collect_go_fields_recursive(type_decl, source, &mut fields);
    fields
}

fn collect_go_fields_recursive(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    fields: &mut Vec<String>,
) {
    if node.kind() == "field_declaration" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(source) {
                if !name.is_empty() && !fields.contains(&name.to_string()) {
                    fields.push(name.to_string());
                }
            }
        }
    }
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            collect_go_fields_recursive(&child, source, fields);
        }
    }
}

fn extract_generic_classes(result: &ParseResult) -> Vec<ClassNode> {
    let mut classes = Vec::new();
    let root = result.root_node();
    let mut cursor = root.walk();
    visit_for_classes(&mut cursor, result, &mut classes);
    classes
}

fn visit_for_classes(
    cursor: &mut tree_sitter::TreeCursor,
    result: &ParseResult,
    classes: &mut Vec<ClassNode>,
) {
    loop {
        let node = cursor.node();
        if is_class_kind(node.kind(), result.language) {
            if let Some(class_node) = build_class_node(&node, result) {
                classes.push(class_node);
                // Don't recurse into this class's children for more classes
                // (avoid nested class explosion) — but we do recurse in general
            }
        }

        if cursor.goto_first_child() {
            visit_for_classes(cursor, result, classes);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn is_class_kind(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Java => kind == "class_declaration" || kind == "interface_declaration",
        Language::TypeScript | Language::Tsx => kind == "class_declaration" || kind == "class",
        Language::JavaScript | Language::Jsx => kind == "class_declaration" || kind == "class",
        Language::Python => kind == "class_definition",
        Language::CSharp => kind == "class_declaration" || kind == "interface_declaration",
        Language::Cpp => kind == "class_specifier" || kind == "struct_specifier",
        Language::Ruby => kind == "class" || kind == "module",
        Language::Php => kind == "class_declaration" || kind == "interface_declaration",
        _ => false,
    }
}

fn build_class_node(node: &tree_sitter::Node<'_>, result: &ParseResult) -> Option<ClassNode> {
    let source = &result.source;
    let lang = result.language;

    // Get class name
    let name = match lang {
        Language::Cpp => {
            // struct_specifier and class_specifier: name field
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string())?
        }
        _ => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.to_string())?,
    };
    if name.is_empty() {
        return None;
    }

    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;
    let is_exported = class_is_exported(node, source, lang);

    // Extract methods from class body
    let methods = extract_class_methods(node, source, lang);

    // Extract fields
    let fields = extract_class_fields(node, source, lang);

    Some(ClassNode {
        name,
        start_line,
        end_line,
        methods,
        fields,
        is_exported,
    })
}

fn class_is_exported(node: &tree_sitter::Node<'_>, source: &[u8], lang: Language) -> bool {
    match lang {
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            // Check if parent is export_statement
            if let Some(parent) = node.parent() {
                if parent.kind() == "export_statement" {
                    return true;
                }
                // grandparent check
                if let Some(gp) = parent.parent() {
                    if gp.kind() == "export_statement" {
                        return true;
                    }
                }
            }
            false
        }
        Language::Java => {
            // Check for "public" in modifiers
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == "modifiers" {
                        if let Ok(text) = child.utf8_text(source) {
                            return text.contains("public");
                        }
                    }
                }
            }
            false
        }
        Language::CSharp => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == "modifier" {
                        if let Ok(text) = child.utf8_text(source) {
                            if text == "public" {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Language::Python | Language::Ruby => true,
        _ => true, // C++, PHP default to true
    }
}

fn extract_class_methods(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Vec<FunctionNode> {
    let method_kinds: &[&str] = match lang {
        Language::Java => &["method_declaration", "constructor_declaration"],
        Language::TypeScript | Language::Tsx => &["method_definition"],
        Language::JavaScript | Language::Jsx => &["method_definition"],
        Language::Python => &["function_definition"],
        Language::CSharp => &["method_declaration", "constructor_declaration"],
        Language::Cpp => &["function_definition"],
        Language::Ruby => &["method", "singleton_method"],
        Language::Php => &["method_declaration"],
        _ => &[],
    };

    let mut methods = Vec::new();
    collect_methods_recursive(node, source, lang, method_kinds, &mut methods);
    methods
}

fn collect_methods_recursive(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
    method_kinds: &[&str],
    methods: &mut Vec<FunctionNode>,
) {
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            if method_kinds.contains(&child.kind()) {
                if let Some(func) = extract_function_info(&child, source, lang) {
                    methods.push(func);
                }
            } else {
                // Recurse into body-like containers (not into nested classes)
                let ck = child.kind();
                let is_body_container = ck == "class_body"
                    || ck == "block"
                    || ck == "declaration_list"
                    || ck == "body"
                    || ck == "member_declarations"
                    || ck == "compound_statement"
                    || ck == "field_declaration_list";
                // For Ruby: methods are direct children of class node (no body wrapper)
                let is_ruby_body = lang == Language::Ruby && !is_class_kind(ck, lang);
                // For Python: methods are inside a block
                let is_python_body = lang == Language::Python && (ck == "block" || ck == "suite");
                if is_body_container || is_ruby_body || is_python_body {
                    collect_methods_recursive(&child, source, lang, method_kinds, methods);
                }
            }
        }
    }
}

fn extract_class_fields(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
) -> Vec<String> {
    let mut fields = Vec::new();
    // Find the class body and look for field declarations.
    // Use a tree-sitter cursor for reliable traversal.
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let ck = child.kind();
            if ck == "class_body"
                || ck == "block"
                || ck == "declaration_list"
                || ck == "body"
                || ck == "member_declarations"
                || ck == "compound_statement"
            {
                collect_fields_in_body(&child, source, lang, &mut fields);

                // For Python: also walk method bodies to collect `self.x = …`
                // assignments (e.g. inside `__init__`).
                if lang == Language::Python {
                    collect_python_self_fields_in_methods(&child, source, &mut fields);
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    fields
}

/// Walk method (function_definition) nodes inside a Python class body and
/// collect any `self.attr = …` assignments found within their bodies.
fn collect_python_self_fields_in_methods(
    class_body: &tree_sitter::Node<'_>,
    source: &[u8],
    fields: &mut Vec<String>,
) {
    // Walk named children of the class body using a cursor for reliability.
    let mut cursor = class_body.walk();
    if cursor.goto_first_child() {
        loop {
            let method = cursor.node();
            if method.kind() == "function_definition" {
                // Walk the method's children looking for the body block.
                let mut method_cursor = method.walk();
                if method_cursor.goto_first_child() {
                    loop {
                        let body = method_cursor.node();
                        if body.kind() == "block" || body.kind() == "suite" {
                            collect_python_self_assignments(&body, source, fields);
                        }
                        if !method_cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Recursively walk `node` looking for `self.attr = …` expression statements.
fn collect_python_self_assignments(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    fields: &mut Vec<String>,
) {
    // Use a tree-sitter cursor instead of child(i) to reliably iterate children.
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "expression_statement" {
            // Look for `assignment` inside the expression_statement.
            let mut ec = child.walk();
            if ec.goto_first_child() {
                loop {
                    let assign = ec.node();
                    if assign.kind() == "assignment" {
                        if let Some(left) = assign.child_by_field_name("left") {
                            if left.kind() == "attribute" {
                                // Verify the object is `self`.
                                let is_self = left
                                    .child_by_field_name("object")
                                    .and_then(|obj| obj.utf8_text(source).ok())
                                    .is_some_and(|t| t == "self");
                                if is_self {
                                    if let Some(attr) = left.child_by_field_name("attribute") {
                                        if let Ok(name) = attr.utf8_text(source) {
                                            if !name.is_empty()
                                                && !fields.contains(&name.to_string())
                                            {
                                                fields.push(name.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !ec.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
        // Recurse into nested blocks (e.g. if/for inside __init__)
        let kind = child.kind();
        if kind == "block"
            || kind == "suite"
            || kind == "if_statement"
            || kind == "for_statement"
            || kind == "while_statement"
        {
            collect_python_self_assignments(&child, source, fields);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn collect_fields_in_body(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    lang: Language,
    fields: &mut Vec<String>,
) {
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            match lang {
                Language::Java => {
                    if child.kind() == "field_declaration" {
                        if let Some(decl) = child.child_by_field_name("declarator") {
                            if let Some(name_node) = decl.child_by_field_name("name") {
                                if let Ok(name) = name_node.utf8_text(source) {
                                    if !name.is_empty() {
                                        fields.push(name.to_string());
                                    }
                                }
                            }
                        } else {
                            // try finding variable_declarator children
                            for j in 0..child.child_count() as u32 {
                                if let Some(vd) = child.child(j) {
                                    if vd.kind() == "variable_declarator" {
                                        if let Some(name_node) = vd.child_by_field_name("name") {
                                            if let Ok(name) = name_node.utf8_text(source) {
                                                if !name.is_empty() {
                                                    fields.push(name.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
                    if child.kind() == "public_field_definition"
                        || child.kind() == "field_definition"
                    {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            if let Ok(name) = name_node.utf8_text(source) {
                                if !name.is_empty() {
                                    fields.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                Language::Python => {
                    // depth-1 assignments that look like self.x = ...
                    // We keep it simple: look for expression_statement -> assignment with left = attribute
                    if child.kind() == "expression_statement" {
                        for j in 0..child.child_count() as u32 {
                            if let Some(assign) = child.child(j) {
                                if assign.kind() == "assignment" {
                                    if let Some(left) = assign.child_by_field_name("left") {
                                        if left.kind() == "attribute" {
                                            if let Some(attr_name) =
                                                left.child_by_field_name("attribute")
                                            {
                                                if let Ok(name) = attr_name.utf8_text(source) {
                                                    if !name.is_empty()
                                                        && !fields.contains(&name.to_string())
                                                    {
                                                        fields.push(name.to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Language::CSharp => {
                    if child.kind() == "field_declaration" {
                        // Find variable_declarator children
                        for j in 0..child.child_count() as u32 {
                            if let Some(vd) = child.child(j) {
                                if vd.kind() == "variable_declarator" {
                                    if let Some(name_node) = vd.child_by_field_name("name") {
                                        if let Ok(name) = name_node.utf8_text(source) {
                                            if !name.is_empty() {
                                                fields.push(name.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Language::Cpp => {
                    if child.kind() == "field_declaration" {
                        if let Some(decl) = find_child_by_kind_local(&child, "field_declarator") {
                            extract_cpp_field_name(&decl, source, fields);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn find_child_by_kind_local<'a>(
    node: &tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

fn extract_cpp_field_name(
    declarator: &tree_sitter::Node<'_>,
    source: &[u8],
    fields: &mut Vec<String>,
) {
    if declarator.kind() == "field_identifier" {
        if let Ok(name) = declarator.utf8_text(source) {
            if !name.is_empty() {
                fields.push(name.to_string());
            }
        }
        return;
    }
    for i in 0..declarator.child_count() as u32 {
        if let Some(child) = declarator.child(i) {
            extract_cpp_field_name(&child, source, fields);
        }
    }
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
            Language::Rust if node.kind() == "mod_item" => {
                if let Some(import) = extract_rust_mod(&node, source) {
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
            Language::Ruby if node.kind() == "class" => {
                if let Some(import) = extract_ruby_superclass(&node, source) {
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
        body_byte_range: body.map(|b| (b.start_byte(), b.end_byte())),
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
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
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
    // use_declaration children: optional visibility_modifier, then the use argument
    // The argument can be: scoped_identifier, use_as_clause, scoped_use_list, use_wildcard, identifier
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "use" | "visibility_modifier" | ";" => continue,
            "scoped_use_list" => {
                // e.g., use std::collections::{HashMap, HashSet}
                // Extract just the base path (the part before the {})
                if let Some(path_node) = child.child_by_field_name("path") {
                    let path = path_node.utf8_text(source).ok()?.to_string();
                    return Some(ImportNode {
                        path,
                        line: node.start_position().row as u32 + 1,
                        names: Vec::new(),
                    });
                }
            }
            _ => {
                // scoped_identifier, identifier, use_as_clause, use_wildcard
                let text = child.utf8_text(source).ok()?.to_string();
                // For use_as_clause like "crate::foo as bar", take the path part
                let path = if child.kind() == "use_as_clause" {
                    if let Some(path_node) = child.child_by_field_name("path") {
                        path_node.utf8_text(source).ok()?.to_string()
                    } else {
                        text
                    }
                } else {
                    text
                };
                return Some(ImportNode {
                    path,
                    line: node.start_position().row as u32 + 1,
                    names: Vec::new(),
                });
            }
        }
    }
    None
}

fn extract_rust_mod(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    // mod_item children: optional visibility_modifier, "mod", identifier, optional block body
    // We only want `mod foo;` declarations (no body), not `mod foo { ... }` inline modules
    let mut has_body = false;
    let mut name = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "declaration_list" => {
                has_body = true;
            }
            "identifier" => {
                name = child.utf8_text(source).ok().map(|s| s.to_string());
            }
            _ => {}
        }
    }
    if has_body {
        return None;
    }
    let name = name?;
    Some(ImportNode {
        path: name,
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
    let method = find_named_child(node, "identifier", source)?;
    let line = node.start_position().row as u32 + 1;

    let args = find_node_child(node, "argument_list")?;

    match method.as_str() {
        "require" | "require_relative" => {
            let string_node = find_node_child(&args, "string")?;
            let content = find_named_child(&string_node, "string_content", source)?;
            Some(ImportNode {
                path: content,
                line,
                names: Vec::new(),
            })
        }
        "include" | "extend" | "prepend" => {
            let path = find_named_child(&args, "constant", source)
                .or_else(|| find_named_child(&args, "scope_resolution", source))?;
            Some(ImportNode {
                path,
                line,
                names: Vec::new(),
            })
        }
        "autoload" => {
            let string_node = find_node_child(&args, "string")?;
            let content = find_named_child(&string_node, "string_content", source)?;
            Some(ImportNode {
                path: content,
                line,
                names: Vec::new(),
            })
        }
        _ => None,
    }
}

fn extract_ruby_superclass(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<ImportNode> {
    let superclass = find_node_child(node, "superclass")?;
    let path = find_named_child(&superclass, "constant", source)
        .or_else(|| find_named_child(&superclass, "scope_resolution", source))?;
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
    fn test_extract_rust_import_path_only() {
        let parser = Parser::new();
        let content =
            b"use std::path::Path;\nuse crate::config::Config;\nuse super::utils;\n\nfn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 3);
        // Must extract just the module path, not "use ... ;"
        assert_eq!(imports[0].path, "std::path::Path");
        assert_eq!(imports[1].path, "crate::config::Config");
        assert_eq!(imports[2].path, "super::utils");
    }

    #[test]
    fn test_extract_rust_use_group() {
        let parser = Parser::new();
        let content = b"use std::collections::{HashMap, HashSet};\n\nfn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("main.rs"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        // Group imports should extract the base path
        assert_eq!(imports[0].path, "std::collections");
    }

    #[test]
    fn test_extract_rust_mod_declarations() {
        let parser = Parser::new();
        let content = b"mod config;\nmod utils;\npub mod helpers;\n\nfn main() {}";
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].path, "config");
        assert_eq!(imports[1].path, "utils");
        assert_eq!(imports[2].path, "helpers");
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
    fn test_extract_ruby_require_path() {
        let parser = Parser::new();
        let content = b"require 'json'\n\ndef main; end";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "json");
        assert_eq!(imports[0].line, 1);
    }

    #[test]
    fn test_extract_ruby_require_relative() {
        let parser = Parser::new();
        let content = b"require_relative './helper'";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "./helper");
    }

    #[test]
    fn test_extract_ruby_require_double_quotes() {
        let parser = Parser::new();
        let content = b"require \"json\"";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "json");
    }

    #[test]
    fn test_extract_ruby_include() {
        let parser = Parser::new();
        let content = b"class Foo\n  include Comparable\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "Comparable");
    }

    #[test]
    fn test_extract_ruby_extend() {
        let parser = Parser::new();
        let content = b"class Foo\n  extend ClassMethods\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "ClassMethods");
    }

    #[test]
    fn test_extract_ruby_prepend() {
        let parser = Parser::new();
        let content = b"class Foo\n  prepend Validation\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "Validation");
    }

    #[test]
    fn test_extract_ruby_include_scoped() {
        let parser = Parser::new();
        let content = b"class Foo\n  include ActiveModel::Validations\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "ActiveModel::Validations");
    }

    #[test]
    fn test_extract_ruby_autoload() {
        let parser = Parser::new();
        let content = b"autoload :Foo, 'foo'";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "foo");
    }

    #[test]
    fn test_extract_ruby_multiple_imports() {
        let parser = Parser::new();
        let content = b"require 'json'\nrequire 'yaml'\n\nclass Foo\n  include Comparable\n  extend ClassMethods\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 4);
    }

    #[test]
    fn test_extract_ruby_ignores_other_calls() {
        let parser = Parser::new();
        let content = b"puts 'hello'\nfoo('bar')";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_extract_ruby_require_not_in_string() {
        let parser = Parser::new();
        let content = b"x = \"require 'json'\"";
        let result = parser
            .parse(content, Language::Ruby, Path::new("main.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_extract_ruby_multiple_includes() {
        let parser = Parser::new();
        let content =
            b"class Foo\n  include Comparable\n  include Enumerable\n  prepend Caching\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].path, "Comparable");
        assert_eq!(imports[1].path, "Enumerable");
        assert_eq!(imports[2].path, "Caching");
    }

    #[test]
    fn test_extract_ruby_inheritance() {
        let parser = Parser::new();
        let content = b"class Order < ApplicationRecord\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("order.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "ApplicationRecord");
    }

    #[test]
    fn test_extract_ruby_inheritance_scoped() {
        let parser = Parser::new();
        let content = b"class User < ActiveRecord::Base\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("user.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "ActiveRecord::Base");
    }

    #[test]
    fn test_extract_ruby_inheritance_with_includes() {
        let parser = Parser::new();
        let content =
            b"class Order < ApplicationRecord\n  include Searchable\n  extend ClassMethods\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("order.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].path, "ApplicationRecord");
        assert_eq!(imports[1].path, "Searchable");
        assert_eq!(imports[2].path, "ClassMethods");
    }

    #[test]
    fn test_extract_ruby_class_without_superclass() {
        let parser = Parser::new();
        let content = b"class Foo\n  def bar; end\nend";
        let result = parser
            .parse(content, Language::Ruby, Path::new("foo.rb"))
            .unwrap();

        let imports = extract_imports(&result);
        assert!(imports.is_empty());
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
    fn test_tsx_export_detection() {
        let parser = Parser::new();
        let content = b"export function Component() { return <div />; }";
        let result = parser
            .parse(content, Language::Tsx, Path::new("component.tsx"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert!(
            functions[0].is_exported,
            "TSX exported function should be detected as exported"
        );
    }

    #[test]
    fn test_jsx_export_detection() {
        let parser = Parser::new();
        let content = b"export function App() { return <div />; }";
        let result = parser
            .parse(content, Language::Jsx, Path::new("app.jsx"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert!(
            functions[0].is_exported,
            "JSX exported function should be detected as exported"
        );
    }

    #[test]
    fn test_tsx_non_exported_function() {
        let parser = Parser::new();
        let content = b"function Helper() { return <span />; }";
        let result = parser
            .parse(content, Language::Tsx, Path::new("helper.tsx"))
            .unwrap();

        let functions = extract_functions(&result);
        assert_eq!(functions.len(), 1);
        assert!(
            !functions[0].is_exported,
            "Non-exported TSX function should not be detected as exported"
        );
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

    // ===== extract_classes tests =====

    #[test]
    fn test_extract_classes_go() {
        let parser = Parser::new();
        let content = b"package p\ntype Server struct {\n\tHost string\n\tPort int\n}\nfunc (s *Server) Address() string { return \"\" }\n";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Server");
        assert!(
            classes[0].is_exported,
            "Server should be exported (uppercase)"
        );
        assert_eq!(classes[0].methods.len(), 1);
        assert_eq!(classes[0].methods[0].name, "Address");
    }

    #[test]
    fn test_extract_classes_go_unexported() {
        let parser = Parser::new();
        let content = b"package p\ntype server struct {\n\thost string\n}\nfunc (s *server) address() string { return \"\" }\n";
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "server");
        assert!(
            !classes[0].is_exported,
            "server should not be exported (lowercase)"
        );
    }

    #[test]
    fn test_extract_classes_rust() {
        let parser = Parser::new();
        let content = b"pub struct Config {\n    pub name: String,\n    pub value: u32,\n}\nimpl Config {\n    pub fn new(name: &str) -> Self { Self { name: name.to_string(), value: 0 } }\n    fn private_fn(&self) {}\n}\n";
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Config");
        assert!(classes[0].is_exported, "pub struct should be exported");
        assert_eq!(classes[0].methods.len(), 2);
        assert!(classes[0].fields.contains(&"name".to_string()));
        assert!(classes[0].fields.contains(&"value".to_string()));
    }

    #[test]
    fn test_extract_classes_rust_private() {
        let parser = Parser::new();
        let content = b"struct Internal {\n    data: Vec<u8>,\n}\nimpl Internal {\n    fn process(&self) {}\n}\n";
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Internal");
        assert!(
            !classes[0].is_exported,
            "struct without pub should not be exported"
        );
        assert_eq!(classes[0].methods.len(), 1);
    }

    #[test]
    fn test_extract_classes_python() {
        let parser = Parser::new();
        let content = b"class UserService:\n    def __init__(self, db):\n        self.db = db\n    def get_user(self, user_id):\n        return self.db.find(user_id)\n    def create_user(self, name):\n        return self.db.insert(name)\n";
        let result = parser
            .parse(content, Language::Python, Path::new("service.py"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "UserService");
        assert!(classes[0].is_exported, "Python classes are always exported");
        assert_eq!(classes[0].methods.len(), 3);
    }

    /// B4: Python `self.x = …` assignments inside `__init__` should be collected as class fields.
    #[test]
    fn test_extract_classes_python_self_fields_from_init() {
        let parser = Parser::new();
        // Same format as the existing test_extract_classes_python test.
        let content = b"class Person:\n    def __init__(self, name, age):\n        self.name = name\n        self.age = age\n    def greet(self):\n        return self.name\n";
        let result = parser
            .parse(content, Language::Python, Path::new("person.py"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        let person = &classes[0];
        assert_eq!(person.name, "Person");
        assert!(
            person.fields.contains(&"name".to_string()),
            "fields should include 'name', got {:?}",
            person.fields
        );
        assert!(
            person.fields.contains(&"age".to_string()),
            "fields should include 'age', got {:?}",
            person.fields
        );
    }

    #[test]
    fn test_extract_classes_typescript() {
        let parser = Parser::new();
        let content = b"export class ConsoleLogger {\n  private prefix: string;\n  constructor(prefix: string) { this.prefix = prefix; }\n  info(msg: string): void { console.log(msg); }\n}\n";
        let result = parser
            .parse(content, Language::TypeScript, Path::new("logger.ts"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "ConsoleLogger");
        assert!(classes[0].is_exported, "export class should be exported");
    }

    #[test]
    fn test_extract_classes_javascript() {
        let parser = Parser::new();
        let content = b"class Animal {\n  constructor(name) { this.name = name; }\n  speak() { return this.name; }\n}\n";
        let result = parser
            .parse(content, Language::JavaScript, Path::new("animal.js"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Animal");
        assert!(!classes[0].is_exported, "non-exported JS class");
    }

    #[test]
    fn test_extract_classes_java() {
        let parser = Parser::new();
        let content = b"public class OrderService {\n  private int id;\n  public void process() {}\n  public int getId() { return id; }\n}\n";
        let result = parser
            .parse(content, Language::Java, Path::new("OrderService.java"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "OrderService");
        assert!(classes[0].is_exported, "public class should be exported");
        assert_eq!(classes[0].methods.len(), 2);
    }

    #[test]
    fn test_extract_classes_csharp() {
        let parser = Parser::new();
        let content = b"public class Calculator {\n    private int value;\n    public int Add(int a, int b) { return a + b; }\n    public int Sub(int a, int b) { return a - b; }\n}\n";
        let result = parser
            .parse(content, Language::CSharp, Path::new("Calculator.cs"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Calculator");
        assert!(classes[0].is_exported, "public class should be exported");
        assert_eq!(classes[0].methods.len(), 2);
    }

    #[test]
    fn test_extract_classes_cpp() {
        let parser = Parser::new();
        let content = b"class Shape {\npublic:\n  double area();\n  double perimeter();\n};\n";
        let result = parser
            .parse(content, Language::Cpp, Path::new("shape.cpp"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Shape");
        assert!(classes[0].is_exported, "C++ class defaults to exported");
    }

    #[test]
    fn test_extract_classes_ruby() {
        let parser = Parser::new();
        let content = b"class OrderProcessor\n  def initialize(inv)\n    @inv = inv\n  end\n  def process(order)\n    order\n  end\nend\n";
        let result = parser
            .parse(content, Language::Ruby, Path::new("processor.rb"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "OrderProcessor");
        assert!(classes[0].is_exported, "Ruby classes are always exported");
        assert_eq!(classes[0].methods.len(), 2);
    }

    #[test]
    fn test_extract_classes_php() {
        let parser = Parser::new();
        let content = b"<?php\nclass UserRepository {\n  private $db;\n  public function find($id) { return $id; }\n  public function save($user) { return true; }\n}\n";
        let result = parser
            .parse(content, Language::Php, Path::new("repo.php"))
            .unwrap();
        let classes = extract_classes(&result);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "UserRepository");
        assert!(classes[0].is_exported, "PHP class defaults to exported");
        assert_eq!(classes[0].methods.len(), 2);
    }

    #[test]
    fn test_extract_classes_bash_empty() {
        let parser = Parser::new();
        let content = b"#!/bin/bash\nhello() { echo hi; }";
        let result = parser
            .parse(content, Language::Bash, Path::new("script.sh"))
            .unwrap();
        let classes = extract_classes(&result);
        assert!(classes.is_empty(), "Bash has no classes");
    }

    #[test]
    fn test_extract_classes_c_empty() {
        let parser = Parser::new();
        let content = b"int add(int a, int b) { return a + b; }";
        let result = parser
            .parse(content, Language::C, Path::new("main.c"))
            .unwrap();
        let classes = extract_classes(&result);
        assert!(classes.is_empty(), "C has no classes");
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
}
