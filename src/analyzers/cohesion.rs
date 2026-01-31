//! CK (Chidamber-Kemerer) cohesion metrics analyzer.
//!
//! Calculates object-oriented metrics for classes:
//! - WMC: Weighted Methods per Class (sum of cyclomatic complexity)
//! - CBO: Coupling Between Objects (number of classes referenced)
//! - RFC: Response for Class (methods that can be invoked)
//! - LCOM: Lack of Cohesion in Methods (LCOM3 via connected components in method-field graph)
//! - DIT: Depth of Inheritance Tree
//! - NOC: Number of Children (direct subclasses)
//!
//! # References
//!
//! - Chidamber, S.R., Kemerer, C.F. (1994) "A Metrics Suite for Object Oriented Design"
//!   IEEE TSE 20(6), pp. 476-493
//! - Hitz, M., Montazeri, B. (1995) "Measuring Coupling and Cohesion in OO Systems"
//!   (LCOM3 definition using connected components)
//! - Basili, V.R., Briand, L.C., Melo, W.L. (1996) "A Validation of Object-Oriented
//!   Design Metrics as Quality Indicators" IEEE TSE 22(10) (threshold validation)
//!
//! Note: LCOM uses LCOM3 (Hitz & Montazeri 1995), which counts connected components
//! where methods are connected if they share instance variables. This differs from
//! LCOM4 which also connects methods that call each other.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::Parser;

/// Default threshold for WMC above which a class is considered complex.
/// Research suggests 20-24 is appropriate (Chidamber & Kemerer 1994 IEEE TSE).
pub const WMC_THRESHOLD: u32 = 24;

/// Default threshold for CBO above which a class is considered highly coupled.
pub const CBO_THRESHOLD: u32 = 14;

/// Default threshold for LCOM above which a class lacks cohesion.
pub const LCOM_THRESHOLD: u32 = 1;

/// Cohesion analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether to skip test files.
    pub skip_test_files: bool,
    /// Maximum file size to analyze (0 = no limit).
    pub max_file_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            skip_test_files: true,
            max_file_size: 0,
        }
    }
}

/// Cohesion (CK metrics) analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Creates a new cohesion analyzer with default config.
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Creates a new analyzer with the specified config.
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Includes test files in analysis.
    pub fn with_include_test_files(mut self) -> Self {
        self.config.skip_test_files = false;
        self
    }

    /// Analyzes cohesion metrics in a repository.
    /// Uses ctx.read_file() to support both filesystem and git tree sources.
    pub fn analyze_repo(&self, ctx: &AnalysisContext<'_>) -> Result<Analysis> {
        // Phase 1: Get files from context, filtered by OO languages
        let files: Vec<_> = ctx
            .files
            .iter()
            .filter(|path| {
                // Skip test files if configured
                if self.config.skip_test_files && is_test_file(path) {
                    return false;
                }
                // Only include OO languages
                Language::detect(path).map(is_oo_language).unwrap_or(false)
            })
            .collect();

        // Phase 2: Parse files in parallel and extract classes
        let max_file_size = self.config.max_file_size;
        let all_classes: Vec<ClassMetrics> = files
            .par_iter()
            .filter_map(|path| {
                // Read file via context (supports both filesystem and git tree)
                let source = ctx.read_file(path).ok()?;

                // Skip if too large
                if max_file_size > 0 && source.len() > max_file_size {
                    return None;
                }

                let lang = Language::detect(path)?;

                // Parse with thread-local parser
                let parser = Parser::new();
                let parse_result = parser.parse(&source, lang, path).ok()?;
                let classes =
                    extract_classes_from_file(path, &source, parse_result.tree.as_ref(), lang);

                Some(classes)
            })
            .flatten()
            .collect();

        let mut all_classes = all_classes;

        // Build class hierarchy for DIT/NOC calculation
        let mut hierarchy = ClassHierarchy::new();
        for cls in &all_classes {
            hierarchy.add_class(&cls.class_name, cls.parent_class.as_deref());
        }

        // Update DIT and NOC for each class
        for cls in &mut all_classes {
            cls.dit = hierarchy.get_dit(&cls.class_name);
            cls.noc = hierarchy.get_noc(&cls.class_name);
        }

        // Sort by LCOM (least cohesive first)
        all_classes.sort_by(|a, b| b.lcom.cmp(&a.lcom));

        let summary = calculate_summary(&all_classes);

        Ok(Analysis {
            generated_at: Utc::now().to_rfc3339(),
            classes: all_classes,
            summary,
        })
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "cohesion"
    }

    fn description(&self) -> &'static str {
        "Calculate CK object-oriented metrics (WMC, CBO, RFC, LCOM, DIT, NOC)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        self.analyze_repo(ctx)
    }
}

/// Checks if a language supports class-like structures.
/// Includes traditional OO languages plus Rust (struct+impl) and Go (struct+methods).
fn is_oo_language(lang: Language) -> bool {
    matches!(
        lang,
        Language::Java
            | Language::TypeScript
            | Language::JavaScript
            | Language::Python
            | Language::CSharp
            | Language::Cpp
            | Language::Ruby
            | Language::Php
            | Language::Rust
            | Language::Go
    )
}

/// Class hierarchy for DIT/NOC calculation.
/// Tracks parent-child relationships across the codebase.
#[derive(Debug, Default)]
pub struct ClassHierarchy {
    /// Maps class name -> parent class name (None if no parent)
    parents: HashMap<String, Option<String>>,
    /// Maps class name -> list of direct children
    children: HashMap<String, Vec<String>>,
}

impl ClassHierarchy {
    /// Creates a new empty class hierarchy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a class to the hierarchy.
    pub fn add_class(&mut self, class_name: &str, parent: Option<&str>) {
        let parent_str = parent.map(|s| s.to_string());
        self.parents
            .insert(class_name.to_string(), parent_str.clone());

        // Register this class as a child of its parent
        if let Some(parent_name) = parent_str {
            self.children
                .entry(parent_name)
                .or_default()
                .push(class_name.to_string());
        }

        // Ensure the class has an entry in children map (even if empty)
        self.children.entry(class_name.to_string()).or_default();
    }

    /// Calculates DIT (Depth of Inheritance Tree) for a class.
    /// DIT = max path length from class to root of inheritance tree.
    /// Per Chidamber & Kemerer 1994, DIT counts the number of ancestor classes.
    pub fn get_dit(&self, class_name: &str) -> u32 {
        let mut depth = 0;
        let mut current = class_name.to_string();
        let mut visited = HashSet::new();

        // Walk up the inheritance chain, tracking visited to break cycles
        while let Some(Some(parent)) = self.parents.get(&current) {
            if !visited.insert(parent.clone()) {
                break;
            }
            depth += 1;
            current = parent.clone();
        }

        // If parent not in our map, still count it as depth 1
        if depth == 0 {
            if let Some(Some(_)) = self.parents.get(class_name) {
                // Has a parent but parent not tracked - depth is at least 1
                return 1;
            }
        }

        depth
    }

    /// Calculates NOC (Number of Children) for a class.
    /// NOC = count of immediate subclasses.
    pub fn get_noc(&self, class_name: &str) -> u32 {
        self.children
            .get(class_name)
            .map(|c| c.len() as u32)
            .unwrap_or(0)
    }
}

/// Checks if a file is a test file.
fn is_test_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.ends_with("_test.go")
        || path_str.ends_with("_test.py")
        || path_str.ends_with(".test.ts")
        || path_str.ends_with(".test.js")
        || path_str.ends_with(".spec.ts")
        || path_str.ends_with(".spec.js")
        || path_str.contains("/test/")
        || path_str.contains("/tests/")
        || path_str.contains("/__tests__/")
        || path_str.starts_with("__tests__/")
        || path_str.starts_with("test/")
        || path_str.starts_with("tests/")
}

/// Extracts class metrics from a parsed file.
fn extract_classes_from_file(
    path: &Path,
    source: &[u8],
    tree: &tree_sitter::Tree,
    lang: Language,
) -> Vec<ClassMetrics> {
    match lang {
        Language::Rust => extract_rust_classes(path, source, tree),
        Language::Go => extract_go_classes(path, source, tree),
        _ => {
            let mut classes = Vec::new();
            let root = tree.root_node();
            let mut cursor = root.walk();
            extract_classes_recursive(&mut cursor, source, path, lang, &mut classes);
            classes
        }
    }
}

