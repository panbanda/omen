//! Dead code detection analyzer.
//!
//! Finds unreachable/unused functions, variables, and classes using
//! reference graph analysis.
//!
//! ## Current Limitations
//!
//! TODO: Variable and class tracking is not yet implemented.
//! Currently only function definitions and usages are tracked.
//! Future work should include:
//! - Variable declarations (let, const, var, etc.)
//! - Class/struct definitions
//! - Module-level constants
//! - Type aliases

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::{self, Parser};

/// Dead code analyzer.
pub struct Analyzer {
    parser: Parser,
    confidence_threshold: f64,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            confidence_threshold: 0.8,
        }
    }

    pub fn with_confidence(mut self, threshold: f64) -> Self {
        self.confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Analyze a single file for definitions and usages.
    fn analyze_file(&self, path: &std::path::Path) -> Result<FileDeadCode> {
        let result = self.parser.parse_file(path)?;
        Ok(collect_file_data(&result))
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "deadcode"
    }

    fn description(&self) -> &'static str {
        "Find unreachable/unused functions and variables"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        // Check if this is a Rust project with Cargo.toml
        let cargo_toml = ctx.root.join("Cargo.toml");
        let is_rust_project = cargo_toml.exists();

        // For Rust projects, use cargo check for accurate dead code detection
        let cargo_items = if is_rust_project {
            match CargoDeadCodeAnalyzer::analyze(ctx.root) {
                Ok(analysis) => analysis.items,
                Err(e) => {
                    tracing::warn!("Cargo analysis failed, falling back to tree-sitter: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Phase 1: Collect definitions and usages from all files
        // For Rust projects, skip .rs files since cargo handles them
        let files: Vec<_> = ctx
            .files
            .iter()
            .filter(|path| {
                if is_rust_project {
                    // Skip Rust files - cargo handles them more accurately
                    path.extension().map(|e| e != "rs").unwrap_or(true)
                } else {
                    true
                }
            })
            .collect();
        let file_results: Vec<FileDeadCode> = files
            .par_iter()
            .filter_map(|path| self.analyze_file(path).ok())
            .collect();

        // Phase 2: Build global symbol tables with qualified names
        // Use file::function_name format to prevent collisions when multiple files
        // have functions with the same name.
        let mut all_definitions: HashMap<String, Definition> = HashMap::new();
        let mut all_usages: HashSet<String> = HashSet::new();
        let mut all_calls: Vec<CallReference> = Vec::new();
        // Map from simple name to qualified names for cross-file call resolution
        let mut name_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

        for fdc in &file_results {
            for (name, def) in &fdc.definitions {
                let qualified_name = format!("{}::{}", def.file, name);
                all_definitions.insert(qualified_name.clone(), def.clone());
                name_to_qualified
                    .entry(name.clone())
                    .or_default()
                    .push(qualified_name);
            }
            all_usages.extend(fdc.usages.iter().cloned());
            all_calls.extend(fdc.calls.iter().cloned());
        }

        // Phase 3: Build call graph edges (for reachability) using qualified names
        let mut call_graph: HashMap<String, Vec<String>> = HashMap::new();
        for call in &all_calls {
            // Qualify the caller with its file path
            let qualified_caller = format!("{}::{}", call.file, call.caller);

            // For the callee, first try same-file lookup, then cross-file
            let qualified_callees: Vec<String> = {
                let same_file_qualified = format!("{}::{}", call.file, call.callee);
                if all_definitions.contains_key(&same_file_qualified) {
                    vec![same_file_qualified]
                } else if let Some(qualified_names) = name_to_qualified.get(&call.callee) {
                    // Cross-file call: could be any of the matching functions
                    qualified_names.clone()
                } else {
                    vec![]
                }
            };

            for qualified_callee in qualified_callees {
                call_graph
                    .entry(qualified_caller.clone())
                    .or_default()
                    .push(qualified_callee);
            }
        }

        // Phase 4: Mark reachable from entry points (using qualified names)
        let mut reachable: HashSet<String> = HashSet::new();
        let mut queue: Vec<String> = Vec::new();

        // Identify entry points using qualified names
        for (qualified_name, def) in &all_definitions {
            // Extract simple name from qualified name for entry point check
            let simple_name = qualified_name.rsplit("::").next().unwrap_or(qualified_name);
            if is_entry_point(simple_name, def) {
                reachable.insert(qualified_name.clone());
                queue.push(qualified_name.clone());
            }
        }

        // BFS to mark reachable (all names are qualified now)
        while let Some(current) = queue.pop() {
            if let Some(callees) = call_graph.get(&current) {
                for callee in callees {
                    if all_definitions.contains_key(callee) && !reachable.contains(callee) {
                        reachable.insert(callee.clone());
                        queue.push(callee.clone());
                    }
                }
            }
        }

        // Phase 5: Classify dead code (using qualified names)
        let mut items = Vec::new();
        let mut by_kind: HashMap<String, usize> = HashMap::new();

        // Add cargo-detected dead code items (Rust files)
        for cargo_item in cargo_items {
            let item = DeadCodeItem {
                name: cargo_item.name,
                kind: cargo_item.kind.clone(),
                file: cargo_item.file,
                line: cargo_item.line,
                end_line: cargo_item.end_line,
                visibility: "unknown".to_string(),
                confidence: 1.0, // Cargo/rustc is authoritative
                reason: cargo_item.message,
            };
            *by_kind.entry(cargo_item.kind).or_insert(0) += 1;
            items.push(item);
        }

        // Add tree-sitter detected dead code (non-Rust files)
        for (qualified_name, def) in &all_definitions {
            // Extract simple name for entry point check and usage lookup
            let simple_name = qualified_name.rsplit("::").next().unwrap_or(qualified_name);

            // Skip entry points
            if is_entry_point(simple_name, def) {
                continue;
            }

            // Check if unreachable (using qualified name) AND not used (simple name in usages)
            let is_unreachable = !reachable.contains(qualified_name);
            let is_unused = !all_usages.contains(simple_name);

            if is_unreachable || is_unused {
                let confidence = calculate_confidence(def, is_unreachable, is_unused);

                if confidence >= self.confidence_threshold {
                    let item = DeadCodeItem {
                        name: simple_name.to_string(),
                        kind: def.kind.clone(),
                        file: def.file.clone(),
                        line: def.line,
                        end_line: def.end_line,
                        visibility: def.visibility.clone(),
                        confidence,
                        reason: if is_unreachable && is_unused {
                            "Not reachable from entry points and no references found".to_string()
                        } else if is_unreachable {
                            "Not reachable from entry points".to_string()
                        } else {
                            "No references found in codebase".to_string()
                        },
                    };

                    *by_kind.entry(def.kind.clone()).or_insert(0) += 1;
                    items.push(item);
                }
            }
        }

        let total_items = items.len();
        let analysis = Analysis {
            items,
            summary: AnalysisSummary {
                total_items,
                by_kind,
                total_definitions: all_definitions.len(),
                reachable_count: reachable.len(),
            },
        };

        tracing::info!(
            "Deadcode analysis completed in {:?}: {} items found",
            start.elapsed(),
            analysis.summary.total_items
        );

        Ok(analysis)
    }
}

/// Collect definitions and usages from a parsed file.
fn collect_file_data(result: &parser::ParseResult) -> FileDeadCode {
    let functions = parser::extract_functions(result);
    let mut fdc = FileDeadCode {
        path: result.path.to_string_lossy().to_string(),
        definitions: HashMap::new(),
        usages: HashSet::new(),
        calls: Vec::new(),
    };

    let is_test_file = is_test_file(&fdc.path);

    // For Rust, extract function attributes and context from the AST
    let function_info = if result.language == Language::Rust {
        extract_rust_function_attributes(result)
    } else {
        HashMap::new()
    };

    // Extract function definitions
    for func in functions {
        let visibility = get_visibility(&func.name, result.language);
        // Use the parser's is_exported which correctly checks for pub in Rust
        let exported = func.is_exported || is_exported(&func.name, result.language);
        let info = function_info.get(&func.name);
        let attributes = info.map(|i| i.attributes.clone()).unwrap_or_default();
        // Mark as test file if already in test file OR inside #[cfg(test)] module
        let is_in_test_context =
            is_test_file || info.map(|i| i.in_cfg_test_module).unwrap_or(false);
        let is_trait_impl = info.map(|i| i.is_trait_impl).unwrap_or(false);

        fdc.definitions.insert(
            func.name.clone(),
            Definition {
                name: func.name.clone(),
                kind: "function".to_string(),
                file: fdc.path.clone(),
                line: func.start_line,
                end_line: func.end_line,
                visibility,
                exported,
                is_test_file: is_in_test_context,
                attributes,
                is_trait_impl,
            },
        );
    }

    // Extract usages and calls by walking the AST
    collect_usages_and_calls(result, &mut fdc);

    fdc
}

/// Info about a Rust function extracted from the AST.
struct RustFunctionInfo {
    attributes: Vec<String>,
    in_cfg_test_module: bool,
    is_trait_impl: bool,
}

/// Extract attributes and context for Rust functions.
/// Returns a map from function name to function info.
fn extract_rust_function_attributes(
    result: &parser::ParseResult,
) -> HashMap<String, RustFunctionInfo> {
    let mut info: HashMap<String, RustFunctionInfo> = HashMap::new();
    let root = result.root_node();
    let source = &result.source;

    // Walk the AST looking for function_item nodes
    fn visit(
        node: tree_sitter::Node<'_>,
        source: &[u8],
        info: &mut HashMap<String, RustFunctionInfo>,
        in_cfg_test: bool,
        in_trait_impl: bool,
    ) {
        let mut current_in_cfg_test = in_cfg_test;
        let mut current_in_trait_impl = in_trait_impl;

        // Check if this is a mod_item with #[cfg(test)] attribute
        if node.kind() == "mod_item" && has_cfg_test_attribute(&node, source) {
            current_in_cfg_test = true;
        }

        // Check if this is a trait impl block (impl Trait for Type)
        if node.kind() == "impl_item" {
            current_in_trait_impl = is_trait_impl_block(&node);
        }

        if node.kind() == "function_item" {
            // Get the function name
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(func_name) = name_node.utf8_text(source) {
                    // Look for preceding attribute_item siblings
                    let mut preceding_attrs = Vec::new();
                    let mut prev = node.prev_sibling();
                    while let Some(sibling) = prev {
                        if sibling.kind() == "attribute_item" {
                            if let Some(attr_name) = extract_attribute_name(&sibling, source) {
                                preceding_attrs.push(attr_name);
                            }
                        } else {
                            // Stop at non-attribute nodes
                            break;
                        }
                        prev = sibling.prev_sibling();
                    }
                    info.insert(
                        func_name.to_string(),
                        RustFunctionInfo {
                            attributes: preceding_attrs,
                            in_cfg_test_module: current_in_cfg_test,
                            is_trait_impl: current_in_trait_impl,
                        },
                    );
                }
            }
        }

        // Recurse into children with updated context
        for child in node.children(&mut node.walk()) {
            visit(
                child,
                source,
                info,
                current_in_cfg_test,
                current_in_trait_impl,
            );
        }
    }

    visit(root, source, &mut info, false, false);
    info
}

/// Check if an impl_item is a trait impl (impl Trait for Type) vs inherent impl (impl Type).
fn is_trait_impl_block(impl_node: &tree_sitter::Node<'_>) -> bool {
    // Trait impl has "for" keyword between the trait name and the type
    for child in impl_node.children(&mut impl_node.walk()) {
        if child.kind() == "for" {
            return true;
        }
    }
    false
}

/// Check if a mod_item has a preceding #[cfg(test)] attribute.
fn has_cfg_test_attribute(mod_node: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut prev = mod_node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "attribute_item" {
            // Check if this is #[cfg(test)]
            for child in sibling.children(&mut sibling.walk()) {
                if child.kind() == "attribute" {
                    if let Ok(text) = child.utf8_text(source) {
                        if text == "cfg(test)" {
                            return true;
                        }
                    }
                }
            }
        } else {
            // Stop at non-attribute nodes
            break;
        }
        prev = sibling.prev_sibling();
    }
    false
}

/// Extract the attribute name from an attribute_item node.
/// Handles both simple (#[test]) and path-based (#[tokio::test]) attributes.
fn extract_attribute_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    // Find the "attribute" child
    for child in node.children(&mut node.walk()) {
        if child.kind() == "attribute" {
            // The attribute can contain an identifier or a scoped_identifier
            for attr_child in child.children(&mut child.walk()) {
                match attr_child.kind() {
                    "identifier" => {
                        return attr_child.utf8_text(source).ok().map(|s| s.to_string());
                    }
                    "scoped_identifier" => {
                        // For paths like tokio::test, return the full path
                        return attr_child.utf8_text(source).ok().map(|s| s.to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

/// Walk AST to collect identifier usages and function calls.
/// Uses iterative traversal with a single TreeCursor for performance.
fn collect_usages_and_calls(result: &parser::ParseResult, fdc: &mut FileDeadCode) {
    let source = &result.source;
    let lang = result.language;
    let mut cursor = result.tree.walk();
    let mut current_function: Option<String> = None;
    let mut function_depth = 0u32;

    // Iterative pre-order traversal
    loop {
        let node = cursor.node();
        let kind = node.kind();

        // Track function context using depth
        if is_function_node(kind) {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    current_function = Some(name.to_string());
                    function_depth = cursor.depth();
                }
            }
        }

        // Collect usages from identifiers (excluding definitions)
        if (kind == "identifier" || kind == "type_identifier") && !is_definition_context(&node) {
            if let Ok(name) = node.utf8_text(source) {
                fdc.usages.insert(name.to_string());
            }
        }

        // Collect function calls
        if kind == "call_expression" || kind == "function_call" || kind == "call" {
            if let Some(callee) = extract_callee(&node, source, lang) {
                if let Some(ref caller) = current_function {
                    fdc.calls.push(CallReference {
                        caller: caller.clone(),
                        callee,
                        file: fdc.path.clone(),
                        line: node.start_position().row as u32 + 1,
                    });
                }
            }
        }

        // Move to next node in pre-order traversal
        if cursor.goto_first_child() {
            continue;
        }

        // No children, try siblings or go up
        loop {
            // Clear function context when leaving its scope
            if current_function.is_some()
                && cursor.depth() <= function_depth
                && is_function_node(cursor.node().kind())
            {
                current_function = None;
            }

            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return; // Done traversing
            }
        }
    }
}

fn is_function_node(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_definition"
            | "method_declaration"
            | "method_definition"
            | "function_item"
            | "arrow_function"
            | "method"
    )
}

fn is_definition_context(node: &tree_sitter::Node<'_>) -> bool {
    if let Some(parent) = node.parent() {
        let parent_kind = parent.kind();
        // Check if this identifier is the "name" field of a definition
        if let Some(name_node) = parent.child_by_field_name("name") {
            if name_node.id() == node.id() {
                return matches!(
                    parent_kind,
                    "function_declaration"
                        | "function_definition"
                        | "method_declaration"
                        | "variable_declarator"
                        | "let_declaration"
                        | "const_item"
                        | "static_item"
                        | "class_declaration"
                        | "struct_item"
                );
            }
        }
    }
    false
}

fn extract_callee(node: &tree_sitter::Node<'_>, source: &[u8], _lang: Language) -> Option<String> {
    // Try "function" field first
    if let Some(fn_node) = node.child_by_field_name("function") {
        // Handle member expressions (obj.method())
        if fn_node.kind() == "member_expression" || fn_node.kind() == "field_expression" {
            if let Some(prop) = fn_node.child_by_field_name("property") {
                return prop.utf8_text(source).ok().map(|s| s.to_string());
            }
        }
        return fn_node.utf8_text(source).ok().map(|s| s.to_string());
    }

    // Try first child
    if node.child_count() > 0 {
        let first = node.child(0)?;
        if first.kind() == "identifier" {
            return first.utf8_text(source).ok().map(|s| s.to_string());
        }
    }

    None
}

fn get_visibility(name: &str, lang: Language) -> String {
    match lang {
        Language::Go => {
            if !name.is_empty() && name.chars().next().unwrap().is_uppercase() {
                "public".to_string()
            } else {
                "private".to_string()
            }
        }
        Language::Python => {
            if name.starts_with("__") {
                "private".to_string()
            } else if name.starts_with('_') {
                "internal".to_string()
            } else {
                "public".to_string()
            }
        }
        Language::Ruby => {
            if name.starts_with('_') {
                "private".to_string()
            } else {
                "public".to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

fn is_exported(name: &str, lang: Language) -> bool {
    match lang {
        Language::Go => !name.is_empty() && name.chars().next().unwrap().is_uppercase(),
        Language::Python => !name.starts_with('_'),
        Language::Rust => false, // Would need AST context for `pub`
        _ => false,
    }
}

fn is_test_file(path: &str) -> bool {
    path.ends_with("_test.go")
        || path.ends_with("_test.py")
        || path.ends_with(".test.ts")
        || path.ends_with(".test.js")
        || path.ends_with(".spec.ts")
        || path.ends_with(".spec.js")
        || path.ends_with("_spec.rb")
        || path.ends_with("_test.rb")
        || path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
}

fn is_entry_point(name: &str, def: &Definition) -> bool {
    // Standard entry points
    if name == "main" || name == "init" || name == "Main" {
        return true;
    }

    // Test functions by name pattern
    if name.starts_with("Test") || name.starts_with("test") {
        return true;
    }

    // Rust test attributes (#[test], #[tokio::test], #[bench], etc.)
    for attr in &def.attributes {
        if attr == "test" || attr.ends_with("::test") || attr == "bench" || attr == "tokio::main" {
            return true;
        }
    }

    // Benchmark/Example/Fuzz (Go)
    if name.starts_with("Benchmark") || name.starts_with("Example") || name.starts_with("Fuzz") {
        return true;
    }

    // HTTP handlers
    if name.ends_with("Handler")
        || name.ends_with("handler")
        || name == "ServeHTTP"
        || name == "Handle"
    {
        return true;
    }

    // Event handlers
    if name.starts_with("On") || name.starts_with("on") || name.starts_with("Handle") {
        return true;
    }

    // Exported symbols in Go/Rust are often entry points
    if def.exported {
        return true;
    }

    // Trait implementation methods are entry points because they may be called
    // via dynamic dispatch (trait objects) which static analysis cannot track
    if def.is_trait_impl {
        return true;
    }

    false
}

fn calculate_confidence(def: &Definition, is_unreachable: bool, is_unused: bool) -> f64 {
    let mut confidence: f64 = 0.9;

    // Higher confidence for unreachable + unused
    if is_unreachable && is_unused {
        confidence += 0.05;
    }

    // Reduce for exported symbols
    if def.exported {
        confidence -= 0.25;
    }

    // Higher for private symbols
    if def.visibility == "private" {
        confidence += 0.05;
    }

    // Reduce for test files
    if def.is_test_file {
        confidence -= 0.15;
    }

    confidence.clamp(0.0, 1.0)
}

// Internal types for file analysis
struct FileDeadCode {
    path: String,
    definitions: HashMap<String, Definition>,
    usages: HashSet<String>,
    calls: Vec<CallReference>,
}

#[derive(Clone)]
struct Definition {
    #[allow(dead_code)]
    name: String,
    kind: String,
    file: String,
    line: u32,
    end_line: u32,
    visibility: String,
    exported: bool,
    is_test_file: bool,
    /// Attributes on this definition (e.g., "test", "tokio::test", "bench")
    attributes: Vec<String>,
    /// Whether this is a trait implementation method
    is_trait_impl: bool,
}

#[derive(Clone)]
struct CallReference {
    caller: String,
    callee: String,
    file: String,
    #[allow(dead_code)]
    line: u32,
}

// Public output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub items: Vec<DeadCodeItem>,
    pub summary: AnalysisSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeItem {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub visibility: String,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_items: usize,
    pub by_kind: HashMap<String, usize>,
    pub total_definitions: usize,
    pub reachable_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "deadcode");
    }

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file("foo_test.go"));
        assert!(is_test_file("bar.test.ts"));
        assert!(is_test_file("src/tests/helper.py"));
        assert!(!is_test_file("main.go"));
        assert!(!is_test_file("app.ts"));
    }

    #[test]
    fn test_is_entry_point() {
        let def = Definition {
            name: "main".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("main", &def));

        let def2 = Definition {
            name: "TestSomething".to_string(),
            kind: "function".to_string(),
            file: "foo_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            is_test_file: true,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("TestSomething", &def2));
    }

    #[test]
    fn test_get_visibility() {
        assert_eq!(get_visibility("PublicFunc", Language::Go), "public");
        assert_eq!(get_visibility("privateFunc", Language::Go), "private");
        assert_eq!(get_visibility("__private", Language::Python), "private");
        assert_eq!(get_visibility("_internal", Language::Python), "internal");
        assert_eq!(get_visibility("public", Language::Python), "public");
    }

    #[test]
    fn test_calculate_confidence() {
        let private_def = Definition {
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        let conf = calculate_confidence(&private_def, true, true);
        assert!(conf > 0.9); // High confidence for private, unreachable, unused

        let exported_def = Definition {
            name: "Public".to_string(),
            kind: "function".to_string(),
            file: "lib.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        let conf2 = calculate_confidence(&exported_def, false, true);
        assert!(conf2 < 0.7); // Lower confidence for exported
    }

    #[test]
    fn test_same_name_different_files() {
        // Test that qualified names prevent collisions when multiple files
        // have functions with the same name.
        let mut all_definitions: HashMap<String, Definition> = HashMap::new();
        let mut name_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

        // Two files both have a helper() function
        let def1 = Definition {
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "src/util.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };

        let def2 = Definition {
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "src/parser.go".to_string(),
            line: 5,
            end_line: 15,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };

        // Using qualified names, both should be tracked
        let qualified1 = format!("{}::{}", def1.file, "helper");
        let qualified2 = format!("{}::{}", def2.file, "helper");

        all_definitions.insert(qualified1.clone(), def1.clone());
        all_definitions.insert(qualified2.clone(), def2.clone());

        name_to_qualified
            .entry("helper".to_string())
            .or_default()
            .push(qualified1.clone());
        name_to_qualified
            .entry("helper".to_string())
            .or_default()
            .push(qualified2.clone());

        // Both definitions should exist (no collision)
        assert_eq!(all_definitions.len(), 2);
        assert!(all_definitions.contains_key("src/util.go::helper"));
        assert!(all_definitions.contains_key("src/parser.go::helper"));

        // The name_to_qualified map should track both
        assert_eq!(name_to_qualified.get("helper").unwrap().len(), 2);

        // Verify they are different definitions by checking file paths
        let stored_def1 = all_definitions.get("src/util.go::helper").unwrap();
        let stored_def2 = all_definitions.get("src/parser.go::helper").unwrap();
        assert_eq!(stored_def1.file, "src/util.go");
        assert_eq!(stored_def2.file, "src/parser.go");
        assert_eq!(stored_def1.line, 1);
        assert_eq!(stored_def2.line, 5);
    }

    #[test]
    fn test_qualified_name_extraction() {
        // Test extracting simple name from qualified name
        let qualified = "src/util.go::helper";
        let simple = qualified.rsplit("::").next().unwrap_or(qualified);
        assert_eq!(simple, "helper");

        // Edge case: no :: in name
        let simple_only = "helper";
        let extracted = simple_only.rsplit("::").next().unwrap_or(simple_only);
        assert_eq!(extracted, "helper");
    }

    #[test]
    fn test_rust_test_attribute_is_entry_point() {
        // Functions with #[test] attribute should be treated as entry points
        // even if they don't follow a naming convention like "test_*"
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
            #[test]
            fn my_weird_test_name() {
                assert!(true);
            }

            fn helper_not_a_test() {
                println!("I'm not a test");
            }
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // The #[test] function should be recognized as an entry point
        let test_fn = fdc.definitions.get("my_weird_test_name").unwrap();
        assert!(
            is_entry_point("my_weird_test_name", test_fn),
            "Function with #[test] attribute should be an entry point"
        );

        // The helper function should NOT be an entry point
        let helper_fn = fdc.definitions.get("helper_not_a_test").unwrap();
        assert!(
            !is_entry_point("helper_not_a_test", helper_fn),
            "Regular function without #[test] should not be an entry point"
        );
    }

    #[test]
    fn test_rust_tokio_test_attribute_is_entry_point() {
        // Functions with #[tokio::test] attribute should be treated as entry points
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
            #[tokio::test]
            async fn my_async_test() {
                assert!(true);
            }
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // The #[tokio::test] function should be recognized as an entry point
        let test_fn = fdc.definitions.get("my_async_test").unwrap();
        assert!(
            is_entry_point("my_async_test", test_fn),
            "Function with #[tokio::test] attribute should be an entry point, attrs: {:?}",
            test_fn.attributes
        );
    }

    #[test]
    fn test_rust_cfg_test_module_functions_are_entry_points() {
        // Functions inside #[cfg(test)] modules should be treated as entry points
        // since they're test code, even if they don't have #[test] attribute
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
            fn production_code() {
                println!("I'm production code");
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                fn helper_in_test_module() {
                    // This is a test helper, not flagged as dead code
                }

                #[test]
                fn actual_test() {
                    helper_in_test_module();
                }
            }
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // Functions inside #[cfg(test)] should be recognized as being in test context
        // They should have is_test_file = true OR have reduced confidence
        let helper_fn = fdc.definitions.get("helper_in_test_module");
        assert!(
            helper_fn.is_some(),
            "helper_in_test_module should be found in definitions"
        );

        let helper = helper_fn.unwrap();
        // The helper function should be marked as in a test context
        assert!(
            helper.is_test_file || !helper.attributes.is_empty(),
            "Function in #[cfg(test)] module should be recognized as test code"
        );
    }

    #[test]
    fn test_rust_trait_impl_methods_have_reduced_confidence() {
        // Trait implementation methods should have reduced confidence for dead code
        // because they may be called via dynamic dispatch (trait objects)
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
            trait MyTrait {
                fn trait_method(&self);
            }

            struct MyStruct;

            impl MyTrait for MyStruct {
                fn trait_method(&self) {
                    println!("trait method impl");
                }
            }

            fn standalone_function() {
                println!("not a trait impl");
            }
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // The trait impl method should be found and marked as a trait impl
        let trait_method = fdc.definitions.get("trait_method");
        assert!(
            trait_method.is_some(),
            "trait_method should be found in definitions"
        );

        let trait_fn = trait_method.unwrap();
        // Trait impl methods should be either marked as entry points or have reduced confidence
        // For now, we'll check that they're detected as trait impls (via attributes or kind)
        assert!(
            trait_fn.kind == "trait_impl" || is_entry_point("trait_method", trait_fn),
            "Trait impl method should be recognized as a trait implementation"
        );
    }

    #[test]
    fn test_rust_pub_functions_are_entry_points() {
        // Public functions in Rust should be treated as entry points
        // because they're part of the public API
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
            pub fn public_api_function() {
                println!("I'm a public API function");
            }

            fn private_helper() {
                println!("I'm a private helper");
            }
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // The pub function should be marked as exported and be an entry point
        let pub_fn = fdc.definitions.get("public_api_function").unwrap();
        assert!(pub_fn.exported, "pub function should be marked as exported");
        assert!(
            is_entry_point("public_api_function", pub_fn),
            "pub function should be an entry point"
        );

        // The private function should NOT be an entry point
        let private_fn = fdc.definitions.get("private_helper").unwrap();
        assert!(
            !private_fn.exported,
            "private function should not be marked as exported"
        );
        assert!(
            !is_entry_point("private_helper", private_fn),
            "private function should not be an entry point"
        );
    }

    #[test]
    fn test_cargo_analyzer_parse_dead_code_json() {
        let json_line = r#"{"reason":"compiler-message","package_id":"test 0.1.0","manifest_path":"/test/Cargo.toml","target":{"name":"test"},"message":{"rendered":"warning: function `unused_func` is never used\n","code":{"code":"dead_code"},"level":"warning","message":"function `unused_func` is never used","spans":[{"file_name":"src/lib.rs","byte_start":0,"byte_end":10,"line_start":5,"line_end":7,"column_start":1,"column_end":2}]}}"#;

        let item = CargoDeadCodeAnalyzer::parse_json_message(json_line);
        assert!(item.is_some(), "Should parse dead_code JSON message");

        let item = item.unwrap();
        assert_eq!(item.name, "unused_func");
        assert_eq!(item.file, "src/lib.rs");
        assert_eq!(item.line, 5);
        assert_eq!(item.end_line, 7);
    }

    #[test]
    fn test_cargo_analyzer_ignores_non_dead_code() {
        let json_line = r#"{"reason":"compiler-message","message":{"code":{"code":"unused_imports"},"level":"warning","message":"unused import"}}"#;

        let item = CargoDeadCodeAnalyzer::parse_json_message(json_line);
        assert!(item.is_none(), "Should ignore non-dead_code warnings");
    }
}

/// Cargo-based dead code analyzer for Rust projects.
/// Uses `cargo check --message-format=json` to get accurate dead code warnings
/// from the Rust compiler.
pub struct CargoDeadCodeAnalyzer;

/// A dead code item detected by cargo/rustc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDeadCodeItem {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub message: String,
    pub kind: String,
}

/// Result of cargo-based dead code analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDeadCodeAnalysis {
    pub items: Vec<CargoDeadCodeItem>,
    pub summary: CargoDeadCodeSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDeadCodeSummary {
    pub total_items: usize,
    pub by_kind: HashMap<String, usize>,
}

impl CargoDeadCodeAnalyzer {
    /// Analyze a Rust project using cargo check.
    pub fn analyze(project_path: &std::path::Path) -> Result<CargoDeadCodeAnalysis> {
        use std::process::Command;

        // Run cargo check with JSON output
        let output = Command::new("cargo")
            .arg("check")
            .arg("--message-format=json")
            .arg("--all-targets")
            .current_dir(project_path)
            .output()
            .map_err(|e| crate::core::Error::Analysis {
                message: format!("Failed to run cargo check: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let items = Self::parse_cargo_output(&stdout);

        let mut by_kind: HashMap<String, usize> = HashMap::new();
        for item in &items {
            *by_kind.entry(item.kind.clone()).or_insert(0) += 1;
        }

        Ok(CargoDeadCodeAnalysis {
            summary: CargoDeadCodeSummary {
                total_items: items.len(),
                by_kind,
            },
            items,
        })
    }

    /// Parse cargo JSON output for dead code warnings.
    fn parse_cargo_output(output: &str) -> Vec<CargoDeadCodeItem> {
        output
            .lines()
            .filter_map(Self::parse_json_message)
            .collect()
    }

    /// Parse a single JSON message line from cargo output.
    fn parse_json_message(line: &str) -> Option<CargoDeadCodeItem> {
        // Quick check before parsing JSON
        if !line.contains(r#""code":"dead_code""#) {
            return None;
        }

        let json: serde_json::Value = serde_json::from_str(line).ok()?;

        // Verify this is a dead_code warning
        let code = json.get("message")?.get("code")?.get("code")?.as_str()?;
        if code != "dead_code" {
            return None;
        }

        let message_obj = json.get("message")?;
        let message = message_obj.get("message")?.as_str()?.to_string();

        // Extract function/struct/etc name from message
        let name = Self::extract_name_from_message(&message)?;

        // Get location from spans
        let spans = message_obj.get("spans")?.as_array()?;
        let span = spans.first()?;

        let file = span.get("file_name")?.as_str()?.to_string();
        let line = span.get("line_start")?.as_u64()? as u32;
        let end_line = span.get("line_end")?.as_u64()? as u32;

        // Determine kind from message
        let kind = Self::determine_kind(&message);

        Some(CargoDeadCodeItem {
            name,
            file,
            line,
            end_line,
            message,
            kind,
        })
    }

    /// Extract the name of the dead code item from the warning message.
    fn extract_name_from_message(message: &str) -> Option<String> {
        // Patterns like:
        // "function `unused_func` is never used"
        // "struct `UnusedStruct` is never constructed"
        // "constant `UNUSED_CONST` is never used"
        if let Some(start) = message.find('`') {
            let rest = &message[start + 1..];
            if let Some(end) = rest.find('`') {
                return Some(rest[..end].to_string());
            }
        }
        None
    }

    /// Determine the kind of dead code from the message.
    fn determine_kind(message: &str) -> String {
        if message.starts_with("function") || message.starts_with("method") {
            "function".to_string()
        } else if message.starts_with("struct") {
            "struct".to_string()
        } else if message.starts_with("enum") {
            "enum".to_string()
        } else if message.starts_with("constant") || message.starts_with("static") {
            "constant".to_string()
        } else if message.starts_with("type alias") {
            "type_alias".to_string()
        } else if message.starts_with("field") {
            "field".to_string()
        } else if message.starts_with("variant") {
            "variant".to_string()
        } else {
            "unknown".to_string()
        }
    }
}
