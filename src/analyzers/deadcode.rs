//! Dead code detection analyzer.
//!
//! Finds unreachable/unused functions, variables, and classes using
//! reference graph analysis.

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

        // Phase 1: Collect definitions and usages from all files
        // Collect into Vec first for efficient parallel iteration
        let files: Vec<_> = ctx.files.iter().collect();
        let file_results: Vec<FileDeadCode> = files
            .par_iter()
            .filter_map(|path| self.analyze_file(path).ok())
            .collect();

        // Phase 2: Build global symbol tables
        let mut all_definitions: HashMap<String, Definition> = HashMap::new();
        let mut all_usages: HashSet<String> = HashSet::new();
        let mut all_calls: Vec<CallReference> = Vec::new();

        for fdc in &file_results {
            for (name, def) in &fdc.definitions {
                all_definitions.insert(name.clone(), def.clone());
            }
            all_usages.extend(fdc.usages.iter().cloned());
            all_calls.extend(fdc.calls.iter().cloned());
        }

        // Phase 3: Build call graph edges (for reachability)
        let mut call_graph: HashMap<String, Vec<String>> = HashMap::new();
        for call in &all_calls {
            call_graph
                .entry(call.caller.clone())
                .or_default()
                .push(call.callee.clone());
        }

        // Phase 4: Mark reachable from entry points
        let mut reachable: HashSet<String> = HashSet::new();
        let mut queue: Vec<String> = Vec::new();

        // Identify entry points
        for (name, def) in &all_definitions {
            if is_entry_point(name, def) {
                reachable.insert(name.clone());
                queue.push(name.clone());
            }
        }

        // BFS to mark reachable
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

        // Phase 5: Classify dead code
        let mut items = Vec::new();
        let mut by_kind: HashMap<String, usize> = HashMap::new();

        for (name, def) in &all_definitions {
            // Skip entry points
            if is_entry_point(name, def) {
                continue;
            }

            // Check if unreachable AND not used
            let is_unreachable = !reachable.contains(name);
            let is_unused = !all_usages.contains(name);

            if is_unreachable || is_unused {
                let confidence = calculate_confidence(def, is_unreachable, is_unused);

                if confidence >= self.confidence_threshold {
                    let item = DeadCodeItem {
                        name: name.clone(),
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

    // Extract function definitions
    for func in functions {
        let visibility = get_visibility(&func.name, result.language);
        let exported = is_exported(&func.name, result.language);

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
                is_test_file,
            },
        );
    }

    // Extract usages and calls by walking the AST
    collect_usages_and_calls(result, &mut fdc);

    fdc
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

    // Test functions
    if name.starts_with("Test") || name.starts_with("test") {
        return true;
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
}

#[derive(Clone)]
struct CallReference {
    caller: String,
    callee: String,
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
        };
        let conf2 = calculate_confidence(&exported_def, false, true);
        assert!(conf2 < 0.7); // Lower confidence for exported
    }
}