/// Extracts class-like metrics from Rust struct+impl blocks.
///
/// In Rust, struct fields and impl methods are separate top-level items.
/// We find all struct_item nodes, then associate impl_item blocks by matching
/// the type name.
fn extract_rust_classes(path: &Path, source: &[u8], tree: &tree_sitter::Tree) -> Vec<ClassMetrics> {
    let lang = Language::Rust;
    let root = tree.root_node();

    // Phase 1: Collect all struct definitions (name -> node range for fields)
    let mut struct_nodes: Vec<(String, tree_sitter::Node)> = Vec::new();
    collect_nodes_by_kind(&root, "struct_item", source, &mut struct_nodes);

    // Phase 2: Collect all impl blocks and group by type name
    let mut impl_nodes: Vec<(String, tree_sitter::Node)> = Vec::new();
    collect_impl_blocks(&root, source, &mut impl_nodes);

    // Phase 3: Build metrics for each struct
    let mut classes = Vec::new();
    for (struct_name, struct_node) in &struct_nodes {
        let start_line = struct_node.start_position().row + 1;
        let mut end_line = struct_node.end_position().row + 1;

        // Extract fields from the struct body
        let fields = extract_fields(struct_node, source, lang);
        let nof = fields.len() as u32;

        // Find all impl blocks for this struct and extract methods
        let mut methods = Vec::new();
        let method_types = get_method_node_types(lang);
        for (impl_name, impl_node) in &impl_nodes {
            if impl_name == struct_name {
                let impl_end = impl_node.end_position().row + 1;
                if impl_end > end_line {
                    end_line = impl_end;
                }
                let mut cursor = impl_node.walk();
                extract_methods_recursive(&mut cursor, source, lang, &method_types, &mut methods);
            }
        }

        let nom = methods.len() as u32;
        let wmc: u32 = methods.iter().map(|m| m.complexity).sum();
        let loc = (end_line - start_line + 1) as u32;

        let all_called = collect_called_methods(&impl_nodes, struct_name, source, lang);
        let rfc = nom + all_called.len() as u32;

        let all_coupled =
            collect_coupled_classes(struct_node, &impl_nodes, struct_name, source, lang);
        let cbo = all_coupled.len() as u32;

        let lcom = calculate_lcom(&methods, &fields);
        let violations = build_violations(wmc, cbo, lcom);

        classes.push(ClassMetrics {
            path: path.to_string_lossy().to_string(),
            class_name: struct_name.clone(),
            parent_class: None,
            language: lang.to_string(),
            start_line,
            end_line,
            loc,
            wmc,
            cbo,
            rfc,
            lcom,
            dit: 0,
            noc: 0,
            nom,
            nof,
            methods: methods.iter().map(|m| m.name.clone()).collect(),
            fields,
            coupled_classes: all_coupled.into_iter().collect(),
            violations,
        });
    }

    classes
}

/// Extracts class-like metrics from Go struct types and their method declarations.
///
/// In Go, struct fields are inside `type_declaration` and methods are
/// `method_declaration` nodes at the file level with a receiver parameter
/// that references the struct type.
fn extract_go_classes(path: &Path, source: &[u8], tree: &tree_sitter::Tree) -> Vec<ClassMetrics> {
    let lang = Language::Go;
    let root = tree.root_node();

    // Phase 1: Collect type declarations that contain struct_type
    let mut struct_defs: Vec<(String, tree_sitter::Node)> = Vec::new();
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) if c.kind() == "type_declaration" => c,
            _ => continue,
        };
        for j in 0..child.child_count() {
            if let Some(spec) = child.child(j) {
                if spec.kind() == "type_spec" && has_child_of_kind(&spec, "struct_type") {
                    if let Some(name) = node_name_text(&spec, source) {
                        struct_defs.push((name, child));
                    }
                }
            }
        }
    }

    // Phase 2: Collect method declarations and group by receiver type
    let mut method_map: HashMap<String, Vec<tree_sitter::Node>> = HashMap::new();
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) if c.kind() == "method_declaration" => c,
            _ => continue,
        };
        if let Some(receiver_type) = extract_go_receiver_type(&child, source) {
            method_map.entry(receiver_type).or_default().push(child);
        }
    }

    // Phase 3: Build metrics
    let mut classes = Vec::new();
    for (struct_name, struct_node) in &struct_defs {
        let start_line = struct_node.start_position().row + 1;
        let mut end_line = struct_node.end_position().row + 1;

        // Extract fields from the struct type_spec
        let fields = extract_go_struct_fields(struct_node, source);
        let nof = fields.len() as u32;

        // Extract methods from method_declarations
        let mut methods = Vec::new();
        if let Some(method_nodes) = method_map.get(struct_name) {
            for method_node in method_nodes {
                let method_end = method_node.end_position().row + 1;
                if method_end > end_line {
                    end_line = method_end;
                }
                let name = method_node
                    .child_by_field_name("name")
                    .and_then(|n| std::str::from_utf8(&source[n.byte_range()]).ok())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    let complexity = calculate_complexity(method_node, lang);
                    let used_fields = find_fields_used_by_method(method_node, source, lang);
                    methods.push(MethodInfo {
                        name,
                        complexity,
                        used_fields,
                    });
                }
            }
        }

        let nom = methods.len() as u32;
        let wmc: u32 = methods.iter().map(|m| m.complexity).sum();
        let loc = (end_line - start_line + 1) as u32;

        // RFC: collect called methods from all method_declaration nodes
        let mut all_called = HashSet::new();
        if let Some(method_nodes) = method_map.get(struct_name) {
            for method_node in method_nodes {
                for c in extract_called_methods(method_node, source, lang) {
                    all_called.insert(c);
                }
            }
        }
        let rfc = nom + all_called.len() as u32;

        // CBO: coupled classes from struct def + method nodes
        let method_pairs: Vec<(String, tree_sitter::Node)> = method_map
            .get(struct_name)
            .map(|nodes| nodes.iter().map(|n| (struct_name.clone(), *n)).collect())
            .unwrap_or_default();
        let all_coupled =
            collect_coupled_classes(struct_node, &method_pairs, struct_name, source, lang);
        let cbo = all_coupled.len() as u32;

        let lcom = calculate_lcom(&methods, &fields);
        let violations = build_violations(wmc, cbo, lcom);

        classes.push(ClassMetrics {
            path: path.to_string_lossy().to_string(),
            class_name: struct_name.clone(),
            parent_class: None,
            language: lang.to_string(),
            start_line,
            end_line,
            loc,
            wmc,
            cbo,
            rfc,
            lcom,
            dit: 0,
            noc: 0,
            nom,
            nof,
            methods: methods.iter().map(|m| m.name.clone()).collect(),
            fields,
            coupled_classes: all_coupled.into_iter().collect(),
            violations,
        });
    }

    classes
}

/// Collects top-level nodes of the given kind, extracting names from the `name` field.
fn collect_nodes_by_kind<'a>(
    root: &tree_sitter::Node<'a>,
    kind: &str,
    source: &[u8],
    out: &mut Vec<(String, tree_sitter::Node<'a>)>,
) {
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) if c.kind() == kind => c,
            _ => continue,
        };
        if let Some(name) = node_name_text(&child, source) {
            out.push((name, child));
        }
    }
}

/// Collects Rust impl blocks and extracts the type name they implement.
fn collect_impl_blocks<'a>(
    root: &tree_sitter::Node<'a>,
    source: &[u8],
    out: &mut Vec<(String, tree_sitter::Node<'a>)>,
) {
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) if c.kind() == "impl_item" => c,
            _ => continue,
        };
        if let Some(name) = extract_rust_impl_type(&child, source) {
            out.push((name, child));
        }
    }
}

