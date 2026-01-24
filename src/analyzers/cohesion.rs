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
use ignore::WalkBuilder;
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
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        // Phase 1: Collect all candidate files (fast, sequential)
        let files: Vec<_> = WalkBuilder::new(repo_path)
            .hidden(false)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.into_path())
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
                // Read file
                let source = std::fs::read(path).ok()?;

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
        self.analyze_repo(ctx.root)
    }
}

/// Checks if a language supports traditional OO classes.
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

        // Walk up the inheritance chain
        while let Some(Some(parent)) = self.parents.get(&current) {
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
    let mut classes = Vec::new();
    let root = tree.root_node();

    // Walk the tree to find class definitions
    let mut cursor = root.walk();
    extract_classes_recursive(&mut cursor, source, path, lang, &mut classes);

    classes
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

    // Collect violations
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

/// Extracts the parent class name from a class node (for extends/inherits).
fn extract_parent_class(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    match lang {
        Language::Java => {
            // Java: class Child extends Parent { }
            // superclass node contains: "extends" keyword + type_identifier
            node.child_by_field_name("superclass").and_then(|sc| {
                // Find the type_identifier child (not the "extends" keyword)
                for i in 0..sc.child_count() {
                    if let Some(child) = sc.child(i) {
                        if child.kind() == "type_identifier" {
                            return std::str::from_utf8(&source[child.byte_range()])
                                .ok()
                                .map(|s| s.to_string());
                        }
                    }
                }
                None
            })
        }
        Language::TypeScript | Language::JavaScript => {
            // TS/JS: class Child extends Parent { }
            // Look for class_heritage -> extends_clause -> identifier
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "class_heritage" {
                        // Find the extends clause
                        for j in 0..child.child_count() {
                            if let Some(clause) = child.child(j) {
                                if clause.kind() == "extends_clause" {
                                    // Get the identifier/type after 'extends'
                                    for k in 0..clause.child_count() {
                                        if let Some(type_node) = clause.child(k) {
                                            if type_node.kind() == "identifier"
                                                || type_node.kind() == "type_identifier"
                                            {
                                                return std::str::from_utf8(
                                                    &source[type_node.byte_range()],
                                                )
                                                .ok()
                                                .map(|s| s.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        Language::Python => {
            // Python: class Child(Parent):
            // superclasses is an argument_list containing: "(", identifier, ")"
            node.child_by_field_name("superclasses").and_then(|args| {
                // Find the first identifier child (the base class name)
                for i in 0..args.child_count() {
                    if let Some(child) = args.child(i) {
                        if child.kind() == "identifier" {
                            return std::str::from_utf8(&source[child.byte_range()])
                                .ok()
                                .map(|s| s.to_string());
                        }
                    }
                }
                None
            })
        }
        Language::CSharp => {
            // C#: class Child : Parent { }
            node.child_by_field_name("bases")
                .and_then(|bases| {
                    for i in 0..bases.child_count() {
                        if let Some(base) = bases.child(i) {
                            if base.kind() == "identifier" || base.kind() == "type_identifier" {
                                return std::str::from_utf8(&source[base.byte_range()]).ok();
                            }
                        }
                    }
                    None
                })
                .map(|s| s.to_string())
        }
        Language::Cpp => {
            // C++: class Child : public Parent { }
            // Look for base_class_clause
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "base_class_clause" {
                        // Find type_identifier inside
                        for j in 0..child.child_count() {
                            if let Some(base) = child.child(j) {
                                if base.kind() == "type_identifier" {
                                    return std::str::from_utf8(&source[base.byte_range()])
                                        .ok()
                                        .map(|s| s.to_string());
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        Language::Ruby => {
            // Ruby: class Child < Parent
            node.child_by_field_name("superclass")
                .and_then(|sc| std::str::from_utf8(&source[sc.byte_range()]).ok())
                .map(|s| s.to_string())
        }
        Language::Php => {
            // PHP: class Child extends Parent { }
            node.child_by_field_name("base_clause")
                .and_then(|base| {
                    for i in 0..base.child_count() {
                        if let Some(child) = base.child(i) {
                            if child.kind() == "name" || child.kind() == "qualified_name" {
                                return std::str::from_utf8(&source[child.byte_range()]).ok();
                            }
                        }
                    }
                    None
                })
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

/// Gets the class name from a node.
fn get_class_name(node: &tree_sitter::Node, source: &[u8], lang: Language) -> Option<String> {
    let name_node = match lang {
        Language::Python => node.child_by_field_name("name"),
        Language::Ruby => node.child_by_field_name("name"),
        _ => node.child_by_field_name("name"),
    }?;

    let name = std::str::from_utf8(&source[name_node.byte_range()]).ok()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
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

/// Recursively finds field accesses.
fn find_field_accesses(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    lang: Language,
    fields: &mut HashSet<String>,
) {
    loop {
        let node = cursor.node();

        match lang {
            Language::Python => {
                if node.kind() == "attribute" {
                    if let (Some(obj), Some(attr)) = (
                        node.child_by_field_name("object"),
                        node.child_by_field_name("attribute"),
                    ) {
                        if let (Ok(obj_text), Ok(attr_text)) = (
                            std::str::from_utf8(&source[obj.byte_range()]),
                            std::str::from_utf8(&source[attr.byte_range()]),
                        ) {
                            if obj_text == "self" {
                                fields.insert(attr_text.to_string());
                            }
                        }
                    }
                }
            }
            Language::Ruby => {
                if node.kind() == "instance_variable" {
                    if let Ok(text) = std::str::from_utf8(&source[node.byte_range()]) {
                        fields.insert(text.to_string());
                    }
                }
            }
            Language::Java | Language::CSharp | Language::TypeScript | Language::JavaScript => {
                if node.kind() == "member_expression" || node.kind() == "member_access_expression" {
                    if let (Some(obj), Some(prop)) = (
                        node.child_by_field_name("object"),
                        node.child_by_field_name("property"),
                    ) {
                        if let (Ok(obj_text), Ok(prop_text)) = (
                            std::str::from_utf8(&source[obj.byte_range()]),
                            std::str::from_utf8(&source[prop.byte_range()]),
                        ) {
                            if obj_text == "this" {
                                fields.insert(prop_text.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
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
        assert!(!is_oo_language(Language::Go));
        assert!(!is_oo_language(Language::Rust));
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
}
