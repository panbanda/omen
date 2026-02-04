//! Dead code detection analyzer.
//!
//! Finds unreachable/unused functions, variables, and classes using
//! reference graph analysis.
//!
//! ## Limitations
//!
//! Currently only function definitions and usages are tracked.
//! Variable declarations, class/struct definitions, module-level
//! constants, and type aliases are not yet analyzed.

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
            tracing::info!("Detected Cargo.toml, using cargo check for Rust dead code analysis");
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

        // Phase 1: Collect definitions and usages from all files.
        // Tree-sitter extraction runs on all files (including .rs) to build
        // the definition/reference graph. Cargo results supplement this with
        // compiler-level dead code warnings when available.
        let files: Vec<_> = ctx.files.iter().collect();
        let file_results: Vec<FileDeadCode> = files
            .par_iter()
            .filter_map(|path| {
                let full_path = ctx.root.join(path);
                self.analyze_file(&full_path).ok()
            })
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

        // Add cargo-detected dead code items (Rust files).
        // Track (file, line) pairs so tree-sitter results don't duplicate them.
        let mut cargo_reported: HashSet<(String, u32)> = HashSet::new();
        for cargo_item in cargo_items {
            cargo_reported.insert((cargo_item.file.clone(), cargo_item.line));
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

        // Add tree-sitter detected dead code, skipping items already reported by cargo
        for (qualified_name, def) in &all_definitions {
            // Extract simple name for entry point check and usage lookup
            let simple_name = qualified_name.rsplit("::").next().unwrap_or(qualified_name);

            // Skip entry points
            if is_entry_point(simple_name, def) {
                continue;
            }

            // Skip items already reported by cargo (higher confidence)
            if cargo_reported.contains(&(def.file.clone(), def.line)) {
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
    let path_str = result.path.to_string_lossy().to_string();
    let mut fdc = FileDeadCode {
        path: path_str.clone(),
        definitions: HashMap::new(),
        usages: HashSet::new(),
        calls: Vec::new(),
    };

    // Skip generated files entirely -- their definitions are not meaningful
    // for dead code analysis and cause false positives.
    if is_generated_file(&path_str) {
        // Still collect usages so references from generated code to real code
        // are tracked (prevents marking real code as unused).
        collect_usages_and_calls(result, &mut fdc);
        return fdc;
    }

    let functions = parser::extract_functions(result);

    let is_test_file = is_test_file(&fdc.path);

    // For Rust, extract function attributes and context from the AST
    let function_info = if result.language == Language::Rust {
        extract_rust_function_attributes(result)
    } else {
        HashMap::new()
    };

    // For Python, extract __all__, if __name__ calls, and decorators
    let python_ctx = if result.language == Language::Python {
        Some(extract_python_context(result))
    } else {
        None
    };

    // For Ruby, extract visibility modifiers
    let ruby_vis = if result.language == Language::Ruby {
        Some(extract_ruby_visibility(result))
    } else {
        None
    };

    // Extract function definitions
    for func in functions {
        let mut visibility = get_visibility(&func.name, result.language);
        // Use the parser's is_exported which correctly checks for pub in Rust
        let mut exported = func.is_exported || is_exported(&func.name, result.language);
        let info = function_info.get(&func.name);
        let mut attributes = info.map(|i| i.attributes.clone()).unwrap_or_default();
        // Mark as test file if already in test file OR inside #[cfg(test)] module
        let is_in_test_context =
            is_test_file || info.map(|i| i.in_cfg_test_module).unwrap_or(false);
        let is_trait_impl = info.map(|i| i.is_trait_impl).unwrap_or(false);

        // Apply Python-specific context
        if let Some(ref py_ctx) = python_ctx {
            // Functions listed in __all__ are explicitly exported
            if py_ctx.all_exports.contains(&func.name) {
                exported = true;
            }
            // Functions called in if __name__ == "__main__" are entry points
            if py_ctx.name_main_calls.contains(&func.name) {
                attributes.push("__name__main_call".to_string());
            }
            // Attach decorator info
            if let Some(decorators) = py_ctx.decorators.get(&func.name) {
                attributes.extend(decorators.iter().cloned());
            }
        }

        // Apply Ruby visibility modifiers
        if let Some(ref rb_vis) = ruby_vis {
            if let Some(vis) = rb_vis.method_visibility.get(&func.name) {
                visibility = vis.clone();
                if vis == "private" || vis == "protected" {
                    exported = false;
                }
            }
        }

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

/// Python-specific context extracted from the AST.
struct PythonContext {
    /// Names listed in `__all__` (these are explicitly exported).
    all_exports: HashSet<String>,
    /// Functions called inside `if __name__ == "__main__"` blocks.
    name_main_calls: HashSet<String>,
    /// Map from function name to its decorator strings.
    decorators: HashMap<String, Vec<String>>,
}

/// Ruby visibility context: maps method name to visibility modifier.
struct RubyVisibility {
    method_visibility: HashMap<String, String>,
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
            if let Some(func_info) = collect_rust_function_info(
                &node,
                source,
                current_in_cfg_test,
                current_in_trait_impl,
            ) {
                info.insert(func_info.0, func_info.1);
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

/// Collect info for a single Rust function_item node: name, attributes, and context flags.
fn collect_rust_function_info(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    in_cfg_test_module: bool,
    is_trait_impl: bool,
) -> Option<(String, RustFunctionInfo)> {
    let name_node = node.child_by_field_name("name")?;
    let func_name = name_node.utf8_text(source).ok()?;

    let preceding_attrs = collect_preceding_attributes(node, source);
    Some((
        func_name.to_string(),
        RustFunctionInfo {
            attributes: preceding_attrs,
            in_cfg_test_module,
            is_trait_impl,
        },
    ))
}

/// Collect attribute names from consecutive attribute_item siblings preceding a node.
fn collect_preceding_attributes(node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
    let mut attrs = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() != "attribute_item" {
            break;
        }
        if let Some(attr_name) = extract_attribute_name(&sibling, source) {
            attrs.push(attr_name);
        }
        prev = sibling.prev_sibling();
    }
    attrs
}

/// Extract Python-specific context: `__all__` exports, `if __name__` calls, and decorators.
fn extract_python_context(result: &parser::ParseResult) -> PythonContext {
    let root = result.root_node();
    let source = &result.source;
    let mut ctx = PythonContext {
        all_exports: HashSet::new(),
        name_main_calls: HashSet::new(),
        decorators: HashMap::new(),
    };

    fn visit(node: tree_sitter::Node<'_>, source: &[u8], ctx: &mut PythonContext) {
        match node.kind() {
            // Parse `__all__ = ["func1", "func2"]`
            "expression_statement" => {
                try_extract_all_assignment(&node, source, ctx);
            }
            // Parse `if __name__ == "__main__":` blocks
            "if_statement" => {
                if is_name_main_guard(&node, source) {
                    collect_calls_in_subtree(&node, source, &mut ctx.name_main_calls);
                }
            }
            // Parse decorated function definitions
            "decorated_definition" => {
                extract_python_decorators(&node, source, ctx);
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            visit(child, source, ctx);
        }
    }

    visit(root, source, &mut ctx);
    ctx
}

/// Try to extract `__all__ = [...]` from an expression_statement node.
fn try_extract_all_assignment(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    ctx: &mut PythonContext,
) {
    let child = match node.child(0) {
        Some(c) if c.kind() == "assignment" => c,
        _ => return,
    };
    let left = match child.child_by_field_name("left") {
        Some(l) => l,
        None => return,
    };
    if left.utf8_text(source).ok() != Some("__all__") {
        return;
    }
    if let Some(right) = child.child_by_field_name("right") {
        extract_all_list_entries(&right, source, ctx);
    }
}

/// Extract string entries from a list literal (for `__all__`).
fn extract_all_list_entries(node: &tree_sitter::Node<'_>, source: &[u8], ctx: &mut PythonContext) {
    // The right-hand side should be a list node containing string children.
    if node.kind() != "list" {
        return;
    }
    for child in node.children(&mut node.walk()) {
        if child.kind() == "string" {
            if let Ok(text) = child.utf8_text(source) {
                // Strip quotes: "func1" or 'func1'
                let stripped = text
                    .trim_start_matches(['"', '\''])
                    .trim_end_matches(['"', '\'']);
                if !stripped.is_empty() {
                    ctx.all_exports.insert(stripped.to_string());
                }
            }
        }
    }
}

/// Check if an `if_statement` node is `if __name__ == "__main__":`.
fn is_name_main_guard(node: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    // The condition is the first child after "if" keyword.
    if let Some(condition) = node.child_by_field_name("condition") {
        if condition.kind() == "comparison_operator" {
            if let Ok(text) = condition.utf8_text(source) {
                let normalized = text.replace(' ', "");
                return normalized.contains("__name__==\"__main__\"")
                    || normalized.contains("__name__=='__main__'")
                    || normalized.contains("\"__main__\"==__name__")
                    || normalized.contains("'__main__'==__name__");
            }
        }
    }
    false
}

/// Collect all function/method call names within a subtree.
fn collect_calls_in_subtree(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    calls: &mut HashSet<String>,
) {
    if node.kind() == "call" {
        // Get the function name from the call
        if let Some(fn_node) = node.child_by_field_name("function") {
            if fn_node.kind() == "identifier" {
                if let Ok(name) = fn_node.utf8_text(source) {
                    calls.insert(name.to_string());
                }
            } else if fn_node.kind() == "attribute" {
                // obj.method() -- grab the attribute (method) name
                if let Some(attr) = fn_node.child_by_field_name("attribute") {
                    if let Ok(name) = attr.utf8_text(source) {
                        calls.insert(name.to_string());
                    }
                }
            }
        }
    }
    for child in node.children(&mut node.walk()) {
        collect_calls_in_subtree(&child, source, calls);
    }
}

/// Extract decorators from a `decorated_definition` node and associate them with the function.
fn extract_python_decorators(node: &tree_sitter::Node<'_>, source: &[u8], ctx: &mut PythonContext) {
    let mut decorator_texts: Vec<String> = Vec::new();
    let mut func_name: Option<String> = None;

    for child in node.children(&mut node.walk()) {
        if child.kind() == "decorator" {
            if let Ok(text) = child.utf8_text(source) {
                // Strip the leading '@' and any trailing whitespace
                let stripped = text.trim_start_matches('@').trim();
                decorator_texts.push(stripped.to_string());
            }
        } else if child.kind() == "function_definition" {
            if let Some(name_node) = child.child_by_field_name("name") {
                func_name = name_node.utf8_text(source).ok().map(|s| s.to_string());
            }
        }
    }

    if let Some(name) = func_name {
        if !decorator_texts.is_empty() {
            ctx.decorators.insert(name, decorator_texts);
        }
    }
}

/// Decorator patterns that indicate a function is an entry point.
const ENTRY_POINT_DECORATOR_KEYWORDS: &[&str] = &[
    "route",
    "task",
    "fixture",
    "handler",
    "endpoint",
    "command",
    "hook",
    "signal",
    "receiver",
    "property",
    "pytest.fixture",
    "pytest.mark",
    "shared_task",
    "celery.task",
];

/// Check if a decorator string indicates an entry point.
fn is_entry_point_decorator(decorator: &str) -> bool {
    // Exact known patterns
    let lower = decorator.to_lowercase();
    for keyword in ENTRY_POINT_DECORATOR_KEYWORDS {
        if lower.contains(keyword) {
            return true;
        }
    }
    false
}

/// Extract Ruby method visibility based on `private`, `protected`, `public` calls.
///
/// In Ruby, `private` / `protected` / `public` can be used as:
/// 1. Section markers: all methods defined after `private` are private.
/// 2. Method-level: `private :method_name`
fn extract_ruby_visibility(result: &parser::ParseResult) -> RubyVisibility {
    let root = result.root_node();
    let source = &result.source;
    let mut vis = RubyVisibility {
        method_visibility: HashMap::new(),
    };

    fn visit_class_body(node: tree_sitter::Node<'_>, source: &[u8], vis: &mut RubyVisibility) {
        // Only process class/module bodies
        if node.kind() != "class" && node.kind() != "module" && node.kind() != "singleton_class" {
            // Recurse to find classes/modules
            for child in node.children(&mut node.walk()) {
                visit_class_body(child, source, vis);
            }
            return;
        }

        // Find the body node inside the class/module
        if let Some(body) = node.child_by_field_name("body") {
            process_body_for_visibility(&body, source, vis);
        } else {
            // Some grammars put children directly under class node
            process_body_for_visibility(&node, source, vis);
        }

        // Recurse into nested classes
        for child in node.children(&mut node.walk()) {
            if child.kind() == "class" || child.kind() == "module" {
                visit_class_body(child, source, vis);
            }
        }
    }

    fn process_body_for_visibility(
        body: &tree_sitter::Node<'_>,
        source: &[u8],
        vis: &mut RubyVisibility,
    ) {
        let mut current_visibility = "public".to_string();

        for child in body.children(&mut body.walk()) {
            match child.kind() {
                "method" | "singleton_method" => {
                    record_method_visibility(&child, source, &current_visibility, vis);
                }
                "identifier" => {
                    if let Some(new_vis) = parse_visibility_keyword(&child, source) {
                        current_visibility = new_vis;
                    }
                }
                "call" => {
                    if let Some(new_vis) = process_ruby_visibility_call(&child, source, vis) {
                        current_visibility = new_vis;
                    }
                }
                _ => {}
            }
        }
    }

    visit_class_body(root, source, &mut vis);
    vis
}

/// Record a Ruby method's visibility based on the current section marker.
fn record_method_visibility(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    current_visibility: &str,
    vis: &mut RubyVisibility,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = match name_node.utf8_text(source) {
        Ok(n) => n,
        Err(_) => return,
    };
    vis.method_visibility
        .insert(name.to_string(), current_visibility.to_string());
}

/// Parse a bare `private` / `protected` / `public` identifier as a visibility keyword.
/// Returns the new visibility if the node is a visibility keyword, None otherwise.
fn parse_visibility_keyword(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?;
    match text {
        "private" | "protected" | "public" => Some(text.to_string()),
        _ => None,
    }
}

/// Process a Ruby `call` node that may be a visibility modifier (e.g. `private :foo`).
/// Returns Some(visibility) if this acts as a section marker, None otherwise.
fn process_ruby_visibility_call(
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    vis: &mut RubyVisibility,
) -> Option<String> {
    let method_node = node.child_by_field_name("method")?;
    let method_text = method_node.utf8_text(source).ok()?;

    if !matches!(method_text, "private" | "protected" | "public") {
        return None;
    }

    let args = match node.child_by_field_name("arguments") {
        Some(args) => args,
        None => return Some(method_text.to_string()),
    };

    let mut found_symbols = false;
    for arg in args.children(&mut args.walk()) {
        if arg.kind() != "simple_symbol" {
            continue;
        }
        let sym = match arg.utf8_text(source) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let name = sym.trim_start_matches(':').to_string();
        vis.method_visibility.insert(name, method_text.to_string());
        found_symbols = true;
    }

    if found_symbols {
        None
    } else {
        Some(method_text.to_string())
    }
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

            // Detect function-as-value references in call arguments.
            // If an argument is a bare identifier matching a known function
            // definition, add a synthetic call edge from the current function
            // to that identifier so BFS can reach it.
            if let Some(ref caller) = current_function {
                collect_function_value_refs(&node, source, caller, fdc);
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

/// Scan call arguments for bare identifiers that match known function definitions.
/// These represent function-as-value references (callbacks) and get synthetic call edges
/// so that BFS reachability can follow them.
fn collect_function_value_refs(
    call_node: &tree_sitter::Node<'_>,
    source: &[u8],
    caller: &str,
    fdc: &mut FileDeadCode,
) {
    // Find the argument_list / arguments child of the call node
    let args_node = call_node.child_by_field_name("arguments").or_else(|| {
        // Some grammars (Go) use "argument_list" as the node kind
        // rather than a named field
        (0..call_node.child_count())
            .filter_map(|i| call_node.child(i))
            .find(|c| c.kind() == "argument_list")
    });

    let args_node = match args_node {
        Some(n) => n,
        None => return,
    };

    for i in 0..args_node.child_count() {
        let arg = match args_node.child(i) {
            Some(a) => a,
            None => continue,
        };

        if arg.kind() != "identifier" {
            continue;
        }

        let name = match arg.utf8_text(source) {
            Ok(n) => n,
            Err(_) => continue,
        };

        if fdc.definitions.contains_key(name) {
            fdc.calls.push(CallReference {
                caller: caller.to_string(),
                callee: name.to_string(),
                file: fdc.path.clone(),
                line: call_node.start_position().row as u32 + 1,
            });
        }
    }
}

fn get_visibility(name: &str, lang: Language) -> String {
    match lang {
        Language::Go => {
            if !name.is_empty() && name.starts_with(char::is_uppercase) {
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
        Language::Go => !name.is_empty() && name.starts_with(char::is_uppercase),
        Language::Python => !name.starts_with('_'),
        Language::Rust => false, // Would need AST context for `pub`
        _ => false,
    }
}

fn is_generated_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower = filename.to_lowercase();

    // Protobuf generated files
    if lower.ends_with(".pb.go")
        || lower.ends_with(".pb.cc")
        || lower.ends_with(".pb.h")
        || lower.ends_with(".pb2.py")
        || lower.ends_with("_pb2.py")
        || lower.ends_with("_pb2_grpc.py")
    {
        return true;
    }

    // Common generated file patterns
    if lower.ends_with(".gen.go")
        || lower.ends_with(".generated.go")
        || lower.ends_with(".generated.ts")
        || lower.ends_with(".generated.js")
    {
        return true;
    }

    // Go-specific generated file conventions
    if lower.ends_with("_gen.go")
        || lower.ends_with("_generated.go")
        || lower == "bindata.go"
        || lower.ends_with("wire_gen.go")
    {
        return true;
    }

    // Mock generated files
    if lower.starts_with("mock_") && lower.ends_with(".go") {
        return true;
    }

    // Kubernetes code-gen (zz_generated.*.go)
    if lower.starts_with("zz_generated.") {
        return true;
    }

    false
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
    // Python decorators and __name__ == "__main__" calls
    for attr in &def.attributes {
        if attr == "test" || attr.ends_with("::test") || attr == "bench" || attr == "tokio::main" {
            return true;
        }
        // Functions called in `if __name__ == "__main__"` blocks
        if attr == "__name__main_call" {
            return true;
        }
        // Python decorator-based entry points
        if is_entry_point_decorator(attr) {
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

    #[test]
    fn test_rust_extracts_all_function_definitions() {
        // Verifies that tree-sitter extraction finds free functions,
        // inherent impl methods, and trait impl methods in Rust.
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
fn free_function() {}

fn another_free() -> bool { true }

struct MyStruct;

impl MyStruct {
    fn new() -> Self { MyStruct }

    fn method_one(&self) {}

    pub fn method_two(&mut self) {}
}

trait MyTrait {
    fn required(&self);
}

impl MyTrait for MyStruct {
    fn required(&self) {}
}
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        let names: HashSet<&str> = fdc.definitions.keys().map(|s| s.as_str()).collect();
        assert!(
            names.contains("free_function"),
            "should find free_function, got: {:?}",
            names
        );
        assert!(
            names.contains("another_free"),
            "should find another_free, got: {:?}",
            names
        );
        assert!(
            names.contains("new"),
            "should find impl method 'new', got: {:?}",
            names
        );
        assert!(
            names.contains("method_one"),
            "should find impl method 'method_one', got: {:?}",
            names
        );
        assert!(
            names.contains("method_two"),
            "should find impl method 'method_two', got: {:?}",
            names
        );
        assert!(
            names.contains("required"),
            "should find trait impl method 'required', got: {:?}",
            names
        );

        assert_eq!(
            fdc.definitions.len(),
            6,
            "should find exactly 6 function definitions, got: {:?}",
            names
        );
    }

    #[test]
    fn test_rust_non_cargo_project_uses_treesitter() {
        // When there is no Cargo.toml, the deadcode analyzer should use
        // tree-sitter to extract Rust function definitions instead of
        // skipping .rs files.
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
fn alpha() {}
fn beta() {}
fn gamma() {}
fn delta() {}
fn epsilon() {}
        "#;
        let result = parser
            .parse(content, Language::Rust, Path::new("standalone.rs"))
            .unwrap();
        let functions = crate::parser::extract_functions(&result);

        assert_eq!(
            functions.len(),
            5,
            "tree-sitter should find all 5 Rust functions, got: {:?}",
            functions.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_python_all_list_marks_exported() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
__all__ = ["_internal_api", "public_func"]

def _internal_api():
    pass

def public_func():
    pass

def _unlisted_helper():
    pass
"#;
        let result = parser
            .parse(content, Language::Python, Path::new("mymodule.py"))
            .unwrap();

        // Verify __all__ parsing extracts the correct names
        let py_ctx = extract_python_context(&result);
        assert!(
            py_ctx.all_exports.contains("_internal_api"),
            "__all__ should contain _internal_api"
        );
        assert!(
            py_ctx.all_exports.contains("public_func"),
            "__all__ should contain public_func"
        );
        assert!(
            !py_ctx.all_exports.contains("_unlisted_helper"),
            "__all__ should not contain _unlisted_helper"
        );

        // Verify collect_file_data integrates __all__ into the exported flag.
        // _internal_api starts with _ so is_exported() alone returns false,
        // but __all__ listing should force it to exported.
        let fdc = collect_file_data(&result);
        let internal_fn = fdc.definitions.get("_internal_api").unwrap();
        assert!(
            internal_fn.exported,
            "Function in __all__ should be exported even with _ prefix"
        );
        assert!(
            is_entry_point("_internal_api", internal_fn),
            "__all__-listed function should be an entry point"
        );
    }

    #[test]
    fn test_python_name_main_block_functions_are_entry_points() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
def main():
    print("running")

def setup():
    print("setup")

def unused():
    pass

if __name__ == "__main__":
    setup()
    main()
"#;
        let result = parser
            .parse(content, Language::Python, Path::new("script.py"))
            .unwrap();
        let fdc = collect_file_data(&result);

        let main_fn = fdc.definitions.get("main").unwrap();
        assert!(
            is_entry_point("main", main_fn),
            "main() called in __name__ block should be an entry point"
        );

        let setup_fn = fdc.definitions.get("setup").unwrap();
        assert!(
            is_entry_point("setup", setup_fn),
            "setup() called in __name__ block should be an entry point"
        );

        // unused() is not called in the __name__ block and is not exported
        // (it is still "public" by Python naming, so it will be exported via is_exported)
    }

    #[test]
    fn test_python_decorated_functions_are_entry_points() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
@app.route("/hello")
def hello():
    return "hello"

@pytest.fixture
def my_fixture():
    return 42

@celery.task
def process_data():
    pass

@property
def name(self):
    return self._name

def plain_function():
    pass
"#;
        let result = parser
            .parse(content, Language::Python, Path::new("app.py"))
            .unwrap();
        let fdc = collect_file_data(&result);

        let hello_fn = fdc.definitions.get("hello").unwrap();
        assert!(
            is_entry_point("hello", hello_fn),
            "Flask @app.route decorated function should be an entry point, attrs: {:?}",
            hello_fn.attributes
        );

        let fixture_fn = fdc.definitions.get("my_fixture").unwrap();
        assert!(
            is_entry_point("my_fixture", fixture_fn),
            "@pytest.fixture decorated function should be an entry point, attrs: {:?}",
            fixture_fn.attributes
        );

        let task_fn = fdc.definitions.get("process_data").unwrap();
        assert!(
            is_entry_point("process_data", task_fn),
            "@celery.task decorated function should be an entry point, attrs: {:?}",
            task_fn.attributes
        );

        let prop_fn = fdc.definitions.get("name").unwrap();
        assert!(
            is_entry_point("name", prop_fn),
            "@property decorated function should be an entry point, attrs: {:?}",
            prop_fn.attributes
        );

        // plain_function has no decorator -- it is still "public" by Python naming,
        // so it will be exported via is_exported. Check that it has no decorator attrs.
        let plain_fn = fdc.definitions.get("plain_function").unwrap();
        assert!(
            plain_fn
                .attributes
                .iter()
                .all(|a| !is_entry_point_decorator(a)),
            "plain_function should have no entry-point decorators"
        );
    }

    #[test]
    fn test_max_nesting_depth_acceptable() {
        // Verify refactored code keeps nesting under 10 levels
        // This test documents the refactoring goal
        let source = include_str!("deadcode.rs");
        let max_nesting = measure_max_indent_depth(source);
        assert!(
            max_nesting <= 10,
            "deadcode.rs nesting depth {max_nesting} exceeds 10"
        );
    }

    fn measure_max_indent_depth(source: &str) -> usize {
        source
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let trimmed = l.trim_start();
                let indent = l.len() - trimmed.len();
                indent / 4 // 4 spaces per indent level
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn test_go_exported_is_entry_point() {
        let def = Definition {
            name: "PublicFunc".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("PublicFunc", &def));

        let private_def = Definition {
            name: "privateFunc".to_string(),
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
        assert!(!is_entry_point("privateFunc", &private_def));
    }

    #[test]
    fn test_go_benchmark_and_example_entry_points() {
        let def = Definition {
            name: "BenchmarkSort".to_string(),
            kind: "function".to_string(),
            file: "sort_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            is_test_file: true,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("BenchmarkSort", &def));

        let example_def = Definition {
            exported: true,
            ..def.clone()
        };
        assert!(is_entry_point("ExampleSort", &example_def));
    }

    #[test]
    fn test_typescript_function_detection() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
function publicHelper() {
    return 42;
}

export function exportedFunc() {
    return publicHelper();
}
"#;
        let result = parser
            .parse(content, Language::TypeScript, Path::new("util.ts"))
            .unwrap();
        let fdc = collect_file_data(&result);

        assert!(
            fdc.definitions.contains_key("publicHelper"),
            "Should find publicHelper, got: {:?}",
            fdc.definitions.keys().collect::<Vec<_>>()
        );
        assert!(
            fdc.definitions.contains_key("exportedFunc"),
            "Should find exportedFunc"
        );
    }

    #[test]
    fn test_java_function_detection() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
public class MyService {
    public void processData() {
        helperMethod();
    }

    private void helperMethod() {
        System.out.println("helper");
    }
}
"#;
        let result = parser
            .parse(content, Language::Java, Path::new("MyService.java"))
            .unwrap();
        let fdc = collect_file_data(&result);

        assert!(
            fdc.definitions.contains_key("processData"),
            "Should find processData, got: {:?}",
            fdc.definitions.keys().collect::<Vec<_>>()
        );
        assert!(
            fdc.definitions.contains_key("helperMethod"),
            "Should find helperMethod"
        );
    }

    #[test]
    fn test_python_function_detection() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
def public_func():
    _helper()

def _helper():
    pass

def __private():
    pass
"#;
        let result = parser
            .parse(content, Language::Python, Path::new("module.py"))
            .unwrap();
        let fdc = collect_file_data(&result);

        assert!(fdc.definitions.contains_key("public_func"));
        assert!(fdc.definitions.contains_key("_helper"));
        assert!(fdc.definitions.contains_key("__private"));

        // Check visibility
        assert_eq!(fdc.definitions["_helper"].visibility, "internal");
        assert_eq!(fdc.definitions["__private"].visibility, "private");
        assert_eq!(fdc.definitions["public_func"].visibility, "public");
    }

    #[test]
    fn test_rust_function_detection() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
pub fn public_api() {
    internal_helper();
}

fn internal_helper() {
    nested_helper();
}

fn nested_helper() {}
"#;
        let result = parser
            .parse(content, Language::Rust, Path::new("lib.rs"))
            .unwrap();
        let fdc = collect_file_data(&result);

        assert!(fdc.definitions.contains_key("public_api"));
        assert!(fdc.definitions.contains_key("internal_helper"));
        assert!(fdc.definitions.contains_key("nested_helper"));

        assert!(fdc.definitions["public_api"].exported);
        assert!(!fdc.definitions["internal_helper"].exported);
    }

    #[test]
    fn test_function_passed_as_value_creates_call_edge() {
        // When a function is passed as an argument to another function
        // (e.g., mcp.AddTool("name", handleSetWorkflowData)), the referenced
        // function should get a synthetic call edge so BFS can reach it.
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
package main

func registerTool(name string, handler func()) {
    // framework registration
}

func handleData() {
    // callback implementation
}

func setup() {
    registerTool("data", handleData)
}
"#;
        let result = parser
            .parse(content, Language::Go, Path::new("server.go"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // handleData is passed as an argument, not called directly.
        // There should be a call edge from setup -> handleData so that
        // if setup is reachable, handleData is too.
        let has_edge_to_handle_data = fdc
            .calls
            .iter()
            .any(|c| c.caller == "setup" && c.callee == "handleData");
        assert!(
            has_edge_to_handle_data,
            "Should create call edge for function passed as value. Calls: {:?}",
            fdc.calls
                .iter()
                .map(|c| format!("{} -> {}", c.caller, c.callee))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_go_function_detection() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
package main

func PublicFunc() {
    privateHelper()
}

func privateHelper() {
}
"#;
        let result = parser
            .parse(content, Language::Go, Path::new("main.go"))
            .unwrap();
        let fdc = collect_file_data(&result);

        assert!(
            fdc.definitions.contains_key("PublicFunc"),
            "Should find PublicFunc, got: {:?}",
            fdc.definitions.keys().collect::<Vec<_>>()
        );
        assert!(fdc.definitions.contains_key("privateHelper"));

        assert!(fdc.definitions["PublicFunc"].exported);
        assert!(!fdc.definitions["privateHelper"].exported);
    }

    #[test]
    fn test_generated_files_are_detected() {
        assert!(is_generated_file("conductor_grpc.pb.go"));
        assert!(is_generated_file("service.pb.go"));
        assert!(is_generated_file("types.gen.go"));
        assert!(is_generated_file("schema.generated.ts"));
        assert!(is_generated_file("models_gen.go"));
        assert!(is_generated_file("deep_copy_generated.go"));
        assert!(is_generated_file("bindata.go")); // go-bindata output
        assert!(is_generated_file("wire_gen.go")); // Wire DI
        assert!(is_generated_file("mock_service.go")); // mockgen
        assert!(is_generated_file("zz_generated.deepcopy.go")); // k8s code-gen

        // Non-generated files
        assert!(!is_generated_file("server.go"));
        assert!(!is_generated_file("handler.go"));
        assert!(!is_generated_file("main.go"));
        assert!(!is_generated_file("generator.go")); // contains "gen" but isn't generated
        assert!(!is_generated_file("utils.ts"));
    }

    #[test]
    fn test_generated_file_definitions_excluded_from_analysis() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
package pb

func mustEmbedUnimplementedConductorServiceServer() {
    panic("unimplemented")
}

func RegisterConductorServiceServer() {
    // generated registration
}
"#;
        let result = parser
            .parse(content, Language::Go, Path::new("conductor_grpc.pb.go"))
            .unwrap();
        let fdc = collect_file_data(&result);

        // Definitions from generated files should be marked so the analyzer
        // can skip them. We use an empty definitions map for generated files.
        assert!(
            fdc.definitions.is_empty(),
            "Generated file should have no definitions tracked, got: {:?}",
            fdc.definitions.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_handler_patterns_are_entry_points() {
        let base_def = Definition {
            name: "".to_string(),
            kind: "function".to_string(),
            file: "handler.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: false,
            attributes: vec![],
            is_trait_impl: false,
        };

        assert!(is_entry_point("myHandler", &base_def));
        assert!(is_entry_point("ServeHTTP", &base_def));
        assert!(is_entry_point("OnClick", &base_def));
        assert!(is_entry_point("HandleRequest", &base_def));
    }

    #[test]
    fn test_confidence_reduces_for_test_files() {
        let test_def = Definition {
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "helper_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            is_test_file: true,
            attributes: vec![],
            is_trait_impl: false,
        };
        let non_test_def = Definition {
            is_test_file: false,
            file: "helper.go".to_string(),
            ..test_def.clone()
        };

        let test_conf = calculate_confidence(&test_def, true, true);
        let non_test_conf = calculate_confidence(&non_test_def, true, true);
        assert!(
            test_conf < non_test_conf,
            "Test file confidence ({}) should be lower than non-test ({})",
            test_conf,
            non_test_conf
        );
    }

    #[test]
    fn test_ruby_visibility_modifiers() {
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
class MyClass
  def public_method
    puts "public"
  end

  private

  def secret_method
    puts "secret"
  end

  def another_secret
    puts "also secret"
  end

  protected

  def protected_method
    puts "protected"
  end
end
"#;
        let result = parser
            .parse(content, Language::Ruby, Path::new("my_class.rb"))
            .unwrap();
        let fdc = collect_file_data(&result);

        let public_fn = fdc.definitions.get("public_method").unwrap();
        assert_eq!(
            public_fn.visibility, "public",
            "Method before 'private' should be public"
        );
        assert!(public_fn.exported, "Public Ruby method should be exported");

        let secret_fn = fdc.definitions.get("secret_method").unwrap();
        assert_eq!(
            secret_fn.visibility, "private",
            "Method after 'private' should be private"
        );
        assert!(
            !secret_fn.exported,
            "Private Ruby method should not be exported"
        );

        let another_fn = fdc.definitions.get("another_secret").unwrap();
        assert_eq!(
            another_fn.visibility, "private",
            "Second method after 'private' should also be private"
        );

        let protected_fn = fdc.definitions.get("protected_method").unwrap();
        assert_eq!(
            protected_fn.visibility, "protected",
            "Method after 'protected' should be protected"
        );
        assert!(
            !protected_fn.exported,
            "Protected Ruby method should not be exported"
        );
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

        // Log stderr if cargo check had issues (but still parse stdout for warnings)
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                tracing::debug!("cargo check stderr: {}", stderr);
            }
        }

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