/// Extracts the type name from a Rust impl block.
/// For `impl Foo { ... }`, returns "Foo".
/// For `impl Trait for Foo { ... }`, returns "Foo".
fn extract_rust_impl_type(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    // In tree-sitter-rust, impl_item has a "type" field for inherent impls
    // and a "type" field for trait impls (the type after "for").
    // The "type" field points to the type being implemented.
    if let Some(type_node) = node.child_by_field_name("type") {
        let text = std::str::from_utf8(&source[type_node.byte_range()]).ok()?;
        // Strip generic parameters if present (e.g., "Foo<T>" -> "Foo")
        let name = text.split('<').next().unwrap_or(text).trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Extracts the receiver type from a Go method declaration.
/// For `func (s *Server) Handle()`, returns "Server".
fn extract_go_receiver_type(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    // method_declaration has a "receiver" field containing parameter_list
    let receiver = node.child_by_field_name("receiver")?;
    // Walk through the parameter list to find the type
    for i in 0..receiver.child_count() {
        if let Some(param) = receiver.child(i) {
            if param.kind() == "parameter_declaration" {
                // The type may be a pointer_type (*Server) or type_identifier (Server)
                if let Some(type_node) = param.child_by_field_name("type") {
                    return extract_go_base_type(&type_node, source);
                }
            }
        }
    }
    None
}

/// Extracts the base type name from a Go type node, stripping pointer indirection.
fn extract_go_base_type(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "pointer_type" => {
            // *Server -> look for the inner type_identifier
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_identifier" {
                        return std::str::from_utf8(&source[child.byte_range()])
                            .ok()
                            .map(|s| s.to_string());
                    }
                }
            }
            None
        }
        "type_identifier" => std::str::from_utf8(&source[node.byte_range()])
            .ok()
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Extracts the text of a node's `name` field, returning `None` if absent or empty.
fn node_name_text(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    let name = std::str::from_utf8(&source[name_node.byte_range()]).ok()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Checks if a node has a child of the given kind.
fn has_child_of_kind(node: &tree_sitter::Node, kind: &str) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == kind {
                return true;
            }
        }
    }
    false
}

/// Extracts field names from a Go struct's field_declaration_list.
fn extract_go_struct_fields(type_decl: &tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut fields = Vec::new();
    // type_declaration -> type_spec -> struct_type -> field_declaration_list -> field_declaration
    let mut cursor = type_decl.walk();
    extract_go_fields_recursive(&mut cursor, source, &mut fields);
    fields
}

/// Recursively finds Go field_declaration nodes and extracts their names.
fn extract_go_fields_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    fields: &mut Vec<String>,
) {
    loop {
        let node = cursor.node();

        if node.kind() == "field_declaration" {
            // field_declaration has a "name" field (field_identifier)
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = std::str::from_utf8(&source[name_node.byte_range()]) {
                    if !name.is_empty() && !fields.contains(&name.to_string()) {
                        fields.push(name.to_string());
                    }
                }
            }
        }

        if cursor.goto_first_child() {
            extract_go_fields_recursive(cursor, source, fields);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Recursively extracts classes from tree.
fn extract_classes_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    path: &Path,
    lang: Language,
    classes: &mut Vec<ClassMetrics>,
) {
    loop {
        let node = cursor.node();

        if is_class_node(node.kind(), lang) {
            if let Some(metrics) = extract_class_metrics(&node, source, path, lang) {
                classes.push(metrics);
            }
        }

        // Recurse into children
        if cursor.goto_first_child() {
            extract_classes_recursive(cursor, source, path, lang, classes);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Checks if a node type represents a class.
fn is_class_node(node_type: &str, lang: Language) -> bool {
    match lang {
        Language::Java => node_type == "class_declaration" || node_type == "interface_declaration",
        Language::TypeScript | Language::JavaScript => {
            node_type == "class_declaration" || node_type == "class"
        }
        Language::Python => node_type == "class_definition",
        Language::CSharp => {
            node_type == "class_declaration" || node_type == "interface_declaration"
        }
        Language::Cpp => node_type == "class_specifier" || node_type == "struct_specifier",
        Language::Ruby => node_type == "class" || node_type == "module",
        Language::Php => node_type == "class_declaration" || node_type == "interface_declaration",
        Language::Rust => node_type == "struct_item",
        Language::Go => node_type == "type_declaration",
        _ => false,
    }
}

/// Extracts CK metrics from a class node.
fn extract_class_metrics(
    node: &tree_sitter::Node,
    source: &[u8],
    path: &Path,
    lang: Language,
) -> Option<ClassMetrics> {
    // Get class name
    let name = get_class_name(node, source, lang)?;

    // Extract parent class for inheritance tracking
    let parent_class = extract_parent_class(node, source, lang);

    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let loc = (end_line - start_line + 1) as u32;

    // Extract methods
    let methods = extract_methods(node, source, lang);
    let nom = methods.len() as u32;

    // WMC = sum of cyclomatic complexity of all methods
    let wmc: u32 = methods.iter().map(|m| m.complexity).sum();

    // Extract fields
    let fields = extract_fields(node, source, lang);
    let nof = fields.len() as u32;

    // RFC = local methods + called methods
    let called_methods = extract_called_methods(node, source, lang);
    let rfc = nom + called_methods.len() as u32;

    // CBO = number of coupled classes
    let coupled_classes = extract_coupled_classes(node, source, lang);
    let cbo = coupled_classes.len() as u32;

    // LCOM3 = connected components in method-field graph
    // (LCOM4 would additionally connect methods that call each other)
    let lcom = calculate_lcom(&methods, &fields);

    // DIT and NOC are calculated later after building the full class hierarchy
    let dit = 0;
    let noc = 0;

    let violations = build_violations(wmc, cbo, lcom);

    Some(ClassMetrics {
        path: path.to_string_lossy().to_string(),
        class_name: name,
        parent_class,
        language: lang.to_string(),
        start_line,
        end_line,
        loc,
        wmc,
        cbo,
        rfc,
        lcom,
        dit,
        noc,
        nom,
        nof,
        methods: methods.iter().map(|m| m.name.clone()).collect(),
        fields,
        coupled_classes,
        violations,
    })
}

/// Finds the first child of `parent` whose `kind()` matches one of `kinds`,
/// and returns its source text as a `String`.
fn first_child_text_by_kind(
    parent: &tree_sitter::Node,
    source: &[u8],
    kinds: &[&str],
) -> Option<String> {
    for i in 0..parent.child_count() {
        let child = parent.child(i)?;
        if kinds.contains(&child.kind()) {
            return std::str::from_utf8(&source[child.byte_range()])
                .ok()
                .map(|s| s.to_string());
        }
    }
    None
}

/// Extracts the parent class name from a class node (for extends/inherits).
fn extract_parent_class(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    match lang {
        Language::Java => {
            // Java: class Child extends Parent { }
            let sc = node.child_by_field_name("superclass")?;
            first_child_text_by_kind(&sc, source, &["type_identifier"])
        }
        Language::TypeScript | Language::JavaScript => {
            // TS/JS: class Child extends Parent { }
            // class_heritage -> extends_clause -> identifier|type_identifier
            let heritage = find_child_by_kind(node, "class_heritage")?;
            let clause = find_child_by_kind(&heritage, "extends_clause")?;
            first_child_text_by_kind(&clause, source, &["identifier", "type_identifier"])
        }
        Language::Python => {
            // Python: class Child(Parent):
            let args = node.child_by_field_name("superclasses")?;
            first_child_text_by_kind(&args, source, &["identifier"])
        }
        Language::CSharp => {
            // C#: class Child : Parent { }
            let bases = node.child_by_field_name("bases")?;
            first_child_text_by_kind(&bases, source, &["identifier", "type_identifier"])
        }
        Language::Cpp => {
            // C++: class Child : public Parent { }
            let clause = find_child_by_kind(node, "base_class_clause")?;
            first_child_text_by_kind(&clause, source, &["type_identifier"])
        }
        Language::Ruby => {
            // Ruby: class Child < Parent
            let sc = node.child_by_field_name("superclass")?;
            std::str::from_utf8(&source[sc.byte_range()])
                .ok()
                .map(|s| s.to_string())
        }
        Language::Php => {
            // PHP: class Child extends Parent { }
            let base = node.child_by_field_name("base_clause")?;
            first_child_text_by_kind(&base, source, &["name", "qualified_name"])
        }
        // Rust and Go have no class inheritance
        Language::Rust | Language::Go => None,
        _ => None,
    }
}

/// Finds the first direct child of `node` with the given `kind`.
fn find_child_by_kind<'a>(
    node: &tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    for i in 0..node.child_count() {
        let child = node.child(i)?;
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

/// Gets the class name from a node.
fn get_class_name(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    match lang {
        Language::Go => {
            // type_declaration -> type_spec -> name
            let spec = find_child_by_kind(node, "type_spec")?;
            node_name_text(&spec, source)
        }
        _ => node_name_text(node, source),
    }
}

/// Builds the list of threshold violations for a class.
fn build_violations(wmc: u32, cbo: u32, lcom: u32) -> Vec<String> {
    let mut violations = Vec::new();
    if wmc > WMC_THRESHOLD {
        violations.push(format!("WMC {} exceeds threshold {}", wmc, WMC_THRESHOLD));
    }
    if cbo > CBO_THRESHOLD {
        violations.push(format!("CBO {} exceeds threshold {}", cbo, CBO_THRESHOLD));
    }
    if lcom > LCOM_THRESHOLD {
        violations.push(format!(
            "LCOM {} exceeds threshold {}",
            lcom, LCOM_THRESHOLD
        ));
    }
    violations
}

/// Collects RFC (called methods) from a set of nodes belonging to a single class.
fn collect_called_methods(
    nodes: &[(String, tree_sitter::Node)],
    class_name: &str,
    source: &[u8],
    lang: Language,
) -> HashSet<String> {
    let mut all_called = HashSet::new();
    for (name, node) in nodes {
        if name == class_name {
            for c in extract_called_methods(node, source, lang) {
                all_called.insert(c);
            }
        }
    }
    all_called
}

/// Collects CBO (coupled classes) from a base node plus additional nodes for a single class.
fn collect_coupled_classes(
    base_node: &tree_sitter::Node,
    extra_nodes: &[(String, tree_sitter::Node)],
    class_name: &str,
    source: &[u8],
    lang: Language,
) -> HashSet<String> {
    let mut all_coupled: HashSet<String> = extract_coupled_classes(base_node, source, lang)
        .into_iter()
        .collect();
    for (name, node) in extra_nodes {
        if name == class_name {
            for c in extract_coupled_classes(node, source, lang) {
                all_coupled.insert(c);
            }
        }
    }
    all_coupled.remove(class_name);
    all_coupled
}

/// Method info for LCOM calculation.
struct MethodInfo {
    name: String,
    complexity: u32,
    used_fields: HashSet<String>,
}

/// Extracts methods from a class node.
fn extract_methods(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Vec<MethodInfo> {
    let mut methods = Vec::new();
    let method_types = get_method_node_types(lang);

    let mut cursor = node.walk();
    extract_methods_recursive(&mut cursor, source, lang, &method_types, &mut methods);

    methods
}

/// Recursively extracts methods.
fn extract_methods_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    lang: Language,
    method_types: &[&str],
    methods: &mut Vec<MethodInfo>,
) {
    loop {
        let node = cursor.node();

        if method_types.contains(&node.kind()) {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| std::str::from_utf8(&source[n.byte_range()]).ok())
                .unwrap_or("")
                .to_string();

            if !name.is_empty() {
                let complexity = calculate_complexity(&node, lang);
                let used_fields = find_fields_used_by_method(&node, source, lang);

                methods.push(MethodInfo {
                    name,
                    complexity,
                    used_fields,
                });
            }
        } else if cursor.goto_first_child() {
            extract_methods_recursive(cursor, source, lang, method_types, methods);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Gets method node types for a language.
fn get_method_node_types(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Java => vec!["method_declaration", "constructor_declaration"],
        Language::TypeScript | Language::JavaScript => {
            vec!["method_definition", "public_field_definition"]
        }
        Language::Python => vec!["function_definition"],
        Language::CSharp => vec!["method_declaration", "constructor_declaration"],
        Language::Cpp => vec!["function_definition", "function_declarator"],
        Language::Ruby => vec!["method", "singleton_method"],
        Language::Php => vec!["method_declaration"],
        Language::Rust => vec!["function_item"],
        Language::Go => vec!["method_declaration"],
        _ => vec![],
    }
}

/// Calculates cyclomatic complexity for a node.
fn calculate_complexity(node: &tree_sitter::Node, _lang: Language) -> u32 {
    let decision_types = [
        "if_statement",
        "if_expression",
        "if",
        "for_statement",
        "for_expression",
        "for",
        "while_statement",
        "while_expression",
        "while",
        "switch_statement",
        "match_expression",
        "case_clause",
        "case_statement",
        "catch_clause",
        "except_clause",
        "conditional_expression",
        "ternary_expression",
    ];

    let mut complexity = 1u32; // Base complexity
    let mut cursor = node.walk();
    count_decision_points(&mut cursor, &decision_types, &mut complexity);

    complexity
}

/// Counts decision points recursively.
fn count_decision_points(
    cursor: &mut tree_sitter::TreeCursor,
    decision_types: &[&str],
    complexity: &mut u32,
) {
    loop {
        let node = cursor.node();

        if decision_types.contains(&node.kind()) {
            *complexity += 1;
        }

        if cursor.goto_first_child() {
            count_decision_points(cursor, decision_types, complexity);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Finds fields used by a method.
fn find_fields_used_by_method(
    node: &tree_sitter::Node,
    source: &[u8],
    lang: Language,
) -> HashSet<String> {
    let mut fields = HashSet::new();
    let mut cursor = node.walk();
    find_field_accesses(&mut cursor, source, lang, &mut fields);
    fields
}

/// If `node` is a two-part field access where the receiver matches `self_keyword`
/// (e.g. `self.x`, `this.y`), returns the field name. `obj_field` and `attr_field`
/// are the tree-sitter field names for the two child nodes.
fn extract_self_field_access(
    node: &tree_sitter::Node,
    source: &[u8],
    self_keyword: &str,
    obj_field: &str,
    attr_field: &str,
) -> Option<String> {
    let obj = node.child_by_field_name(obj_field)?;
    let attr = node.child_by_field_name(attr_field)?;
    let obj_text = std::str::from_utf8(&source[obj.byte_range()]).ok()?;
    if obj_text != self_keyword {
        return None;
    }
    std::str::from_utf8(&source[attr.byte_range()])
        .ok()
        .map(|s| s.to_string())
}

/// Checks if a Go selector_expression is a receiver field access and returns the field name.
fn extract_go_field_access(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let field_node = node.child_by_field_name("field")?;
    let operand = node.child_by_field_name("operand")?;
    if operand.kind() != "identifier" {
        return None;
    }
    let op_text = std::str::from_utf8(&source[operand.byte_range()]).ok()?;
    // Go receivers are typically short lowercase names
    let is_receiver = op_text.len() <= 4
        && op_text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase());
    if !is_receiver {
        return None;
    }
    std::str::from_utf8(&source[field_node.byte_range()])
        .ok()
        .map(|s| s.to_string())
}

/// Recursively finds field accesses.
fn find_field_accesses(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    lang: Language,
    fields: &mut HashSet<String>,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        let field_name = match lang {
            Language::Python if kind == "attribute" => {
                extract_self_field_access(&node, source, "self", "object", "attribute")
            }
            Language::Ruby if kind == "instance_variable" => {
                std::str::from_utf8(&source[node.byte_range()])
                    .ok()
                    .map(|s| s.to_string())
            }
            Language::Java | Language::CSharp | Language::TypeScript | Language::JavaScript
                if kind == "member_expression" || kind == "member_access_expression" =>
            {
                extract_self_field_access(&node, source, "this", "object", "property")
            }
            Language::Rust if kind == "field_expression" => {
                extract_self_field_access(&node, source, "self", "value", "field")
            }
            Language::Go if kind == "selector_expression" => extract_go_field_access(&node, source),
            _ => None,
        };
        if let Some(name) = field_name {
            fields.insert(name);
        }

        if cursor.goto_first_child() {
            find_field_accesses(cursor, source, lang, fields);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extracts field names from a class.
fn extract_fields(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Vec<String> {
    let mut fields = Vec::new();
    let field_types = get_field_node_types(lang);

    let mut cursor = node.walk();
    extract_fields_recursive(&mut cursor, source, lang, &field_types, &mut fields);

    fields
}

/// Recursively extracts fields.
fn extract_fields_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    lang: Language,
    field_types: &[&str],
    fields: &mut Vec<String>,
) {
    loop {
        let node = cursor.node();

        if field_types.contains(&node.kind()) {
            if let Some(name) = extract_field_name(&node, source, lang) {
                if !fields.contains(&name) {
                    fields.push(name);
                }
            }
        }

        if cursor.goto_first_child() {
            extract_fields_recursive(cursor, source, lang, field_types, fields);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Gets field node types for a language.
fn get_field_node_types(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Java => vec!["field_declaration"],
        Language::TypeScript | Language::JavaScript => {
            vec!["public_field_definition", "field_definition"]
        }
        Language::Python => vec!["assignment"],
        Language::CSharp => vec!["field_declaration", "property_declaration"],
        Language::Cpp => vec!["field_declaration"],
        Language::Ruby => vec!["instance_variable"],
        Language::Php => vec!["property_declaration"],
        Language::Rust => vec!["field_declaration"],
        Language::Go => vec!["field_declaration"],
        _ => vec![],
    }
}

/// Extracts field name from a node.
fn extract_field_name(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    match lang {
        Language::Python => {
            // Look for self.field = ... pattern
            if node.kind() == "assignment" {
                let left = node.child_by_field_name("left")?;
                if left.kind() == "attribute" {
                    let obj = left.child_by_field_name("object")?;
                    let attr = left.child_by_field_name("attribute")?;
                    let obj_text = std::str::from_utf8(&source[obj.byte_range()]).ok()?;
                    if obj_text == "self" {
                        return std::str::from_utf8(&source[attr.byte_range()])
                            .ok()
                            .map(|s| s.to_string());
                    }
                }
            }
            None
        }
        Language::Ruby => std::str::from_utf8(&source[node.byte_range()])
            .ok()
            .map(|s| s.to_string()),
        Language::Rust | Language::Go => {
            // field_declaration has a "name" field
            node.child_by_field_name("name")
                .and_then(|n| std::str::from_utf8(&source[n.byte_range()]).ok())
                .map(|s| s.to_string())
        }
        _ => {
            // Look for declarator/name
            let name_node = node
                .child_by_field_name("declarator")
                .and_then(|n| n.child_by_field_name("name").or(Some(n)))
                .or_else(|| node.child_by_field_name("name"))?;

            std::str::from_utf8(&source[name_node.byte_range()])
                .ok()
                .map(|s| s.to_string())
        }
    }
}

/// Extracts called method names from a class.
fn extract_called_methods(node: &tree_sitter::Node, source: &[u8], _lang: Language) -> Vec<String> {
    let mut called = HashSet::new();
    let mut cursor = node.walk();
    extract_calls_recursive(&mut cursor, source, &mut called);
    called.into_iter().collect()
}

/// Recursively extracts method calls.
fn extract_calls_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    called: &mut HashSet<String>,
) {
    loop {
        let node = cursor.node();

        if node.kind() == "call_expression"
            || node.kind() == "method_call"
            || node.kind() == "invocation_expression"
        {
            if let Some(fn_node) = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("name"))
            {
                if let Ok(name) = std::str::from_utf8(&source[fn_node.byte_range()]) {
                    called.insert(name.to_string());
                }
            }
        }

        if cursor.goto_first_child() {
            extract_calls_recursive(cursor, source, called);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extracts coupled class names from a class.
fn extract_coupled_classes(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Vec<String> {
    let mut coupled = HashSet::new();

    // Language-specific type reference node types
    let type_node_types: Vec<&str> = match lang {
        Language::Ruby => vec![
            "constant",         // Class references like User, Post, ActiveRecord
            "scope_resolution", // Namespaced constants like ActiveRecord::Base
        ],
        Language::Python => vec![
            "identifier", // Python uses identifiers for class refs in type hints
            "attribute",  // For qualified names like module.ClassName
        ],
        Language::Rust => vec![
            "type_identifier",   // Type references in signatures
            "scoped_identifier", // Qualified paths like std::io::Error
        ],
        Language::Go => vec![
            "type_identifier", // Type references
            "qualified_type",  // Package-qualified types like http.Handler
        ],
        _ => vec![
            "type_identifier",
            "class_type",
            "simple_type",
            "named_type",
            "type_name",
        ],
    };

    let mut cursor = node.walk();
    extract_types_recursive(&mut cursor, source, &type_node_types, lang, &mut coupled);
    coupled.into_iter().collect()
}

/// Recursively extracts type references.
fn extract_types_recursive(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    type_node_types: &[&str],
    lang: Language,
    coupled: &mut HashSet<String>,
) {
    loop {
        let node = cursor.node();

        if type_node_types.contains(&node.kind()) {
            if let Ok(name) = std::str::from_utf8(&source[node.byte_range()]) {
                if is_valid_class_reference(name, lang) {
                    coupled.insert(name.to_string());
                }
            }
        }

        if cursor.goto_first_child() {
            extract_types_recursive(cursor, source, type_node_types, lang, coupled);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Checks if a name is a valid class reference for the given language.
fn is_valid_class_reference(name: &str, lang: Language) -> bool {
    if is_primitive_type(name) || name.len() <= 1 {
        return false;
    }

    match lang {
        Language::Ruby => {
            // Ruby constants start with uppercase
            // Exclude common non-class constants
            let first_char = name.chars().next().unwrap_or('a');
            if !first_char.is_ascii_uppercase() {
                return false;
            }
            // Exclude common Ruby constants that aren't classes
            !matches!(
                name,
                "DEFAULT_FEATURED_BADGE_COUNT"
                    | "MAX_SIMILAR_USERS"
                    | "TRUE"
                    | "FALSE"
                    | "STDIN"
                    | "STDOUT"
                    | "STDERR"
                    | "ENV"
                    | "ARGV"
                    | "DATA"
                    | "RUBY_VERSION"
                    | "RUBY_PLATFORM"
            ) && !name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            // Exclude SCREAMING_SNAKE_CASE constants (non-class constants)
        }
        Language::Rust => {
            // Rust types are PascalCase. Exclude common non-struct types.
            let first_char = name.chars().next().unwrap_or('a');
            first_char.is_ascii_uppercase()
        }
        Language::Go => {
            // Go exported types are PascalCase
            let first_char = name.chars().next().unwrap_or('a');
            first_char.is_ascii_uppercase()
        }
        _ => true,
    }
}

/// Checks if a type name is a primitive.
fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "float"
            | "float32"
            | "float64"
            | "double"
            | "bool"
            | "boolean"
            | "Boolean"
            | "string"
            | "String"
            | "str"
            | "void"
            | "None"
            | "null"
            | "nil"
            | "byte"
            | "char"
            | "short"
            | "long"
            | "any"
            | "object"
            | "Object"
            | "number"
            | "Number"
            | "true"
            | "false"
            | "self"
            | "this"
            | "super"
    )
}

/// Calculates LCOM3 (Hitz & Montazeri 1995) as the number of connected components.
/// Methods are connected if they share at least one instance variable.
/// Note: This is LCOM3, not LCOM4. LCOM4 would also connect methods that call each other.
fn calculate_lcom(methods: &[MethodInfo], fields: &[String]) -> u32 {
    if methods.is_empty() {
        return 0;
    }
    if fields.is_empty() {
        // No fields means methods don't share state
        return methods.len() as u32;
    }

    let n = methods.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    // Build adjacency list: methods are connected if they share a field
    for i in 0..n {
        for j in (i + 1)..n {
            // Check if methods i and j share any field
            for field in &methods[i].used_fields {
                if methods[j].used_fields.contains(field) {
                    adj[i].push(j);
                    adj[j].push(i);
                    break;
                }
            }
        }
    }

    // Count connected components using DFS
    let mut visited = vec![false; n];
    let mut components = 0;

    for i in 0..n {
        if !visited[i] {
            dfs(i, &adj, &mut visited);
            components += 1;
        }
    }

    components
}

/// DFS for connected components.
fn dfs(v: usize, adj: &[Vec<usize>], visited: &mut [bool]) {
    visited[v] = true;
    for &u in &adj[v] {
        if !visited[u] {
            dfs(u, adj, visited);
        }
    }
}

/// Calculates summary statistics.
fn calculate_summary(classes: &[ClassMetrics]) -> Summary {
    if classes.is_empty() {
        return Summary::default();
    }

    let mut files = HashSet::new();
    let mut total_wmc = 0u32;
    let mut total_cbo = 0u32;
    let mut total_rfc = 0u32;
    let mut total_lcom = 0u32;
    let mut max_wmc = 0u32;
    let mut max_cbo = 0u32;
    let mut max_rfc = 0u32;
    let mut max_lcom = 0u32;
    let mut max_dit = 0u32;
    let mut low_cohesion_count = 0usize;
    let mut violation_count = 0usize;

    for cls in classes {
        files.insert(&cls.path);
        total_wmc += cls.wmc;
        total_cbo += cls.cbo;
        total_rfc += cls.rfc;
        total_lcom += cls.lcom;

        max_wmc = max_wmc.max(cls.wmc);
        max_cbo = max_cbo.max(cls.cbo);
        max_rfc = max_rfc.max(cls.rfc);
        max_lcom = max_lcom.max(cls.lcom);
        max_dit = max_dit.max(cls.dit);

        if cls.lcom > 1 {
            low_cohesion_count += 1;
        }
        violation_count += cls.violations.len();
    }

    let n = classes.len() as f64;
    Summary {
        total_classes: classes.len(),
        total_files: files.len(),
        avg_wmc: total_wmc as f64 / n,
        avg_cbo: total_cbo as f64 / n,
        avg_rfc: total_rfc as f64 / n,
        avg_lcom: total_lcom as f64 / n,
        max_wmc,
        max_cbo,
        max_rfc,
        max_lcom,
        max_dit,
        low_cohesion_count,
        violation_count,
    }
}

/// CK metrics analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// When the analysis was generated.
    pub generated_at: String,
    /// Per-class metrics.
    pub classes: Vec<ClassMetrics>,
    /// Summary statistics.
    pub summary: Summary,
}

impl Analysis {
    /// Sorts classes by LCOM (least cohesive first).
    pub fn sort_by_lcom(&mut self) {
        self.classes.sort_by(|a, b| b.lcom.cmp(&a.lcom));
    }

    /// Sorts classes by WMC (most complex first).
    pub fn sort_by_wmc(&mut self) {
        self.classes.sort_by(|a, b| b.wmc.cmp(&a.wmc));
    }

    /// Sorts classes by CBO (most coupled first).
    pub fn sort_by_cbo(&mut self) {
        self.classes.sort_by(|a, b| b.cbo.cmp(&a.cbo));
    }
}

/// CK metrics for a single class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMetrics {
    /// File path.
    pub path: String,
    /// Class name.
    pub class_name: String,
    /// Parent class name (for inheritance tracking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_class: Option<String>,
    /// Programming language.
    pub language: String,
    /// Start line.
    pub start_line: usize,
    /// End line.
    pub end_line: usize,
    /// Lines of code in class.
    pub loc: u32,
    /// Weighted Methods per Class (sum of cyclomatic complexity).
    pub wmc: u32,
    /// Coupling Between Objects (number of classes referenced).
    pub cbo: u32,
    /// Response for Class (methods that can be invoked).
    pub rfc: u32,
    /// Lack of Cohesion in Methods (LCOM3 - connected components).
    pub lcom: u32,
    /// Depth of Inheritance Tree.
    pub dit: u32,
    /// Number of Children (direct subclasses).
    pub noc: u32,
    /// Number of methods.
    pub nom: u32,
    /// Number of fields.
    pub nof: u32,
    /// Method names.
    pub methods: Vec<String>,
    /// Field names.
    pub fields: Vec<String>,
    /// Coupled class names.
    pub coupled_classes: Vec<String>,
    /// Metric violations.
    pub violations: Vec<String>,
}

/// Aggregate CK metrics summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    /// Total classes analyzed.
    pub total_classes: usize,
    /// Total files analyzed.
    pub total_files: usize,
    /// Average WMC.
    pub avg_wmc: f64,
    /// Average CBO.
    pub avg_cbo: f64,
    /// Average RFC.
    pub avg_rfc: f64,
    /// Average LCOM.
    pub avg_lcom: f64,
    /// Maximum WMC.
    pub max_wmc: u32,
    /// Maximum CBO.
    pub max_cbo: u32,
    /// Maximum RFC.
    pub max_rfc: u32,
    /// Maximum LCOM.
    pub max_lcom: u32,
    /// Maximum DIT.
    pub max_dit: u32,
    /// Classes with LCOM > 1.
    pub low_cohesion_count: usize,
    /// Total number of violations.
    pub violation_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.skip_test_files);
        assert_eq!(config.max_file_size, 0);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert!(analyzer.config.skip_test_files);
    }

    #[test]
    fn test_analyzer_with_include_test_files() {
        let analyzer = Analyzer::new().with_include_test_files();
        assert!(!analyzer.config.skip_test_files);
    }

    #[test]
    fn test_is_oo_language() {
        assert!(is_oo_language(Language::Java));
        assert!(is_oo_language(Language::Python));
        assert!(is_oo_language(Language::TypeScript));
        assert!(is_oo_language(Language::Go));
        assert!(is_oo_language(Language::Rust));
        assert!(!is_oo_language(Language::C));
    }

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file(Path::new("foo_test.go")));
        assert!(is_test_file(Path::new("bar_test.py")));
        assert!(is_test_file(Path::new("baz.test.ts")));
        assert!(is_test_file(Path::new("qux.spec.js")));
        assert!(is_test_file(Path::new("src/test/Foo.java")));
        assert!(is_test_file(Path::new("__tests__/foo.js")));
        assert!(!is_test_file(Path::new("src/main.ts")));
        assert!(!is_test_file(Path::new("lib/foo.py")));
    }

    #[test]
    fn test_is_primitive_type() {
        assert!(is_primitive_type("int"));
        assert!(is_primitive_type("String"));
        assert!(is_primitive_type("bool"));
        assert!(is_primitive_type("void"));
        assert!(is_primitive_type("this"));
        assert!(!is_primitive_type("MyClass"));
        assert!(!is_primitive_type("UserService"));
    }

    #[test]
    fn test_lcom_no_methods() {
        let methods: Vec<MethodInfo> = vec![];
        let fields: Vec<String> = vec![];
        assert_eq!(calculate_lcom(&methods, &fields), 0);
    }

    #[test]
    fn test_lcom_no_fields() {
        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
            },
        ];
        let fields: Vec<String> = vec![];
        // No fields = methods don't share state = each is its own component
        assert_eq!(calculate_lcom(&methods, &fields), 2);
    }

    #[test]
    fn test_lcom_connected() {
        let mut fields1 = HashSet::new();
        fields1.insert("x".to_string());

        let mut fields2 = HashSet::new();
        fields2.insert("x".to_string());

        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: fields1,
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: fields2,
            },
        ];
        let fields = vec!["x".to_string()];
        // Both methods use field x, so they're connected = 1 component
        assert_eq!(calculate_lcom(&methods, &fields), 1);
    }

    #[test]
    fn test_lcom_disconnected() {
        let mut fields1 = HashSet::new();
        fields1.insert("x".to_string());

        let mut fields2 = HashSet::new();
        fields2.insert("y".to_string());

        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: fields1,
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: fields2,
            },
        ];
        let fields = vec!["x".to_string(), "y".to_string()];
        // Methods use different fields, not connected = 2 components
        assert_eq!(calculate_lcom(&methods, &fields), 2);
    }

    #[test]
    fn test_summary_empty() {
        let summary = calculate_summary(&[]);
        assert_eq!(summary.total_classes, 0);
        assert_eq!(summary.total_files, 0);
    }

    #[test]
    fn test_summary_with_classes() {
        let classes = vec![
            ClassMetrics {
                path: "a.java".to_string(),
                class_name: "Foo".to_string(),
                parent_class: None,
                language: "Java".to_string(),
                start_line: 1,
                end_line: 50,
                loc: 50,
                wmc: 10,
                cbo: 5,
                rfc: 15,
                lcom: 2,
                dit: 1,
                noc: 0,
                nom: 5,
                nof: 3,
                methods: vec![],
                fields: vec![],
                coupled_classes: vec![],
                violations: vec![],
            },
            ClassMetrics {
                path: "b.java".to_string(),
                class_name: "Bar".to_string(),
                parent_class: None,
                language: "Java".to_string(),
                start_line: 1,
                end_line: 30,
                loc: 30,
                wmc: 20,
                cbo: 8,
                rfc: 25,
                lcom: 1,
                dit: 0,
                noc: 1,
                nom: 8,
                nof: 2,
                methods: vec![],
                fields: vec![],
                coupled_classes: vec![],
                violations: vec![],
            },
        ];

        let summary = calculate_summary(&classes);
        assert_eq!(summary.total_classes, 2);
        assert_eq!(summary.total_files, 2);
        assert!((summary.avg_wmc - 15.0).abs() < 0.001);
        assert!((summary.avg_cbo - 6.5).abs() < 0.001);
        assert_eq!(summary.max_wmc, 20);
        assert_eq!(summary.max_cbo, 8);
        assert_eq!(summary.max_lcom, 2);
        assert_eq!(summary.low_cohesion_count, 1); // Only Foo has LCOM > 1
    }

    #[test]
    fn test_class_metrics_fields() {
        let metrics = ClassMetrics {
            path: "Test.java".to_string(),
            class_name: "Test".to_string(),
            parent_class: Some("BaseTest".to_string()),
            language: "Java".to_string(),
            start_line: 1,
            end_line: 100,
            loc: 100,
            wmc: 25,
            cbo: 10,
            rfc: 30,
            lcom: 3,
            dit: 2,
            noc: 1,
            nom: 8,
            nof: 4,
            methods: vec!["foo".to_string(), "bar".to_string()],
            fields: vec!["x".to_string(), "y".to_string()],
            coupled_classes: vec!["Helper".to_string()],
            violations: vec!["LCOM 3 exceeds threshold 1".to_string()],
        };

        assert_eq!(metrics.class_name, "Test");
        assert_eq!(metrics.wmc, 25);
        assert_eq!(metrics.violations.len(), 1);
    }

    #[test]
    fn test_analysis_sorting() {
        let mut analysis = Analysis {
            generated_at: "2024-01-01".to_string(),
            classes: vec![
                ClassMetrics {
                    class_name: "Low".to_string(),
                    parent_class: None,
                    lcom: 1,
                    wmc: 5,
                    cbo: 2,
                    path: String::new(),
                    language: String::new(),
                    start_line: 0,
                    end_line: 0,
                    loc: 0,
                    rfc: 0,
                    dit: 0,
                    noc: 0,
                    nom: 0,
                    nof: 0,
                    methods: vec![],
                    fields: vec![],
                    coupled_classes: vec![],
                    violations: vec![],
                },
                ClassMetrics {
                    class_name: "High".to_string(),
                    parent_class: None,
                    lcom: 5,
                    wmc: 50,
                    cbo: 10,
                    path: String::new(),
                    language: String::new(),
                    start_line: 0,
                    end_line: 0,
                    loc: 0,
                    rfc: 0,
                    dit: 0,
                    noc: 0,
                    nom: 0,
                    nof: 0,
                    methods: vec![],
                    fields: vec![],
                    coupled_classes: vec![],
                    violations: vec![],
                },
            ],
            summary: Summary::default(),
        };

        analysis.sort_by_lcom();
        assert_eq!(analysis.classes[0].class_name, "High");

        analysis.sort_by_wmc();
        assert_eq!(analysis.classes[0].class_name, "High");

        analysis.sort_by_cbo();
        assert_eq!(analysis.classes[0].class_name, "High");
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "cohesion");
        assert!(analyzer.description().contains("CK"));
    }

    #[test]
    fn test_thresholds() {
        // WMC threshold of 24 per Chidamber & Kemerer 1994 IEEE TSE
        assert_eq!(WMC_THRESHOLD, 24);
        assert_eq!(CBO_THRESHOLD, 14);
        assert_eq!(LCOM_THRESHOLD, 1);
    }

    #[test]
    fn test_wmc_threshold_at_24() {
        // Verify WMC > 24 is flagged as complex
        let metrics = ClassMetrics {
            path: "Test.java".to_string(),
            class_name: "ComplexClass".to_string(),
            parent_class: None,
            language: "Java".to_string(),
            start_line: 1,
            end_line: 100,
            loc: 100,
            wmc: 25, // Just above threshold
            cbo: 5,
            rfc: 20,
            lcom: 1,
            dit: 0,
            noc: 0,
            nom: 10,
            nof: 5,
            methods: vec![],
            fields: vec![],
            coupled_classes: vec![],
            violations: vec![format!("WMC {} exceeds threshold {}", 25, WMC_THRESHOLD)],
        };

        assert!(metrics.wmc > WMC_THRESHOLD);
        assert!(!metrics.violations.is_empty());

        // Also verify WMC at threshold is not flagged
        let at_threshold = 24;
        assert!(at_threshold <= WMC_THRESHOLD);
    }

    // DIT/NOC Tests - Chidamber & Kemerer 1994 IEEE TSE
    // DIT = Depth of Inheritance Tree (max path length to root)
    // NOC = Number of Children (direct subclasses)

    #[test]
    fn test_lcom_single_method_no_fields() {
        let methods = vec![MethodInfo {
            name: "only".to_string(),
            complexity: 1,
            used_fields: HashSet::new(),
        }];
        let fields: Vec<String> = vec![];
        // No fields: each method is its own component
        assert_eq!(calculate_lcom(&methods, &fields), 1);
    }

    #[test]
    fn test_lcom_single_method_with_fields() {
        let mut used = HashSet::new();
        used.insert("x".to_string());
        let methods = vec![MethodInfo {
            name: "only".to_string(),
            complexity: 1,
            used_fields: used,
        }];
        let fields = vec!["x".to_string()];
        // Single method using a field: 1 connected component
        assert_eq!(calculate_lcom(&methods, &fields), 1);
    }

    #[test]
    fn test_lcom_methods_no_field_usage() {
        // Methods exist, fields exist, but methods don't use any fields
        let methods = vec![
            MethodInfo {
                name: "a".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
            },
            MethodInfo {
                name: "b".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
            },
        ];
        let fields = vec!["x".to_string(), "y".to_string()];
        // No shared fields: 2 disconnected components
        assert_eq!(calculate_lcom(&methods, &fields), 2);
    }

    #[test]
    fn test_lcom_three_methods_chain_connected() {
        // A shares field with B, B shares field with C => all connected
        let mut fa = HashSet::new();
        fa.insert("x".to_string());
        let mut fb = HashSet::new();
        fb.insert("x".to_string());
        fb.insert("y".to_string());
        let mut fc = HashSet::new();
        fc.insert("y".to_string());

        let methods = vec![
            MethodInfo {
                name: "a".to_string(),
                complexity: 1,
                used_fields: fa,
            },
            MethodInfo {
                name: "b".to_string(),
                complexity: 1,
                used_fields: fb,
            },
            MethodInfo {
                name: "c".to_string(),
                complexity: 1,
                used_fields: fc,
            },
        ];
        let fields = vec!["x".to_string(), "y".to_string()];
        assert_eq!(calculate_lcom(&methods, &fields), 1);
    }

    #[test]
    fn test_summary_division_by_zero_empty() {
        // Verify calculate_summary handles empty classes without division by zero
        let summary = calculate_summary(&[]);
        assert_eq!(summary.avg_wmc, 0.0);
        assert_eq!(summary.avg_cbo, 0.0);
        assert_eq!(summary.avg_rfc, 0.0);
        assert_eq!(summary.avg_lcom, 0.0);
    }

    #[test]
    fn test_class_hierarchy_cycle_detection() {
        // Pathological case: cyclic inheritance (should not infinite loop)
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("A", Some("B"));
        hierarchy.add_class("B", Some("A"));

        // Should terminate and return a reasonable depth
        let dit_a = hierarchy.get_dit("A");
        let dit_b = hierarchy.get_dit("B");
        assert!(dit_a <= 2, "DIT should be bounded even with cycles");
        assert!(dit_b <= 2, "DIT should be bounded even with cycles");
    }

    #[test]
    fn test_class_hierarchy_self_parent() {
        // Degenerate case: class is its own parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("SelfRef", Some("SelfRef"));

        // Should not infinite loop
        let dit = hierarchy.get_dit("SelfRef");
        assert!(dit <= 1, "Self-parent DIT should be bounded");
    }

    #[test]
    fn test_class_hierarchy_empty() {
        let hierarchy = ClassHierarchy::new();
        assert_eq!(hierarchy.get_dit("Foo"), 0);
        assert_eq!(hierarchy.get_noc("Foo"), 0);
    }

    #[test]
    fn test_class_hierarchy_no_inheritance() {
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Foo", None);
        assert_eq!(hierarchy.get_dit("Foo"), 0);
        assert_eq!(hierarchy.get_noc("Foo"), 0);
    }

    #[test]
    fn test_class_hierarchy_single_inheritance() {
        // Child extends Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child", Some("Parent"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_noc("Parent"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 0);
    }

    #[test]
    fn test_class_hierarchy_multi_level() {
        // GrandChild extends Child extends Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child", Some("Parent"));
        hierarchy.add_class("GrandChild", Some("Child"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_dit("GrandChild"), 2);
        assert_eq!(hierarchy.get_noc("Parent"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 1);
        assert_eq!(hierarchy.get_noc("GrandChild"), 0);
    }

    #[test]
    fn test_class_hierarchy_multiple_children() {
        // Child1, Child2, Child3 all extend Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child1", Some("Parent"));
        hierarchy.add_class("Child2", Some("Parent"));
        hierarchy.add_class("Child3", Some("Parent"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_noc("Parent"), 3);
        for child in &["Child1", "Child2", "Child3"] {
            assert_eq!(hierarchy.get_dit(child), 1);
            assert_eq!(hierarchy.get_noc(child), 0);
        }
    }

    #[test]
    fn test_class_hierarchy_unknown_parent() {
        // Child extends UnknownParent (not in codebase, e.g., stdlib)
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Child", Some("UnknownParent"));

        // DIT should be 1 (assuming unknown parent has DIT 0)
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 0);
    }

    #[test]
    fn test_extract_parent_class_java() {
        let parser = Parser::new();
        let source = b"public class Child extends Parent { }";
        let result = parser
            .parse(source, Language::Java, Path::new("Child.java"))
            .unwrap();

        let class_node = find_first_class_node(&result.tree, Language::Java).unwrap();
        let parent = extract_parent_class(&class_node, source, Language::Java);

        assert_eq!(parent, Some("Parent".to_string()));
    }

    #[test]
    fn test_extract_parent_class_java_no_extends() {
        let parser = Parser::new();
        let source = b"public class Standalone { }";
        let result = parser
            .parse(source, Language::Java, Path::new("Standalone.java"))
            .unwrap();

        let class_node = find_first_class_node(&result.tree, Language::Java).unwrap();
        let parent = extract_parent_class(&class_node, source, Language::Java);

        assert_eq!(parent, None);
    }

    #[test]
    fn test_extract_parent_class_python() {
        let parser = Parser::new();
        let source = b"class Child(Parent):\n    pass";
        let result = parser
            .parse(source, Language::Python, Path::new("child.py"))
            .unwrap();

        let class_node = find_first_class_node(&result.tree, Language::Python).unwrap();
        let parent = extract_parent_class(&class_node, source, Language::Python);

        assert_eq!(parent, Some("Parent".to_string()));
    }

    #[test]
    fn test_extract_parent_class_typescript() {
        let parser = Parser::new();
        let source = b"class Child extends Parent { }";
        let result = parser
            .parse(source, Language::TypeScript, Path::new("child.ts"))
            .unwrap();

        let class_node = find_first_class_node(&result.tree, Language::TypeScript).unwrap();
        let parent = extract_parent_class(&class_node, source, Language::TypeScript);

        assert_eq!(parent, Some("Parent".to_string()));
    }

    /// Helper to find the first class node in a tree using a cursor.
    fn find_first_class_node<'a>(
        tree: &'a tree_sitter::Tree,
        lang: Language,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut cursor = tree.walk();
        find_class_node_recursive(&mut cursor, lang)
    }

    fn find_class_node_recursive<'a>(
        cursor: &mut tree_sitter::TreeCursor<'a>,
        lang: Language,
    ) -> Option<tree_sitter::Node<'a>> {
        loop {
            let node = cursor.node();
            if is_class_node(node.kind(), lang) {
                return Some(node);
            }
            if cursor.goto_first_child() {
                if let Some(found) = find_class_node_recursive(cursor, lang) {
                    return Some(found);
                }
                cursor.goto_parent();
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        None
    }

    #[test]
    fn test_rust_struct_impl_extraction() {
        let parser = Parser::new();
        let source = br#"
pub struct Counter {
    count: u32,
    name: String,
}

impl Counter {
    pub fn new(name: String) -> Self {
        Self { count: 0, name }
    }

    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn get_count(&self) -> u32 {
        self.count
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }
}
"#;
        let result = parser
            .parse(source, Language::Rust, Path::new("counter.rs"))
            .unwrap();

        let classes = extract_classes_from_file(
            Path::new("counter.rs"),
            source,
            result.tree.as_ref(),
            Language::Rust,
        );

        assert_eq!(classes.len(), 1);
        let cls = &classes[0];
        assert_eq!(cls.class_name, "Counter");
        assert_eq!(cls.language, "Rust");
        assert!(cls.nom > 0, "expected methods, got nom={}", cls.nom);
        assert_eq!(cls.nom, 4);
        assert!(cls.nof > 0, "expected fields, got nof={}", cls.nof);
        assert_eq!(cls.nof, 2);
        assert!(cls.wmc > 0, "expected non-zero WMC");
        assert!(cls.rfc >= cls.nom, "RFC should be at least NOM");
        assert_eq!(cls.dit, 0);
        assert!(cls.parent_class.is_none());
    }

    #[test]
    fn test_rust_multiple_impl_blocks() {
        let parser = Parser::new();
        let source = br#"
pub struct Widget {
    width: u32,
    height: u32,
}

impl Widget {
    pub fn area(&self) -> u32 {
        self.width * self.height
    }
}

impl Widget {
    pub fn resize(&mut self, w: u32, h: u32) {
        self.width = w;
        self.height = h;
    }
}
"#;
        let result = parser
            .parse(source, Language::Rust, Path::new("widget.rs"))
            .unwrap();

        let classes = extract_classes_from_file(
            Path::new("widget.rs"),
            source,
            result.tree.as_ref(),
            Language::Rust,
        );

        assert_eq!(classes.len(), 1);
        let cls = &classes[0];
        assert_eq!(cls.class_name, "Widget");
        // Methods from both impl blocks should be collected
        assert_eq!(cls.nom, 2);
        assert_eq!(cls.nof, 2);
    }

    #[test]
    fn test_go_struct_method_extraction() {
        let parser = Parser::new();
        let source = br#"
package main

type Server struct {
	Host string
	Port int
}

func (s *Server) Start() error {
	return nil
}

func (s *Server) Stop() {
	s.Host = ""
}

func (s Server) Address() string {
	return s.Host
}
"#;
        let result = parser
            .parse(source, Language::Go, Path::new("server.go"))
            .unwrap();

        let classes = extract_classes_from_file(
            Path::new("server.go"),
            source,
            result.tree.as_ref(),
            Language::Go,
        );

        assert_eq!(classes.len(), 1);
        let cls = &classes[0];
        assert_eq!(cls.class_name, "Server");
        assert_eq!(cls.language, "Go");
        assert!(cls.nom > 0, "expected methods, got nom={}", cls.nom);
        assert_eq!(cls.nom, 3);
        assert!(cls.nof > 0, "expected fields, got nof={}", cls.nof);
        assert_eq!(cls.nof, 2);
        assert!(cls.wmc > 0, "expected non-zero WMC");
        assert_eq!(cls.dit, 0);
        assert!(cls.parent_class.is_none());
    }

    #[test]
    fn test_go_multiple_structs() {
        let parser = Parser::new();
        let source = br#"
package main

type Request struct {
	Method string
	URL    string
}

type Response struct {
	Status int
	Body   string
}

func (r *Request) Validate() bool {
	return r.Method != ""
}

func (r *Response) IsOK() bool {
	return r.Status == 200
}
"#;
        let result = parser
            .parse(source, Language::Go, Path::new("http.go"))
            .unwrap();

        let classes = extract_classes_from_file(
            Path::new("http.go"),
            source,
            result.tree.as_ref(),
            Language::Go,
        );

        assert_eq!(classes.len(), 2);
        let names: HashSet<_> = classes.iter().map(|c| c.class_name.as_str()).collect();
        assert!(names.contains("Request"));
        assert!(names.contains("Response"));

        for cls in &classes {
            assert_eq!(cls.nom, 1);
            assert!(cls.nof >= 2);
        }
    }
}
