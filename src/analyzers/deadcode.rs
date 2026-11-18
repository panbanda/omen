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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::index::extract::FileSymbols;
use crate::index::imports::RustModules;
use crate::index::model::{
    Exported, Receiver, Reference, ResolutionStatus, SharedStr, Symbol, SymbolKind, TestRole,
};
use crate::index::security::entry::{
    entry_points_of_file, find_entry_points_with, EntryPoint, OnDemandTrees,
};
use crate::index::test_code::rust_attribute_text;
use crate::parser::{self, Parser};

/// The result for one file: its definitions and uses, its entry points, and its syntax
/// tree when the analysis needs the tree later.
type StreamedFile = (FileDeadCode, Vec<EntryPoint>, Option<parser::ParseResult>);

/// Dead code analyzer.
pub struct Analyzer {
    parser: Parser,
    confidence_threshold: f64,
    cargo_check: bool,
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
            cargo_check: false,
        }
    }

    pub fn with_confidence(mut self, threshold: f64) -> Self {
        self.confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn with_cargo_check(mut self, enabled: bool) -> Self {
        self.cargo_check = enabled;
        self
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
        let cargo_items = if self.cargo_check && is_rust_project {
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

        // Phase 1: Collect the definitions and the references from the shared
        // index, and the context that only the syntax tree gives (attributes,
        // decorators, visibility modifiers, and uses as a value). Cargo results
        // supplement this with compiler-level dead code warnings when available.
        let index = ctx.code_index()?;
        // An entry point is alive, and its callees are reachable. The entry points are those
        // of the security analysis, with the rows of the configuration.
        let entry_rules = crate::analyzers::security::options_of(&ctx.config.security)?.entry_rules;
        let parse_file = |position: usize| -> Result<Option<parser::ParseResult>> {
            let facts = &index.files[position];
            let path = Path::new(&facts.file);
            crate::index::headers::parse_unchanged(
                facts,
                || ctx.read_file(path),
                |content| self.parser.parse(content, facts.language, path).ok(),
            )
        };
        // Each thread parses a file, uses the tree, and drops it. Only the trees of Rust
        // files stay, because the resolution of paths in Rust needs them.
        let loader = OnDemandTrees::new(&parse_file);
        let streamed: Vec<Option<StreamedFile>> = (0..index.files.len())
            .into_par_iter()
            .map(|position| {
                let facts = &index.files[position];
                let Some(result) = parse_file(position)? else {
                    return Ok(None);
                };
                let data = collect_file_data(&result, facts);
                let entries =
                    entry_points_of_file(position, &result, &index.files, &loader, &entry_rules);
                let tree = (facts.language == Language::Rust).then_some(result);
                Ok(Some((data, entries, tree)))
            })
            .collect::<Result<_>>()?;
        loader.finish()?;
        let mut rust_trees: HashMap<String, parser::ParseResult> = HashMap::new();
        let mut file_results: Vec<FileDeadCode> = Vec::new();
        let entry_keys: HashSet<SharedStr> = if streamed.iter().all(Option::is_some) {
            let mut keys = HashSet::new();
            for (facts, item) in index.files.iter().zip(streamed.into_iter().flatten()) {
                let (data, entries, tree) = item;
                file_results.push(data);
                keys.extend(entries.into_iter().filter_map(|entry| entry.symbol));
                if let Some(tree) = tree {
                    rust_trees.insert(facts.file.clone(), tree);
                }
            }
            keys.into_iter().map(SharedStr::from).collect()
        } else {
            // A file without a tree changes the set of files for the entry points. This
            // path keeps all trees, as the analysis of one set of trees needs.
            drop(streamed);
            let parsed: Vec<Option<(parser::ParseResult, FileDeadCode)>> = index
                .files
                .par_iter()
                .enumerate()
                .map(|(position, facts)| {
                    let Some(result) = parse_file(position)? else {
                        return Ok(None);
                    };
                    let data = collect_file_data(&result, facts);
                    Ok(Some((result, data)))
                })
                .collect::<Result<_>>()?;
            let mut trees: Vec<parser::ParseResult> = Vec::new();
            let mut parsed_facts: Vec<FileSymbols> = Vec::new();
            for (facts, item) in index.files.iter().zip(parsed) {
                if let Some((tree, data)) = item {
                    parsed_facts.push(facts.clone());
                    trees.push(tree);
                    file_results.push(data);
                }
            }
            let keys = find_entry_points_with(&trees, &parsed_facts, &entry_rules)
                .into_iter()
                .filter_map(|entry| entry.symbol.map(SharedStr::from))
                .collect();
            for (facts, tree) in parsed_facts.iter().zip(trees) {
                rust_trees.insert(facts.file.clone(), tree);
            }
            keys
        };

        // Phase 2: Build the global tables. A definition is known by the key
        // of the shared index, which is unique for each definition.
        let mut all_definitions: HashMap<SharedStr, Definition> = HashMap::new();
        let mut all_usages: HashSet<String> = HashSet::new();
        let mut value_references: Vec<CallReference> = Vec::new();
        let mut path_uses: Vec<PathUse> = Vec::new();
        let mut keys_by_name: HashMap<String, Vec<SharedStr>> = HashMap::new();

        for fdc in file_results {
            for def in fdc.definitions {
                keys_by_name
                    .entry(def.name.clone())
                    .or_default()
                    .push(def.key.clone());
                all_definitions.insert(def.key.clone(), def);
            }
            all_usages.extend(fdc.usages);
            value_references.extend(fdc.calls);
            path_uses.extend(fdc.path_uses);
        }
        for keys in keys_by_name.values_mut() {
            keys.sort();
        }

        // Phase 3: Build the edges for reachability. An edge goes from the
        // definition that holds a reference to each definition that the
        // reference can name. A definition is dead only if no reference can
        // name it: the index gives each candidate of an ambiguous call, and a
        // call that the index cannot resolve, or that it marks external, counts
        // for each definition with that name. A wrong external mark must not
        // make a definition dead.
        let mut call_graph: HashMap<SharedStr, Vec<SharedStr>> = HashMap::new();
        let mut roots: Vec<SharedStr> = Vec::new();
        let mut candidate_keys: HashSet<SharedStr> = HashSet::new();
        let mut unresolved_names: HashSet<String> = HashSet::new();
        let manifests = crate::index::imports::Manifests::default();
        let mut rust_ancestors = RustAncestors::new(&index.files);
        // The name that an import alias has in its module: `use a::run as go`
        // gives `run` for `go` in that file.
        let import_aliases: HashMap<(&str, &str), &str> = index
            .files
            .iter()
            .flat_map(|facts| {
                facts.imports.iter().filter_map(|binding| {
                    let local = binding.local_name.as_deref()?;
                    let imported = binding.imported_name.as_deref()?;
                    (local != imported && imported != "*" && imported != "default")
                        .then_some(((facts.file.as_str(), local), imported))
                })
            })
            .collect();
        // A function that a definition uses as a value can run. The use is an
        // edge here in the same way as a call.
        let references = index.files.iter().flat_map(|facts| {
            let calls = facts.references.iter();
            calls.chain(&facts.function_references)
        });
        for reference in references {
            let resolution = &reference.resolution;
            let ancestors = rust_ancestors.of(reference.file.as_str());
            let ancestors: &[String] = &ancestors;
            let weak = matches!(
                resolution.status,
                ResolutionStatus::Unresolved
                    | ResolutionStatus::Ambiguous
                    | ResolutionStatus::External
            );
            let mut targets: Vec<SharedStr> = resolution
                .candidates
                .iter()
                .filter(|key| {
                    !weak
                        || all_definitions
                            .get(key.as_str())
                            .is_none_or(|def| can_be_named_by(def, reference, ancestors))
                })
                .cloned()
                .collect();
            let names_nothing = weak && resolution.candidates.is_empty();
            candidate_keys.extend(targets.iter().cloned());
            if names_nothing {
                unresolved_names.insert(reference.name.clone());
                // A call of an import alias names the definition by its real name.
                let real_name = import_aliases
                    .get(&(reference.file.as_str(), reference.name.as_str()))
                    .copied();
                if let Some(real_name) = real_name {
                    unresolved_names.insert(real_name.to_string());
                }
                let named = std::iter::once(reference.name.as_str()).chain(real_name);
                for keys in named.filter_map(|name| keys_by_name.get(name)) {
                    targets.extend(
                        keys.iter()
                            .filter(|key| {
                                can_be_named_by(&all_definitions[*key], reference, ancestors)
                            })
                            .cloned(),
                    );
                }
            }
            // A Rust call without a receiver and without a path cannot call a
            // method. When the index gives only methods for such a call, a
            // function with the name can be the callee, for example one that a
            // `use` brings from a crate root that the index does not know.
            let is_rust_bare_call = reference.receiver.is_none()
                && reference.qualifier.is_none()
                && reference.file.ends_with(".rs");
            let names_only_methods = !targets.is_empty()
                && targets.iter().all(|key| {
                    all_definitions
                        .get(key.as_str())
                        .is_some_and(|def| !def.is_free_function)
                });
            if is_rust_bare_call && names_only_methods {
                if let Some(keys) = keys_by_name.get(&reference.name) {
                    targets.extend(
                        keys.iter()
                            .filter(|key| {
                                let def = &all_definitions[*key];
                                def.is_free_function && can_be_named_by(def, reference, ancestors)
                            })
                            .cloned(),
                    );
                }
            }
            // Code that is not in a known function runs when the file loads, or
            // when a callback in it runs. Such a reference is a root.
            match reference
                .enclosing
                .as_ref()
                .filter(|key| all_definitions.contains_key(key.as_str()))
            {
                Some(caller) => call_graph
                    .entry(caller.clone())
                    .or_default()
                    .extend(targets),
                None => roots.extend(targets),
            }
        }
        // A Go method that a type needs to implement an interface can run
        // through the interface, also when no call names it.
        let interface_methods = go_interface_methods(&index.files);
        candidate_keys.extend(interface_methods.iter().cloned());
        roots.extend(interface_methods);
        let rust_paths = RustPaths {
            modules: RustModules::new(&index.files, &manifests),
            files: index
                .files
                .iter()
                .map(|facts| (facts.file.as_str(), facts))
                .collect(),
            trees: rust_trees
                .iter()
                .map(|(file, tree)| (file.as_str(), tree))
                .collect(),
            by_name: &keys_by_name,
        };
        let mut unresolved_paths = 0usize;
        for path_use in &path_uses {
            let found = rust_paths.resolve(path_use);
            if !found.is_empty() {
                roots.extend(found);
                continue;
            }
            // The path does not resolve. A function with the last name can be the one.
            unresolved_paths += 1;
            if let Some(keys) = path_use
                .segments
                .last()
                .and_then(|name| keys_by_name.get(name))
            {
                roots.extend(keys.iter().cloned());
            }
        }
        for call in &value_references {
            let callees: Vec<SharedStr> = match keys_by_name.get(&call.callee) {
                Some(keys) => {
                    let local: Vec<SharedStr> = keys
                        .iter()
                        .filter(|key| {
                            all_definitions
                                .get(*key)
                                .is_some_and(|def| def.file == call.file)
                        })
                        .cloned()
                        .collect();
                    // The tokens of a macro do not show the receiver of a
                    // name. Each definition with the name can be the one.
                    if local.is_empty() || call.in_macro_tokens {
                        keys.clone()
                    } else {
                        local
                    }
                }
                None => Vec::new(),
            };
            if call.from_module_scope {
                roots.extend(callees);
                continue;
            }
            let callers: Vec<&SharedStr> = keys_by_name
                .get(&call.caller)
                .into_iter()
                .flatten()
                .filter(|key| {
                    all_definitions
                        .get(*key)
                        .is_some_and(|def| def.file == call.file)
                })
                .collect();
            for caller in callers {
                call_graph
                    .entry(caller.clone())
                    .or_default()
                    .extend(callees.iter().cloned());
            }
        }

        // Phase 4: Mark reachable from entry points
        let mut reachable: HashSet<SharedStr> = HashSet::new();
        let mut queue: Vec<SharedStr> = Vec::new();

        roots.extend(entry_keys.iter().cloned());
        for root in roots {
            if all_definitions.contains_key(&root) && reachable.insert(root.clone()) {
                queue.push(root);
            }
        }

        for (key, def) in &all_definitions {
            if is_entry_point(&def.name, def) && reachable.insert(key.clone()) {
                queue.push(key.clone());
            }
        }

        while let Some(current) = queue.pop() {
            if let Some(callees) = call_graph.get(&current) {
                for callee in callees {
                    if all_definitions.contains_key(callee) && reachable.insert(callee.clone()) {
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
        let test_keys: HashSet<(String, u32)> = all_definitions
            .values()
            .filter(|def| def.test_role != TestRole::None)
            .map(|def| (def.file.clone(), def.line))
            .collect();
        for cargo_item in cargo_items {
            cargo_reported.insert((cargo_item.file.clone(), cargo_item.line));
            let is_test = test_keys.contains(&(cargo_item.file.clone(), cargo_item.line));
            let item = DeadCodeItem {
                name: cargo_item.name,
                kind: cargo_item.kind.clone(),
                file: cargo_item.file,
                line: cargo_item.line,
                end_line: cargo_item.end_line,
                visibility: "unknown".to_string(),
                confidence: 1.0, // Cargo/rustc is authoritative
                reason: cargo_item.message,
                is_test,
            };
            *by_kind.entry(cargo_item.kind).or_insert(0) += 1;
            items.push(item);
        }

        // Add tree-sitter detected dead code, skipping items already reported by cargo
        for (key, def) in &all_definitions {
            // Skip generated files -- they participate in the call graph for
            // reachability but their definitions should not be reported.
            if is_generated_file(&def.file) {
                continue;
            }

            let simple_name = def.name.as_str();

            // Skip entry points
            if is_entry_point(simple_name, def) || entry_keys.contains(key) {
                continue;
            }

            // Skip items already reported by cargo (higher confidence)
            if cargo_reported.contains(&(def.file.clone(), def.line)) {
                continue;
            }

            // Unreachable from the entry points, or named by no reference at all.
            let is_unreachable = !reachable.contains(key);
            let is_unused = !candidate_keys.contains(key)
                && !unresolved_names.contains(simple_name)
                && !all_usages.contains(simple_name);

            if is_unreachable || is_unused {
                let confidence = calculate_confidence(def, is_unreachable, is_unused);

                // The sum of the binary values has noise: 0.9 + 0.05 is not 0.95.
                let confidence = (confidence * 100.0).round() / 100.0;
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
                        is_test: def.test_role != TestRole::None,
                    };

                    *by_kind.entry(def.kind.clone()).or_insert(0) += 1;
                    items.push(item);
                }
            }
        }

        let total_items = items.len();
        // The item with the highest confidence is first, thus the first page
        // of the list has the items that are most probably dead. The place
        // breaks a tie: the definitions come from a map, and its sequence is
        // different in each run.
        items.sort_by(|a, b| {
            b.confidence.total_cmp(&a.confidence).then_with(|| {
                (a.file.as_str(), a.line, a.name.as_str()).cmp(&(
                    b.file.as_str(),
                    b.line,
                    b.name.as_str(),
                ))
            })
        });
        let analysis = Analysis {
            items,
            summary: AnalysisSummary {
                total_items,
                by_kind,
                total_definitions: all_definitions.len(),
                reachable_count: reachable.len(),
                unresolved_paths,
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
///
/// `facts` are the facts of the shared index for the same bytes. They give the
/// definitions. The syntax tree gives the context that the index does not hold.
fn collect_file_data(result: &parser::ParseResult, facts: &FileSymbols) -> FileDeadCode {
    let path_str = facts.file.clone();
    let mut fdc = FileDeadCode {
        path: path_str.clone(),
        definitions: Vec::new(),
        usages: HashSet::new(),
        calls: Vec::new(),
        path_uses: Vec::new(),
    };

    // The index holds only definitions that have a name. An anonymous callback
    // would never match a usage and would be reported as dead with high confidence.
    // A declaration without a body, as a trait method or a prototype does, has no
    // code to remove, and a call can reach it through a different definition.
    let is_stub_file = facts.file.ends_with(".pyi");
    let functions: Vec<&Symbol> = facts
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.is_callable()
                && symbol.kind != SymbolKind::Constructor
                && (symbol.body_range.is_some() || facts.language == Language::Ruby)
                && !is_member_of_an_object(result, symbol)
                // A stub file declares the functions of a different file. It has no code to remove.
                && !is_stub_file
        })
        .collect();

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
        let mut exported =
            func.exported == Exported::Exported || is_exported(&func.name, result.language);
        let info = function_info.get(&func.name_range.start_byte);
        let mut attributes = info.map(|i| i.attributes.clone()).unwrap_or_default();
        let is_trait_impl = info.map(|i| i.is_trait_impl).unwrap_or(false);
        // The interpreter calls a method such as `__eq__`, and the `enum`
        // module calls a method such as `_missing_`. No call site names them.
        let is_python_special_method = result.language == Language::Python
            && func.name.len() > 2
            && func.name.starts_with('_')
            && func.name.ends_with('_')
            && (func.name.starts_with("__") == func.name.ends_with("__"))
            && (func.name.starts_with("__") || func.kind == SymbolKind::Method);
        let runtime_entry = is_python_special_method || info.is_some_and(|i| i.runtime_entry);

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

        fdc.definitions.push(Definition {
            key: func.key.clone(),
            nameable_from_other_files: is_nameable_from_other_files(
                func,
                result.language,
                exported,
                &visibility,
            ),
            is_free_function: func.container.is_none() && func.kind == SymbolKind::Function,
            name: func.name.clone(),
            kind: "function".to_string(),
            file: fdc.path.clone(),
            line: func.range.start_line,
            end_line: func.range.end_line,
            visibility,
            exported,
            test_role: func.test_role,
            runtime_entry,
            attributes,
            is_trait_impl,
        });
    }

    // A file can define one name more than once, as `export const f = () => {}`
    // and an object property `f: () => {}` do. Losing the exported one would
    // report a live export as dead, so the other definitions of the name share
    // its export.
    let exported_names: HashSet<String> = fdc
        .definitions
        .iter()
        .filter(|def| def.exported)
        .map(|def| def.name.clone())
        .collect();
    for def in &mut fdc.definitions {
        if exported_names.contains(&def.name) {
            def.exported = true;
        }
    }

    // Extract usages and the uses of a function as a value by walking the AST
    collect_usages_and_calls(result, &mut fdc);

    fdc
}

/// Info about a Rust function extracted from the AST.
struct RustFunctionInfo {
    /// The path of each attribute of the function, for example `tokio::main`.
    attributes: Vec<String>,
    is_trait_impl: bool,
    /// See `Definition::runtime_entry`.
    runtime_entry: bool,
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

/// The last segment of the path of an attribute that makes the loader, the
/// linker, the compiler, or a framework call the function.
const RUST_RUNTIME_ATTRIBUTES: [&str; 12] = [
    "ctor",
    "dtor",
    "no_mangle",
    "export_name",
    "proc_macro",
    "proc_macro_derive",
    "proc_macro_attribute",
    "panic_handler",
    "wasm_bindgen",
    "pyfunction",
    "napi",
    "main",
];

/// The attribute `allow` or `expect` names the lint `dead_code` or its group `unused`.
fn allows_dead_code(path: &str, arguments: &str) -> bool {
    matches!(path, "allow" | "expect")
        && arguments
            .split(|character: char| !character.is_alphanumeric() && character != '_')
            .any(|word| word == "dead_code" || word == "unused")
}

/// The path and the arguments of each outer attribute of an item. A comment
/// between two attributes, or between an attribute and the item, does not end the list.
fn preceding_attributes<'a>(
    node: &tree_sitter::Node<'_>,
    source: &'a [u8],
) -> Vec<(&'a str, &'a str)> {
    let mut attributes = Vec::new();
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" => attributes.extend(rust_attribute_text(sibling, source)),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        previous = sibling.prev_sibling();
    }
    attributes
}

/// An inner attribute of the block, as `#![allow(dead_code)]` is, permits dead code.
fn inner_attributes_allow_dead_code(block: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = block.walk();
    let found = block
        .children(&mut cursor)
        .filter(|child| child.kind() == "inner_attribute_item")
        .filter_map(|item| rust_attribute_text(item, source))
        .any(|(path, arguments)| allows_dead_code(path, arguments));
    found
}

/// The function has a foreign ABI, as `extern "C" fn` has. Code that is not Rust can call it.
fn has_foreign_abi(function: &tree_sitter::Node<'_>) -> bool {
    let mut cursor = function.walk();
    let found = function
        .children(&mut cursor)
        .filter(|child| child.kind() == "function_modifiers")
        .any(|modifiers| {
            let mut cursor = modifiers.walk();
            let found = modifiers
                .children(&mut cursor)
                .any(|modifier| modifier.kind() == "extern_modifier");
            found
        });
    found
}

/// The attributes and the context of each Rust function, by the start byte of its name.
fn extract_rust_function_attributes(
    result: &parser::ParseResult,
) -> HashMap<usize, RustFunctionInfo> {
    let mut info: HashMap<usize, RustFunctionInfo> = HashMap::new();
    let source: &[u8] = &result.source;
    // Each entry is a node, the trait flag of its block, and the lint flag of its scope.
    let mut stack = vec![(result.root_node(), false, false)];
    while let Some((node, in_trait_impl, allows_dead)) = stack.pop() {
        let mut in_trait_impl = in_trait_impl;
        let mut allows_dead = allows_dead;
        match node.kind() {
            "impl_item" => in_trait_impl = is_trait_impl_block(&node),
            "source_file" | "declaration_list" => {
                allows_dead = allows_dead || inner_attributes_allow_dead_code(&node, source);
            }
            _ => {}
        }
        let is_function = node.kind() == "function_item";
        if is_function || matches!(node.kind(), "mod_item" | "impl_item" | "trait_item") {
            let attributes = preceding_attributes(&node, source);
            allows_dead = allows_dead
                || attributes
                    .iter()
                    .any(|(path, arguments)| allows_dead_code(path, arguments));
            if let Some(name) = node.child_by_field_name("name").filter(|_| is_function) {
                let is_runtime_attribute = |path: &str, arguments: &str| {
                    let last = path.rsplit("::").next().unwrap_or(path);
                    RUST_RUNTIME_ATTRIBUTES.contains(&last)
                        || (path == "unsafe"
                            && (arguments.contains("no_mangle")
                                || arguments.contains("export_name")))
                };
                info.insert(
                    name.start_byte(),
                    RustFunctionInfo {
                        runtime_entry: allows_dead
                            || has_foreign_abi(&node)
                            || attributes
                                .iter()
                                .any(|(path, arguments)| is_runtime_attribute(path, arguments)),
                        attributes: attributes
                            .iter()
                            .map(|(path, _)| path.to_string())
                            .collect(),
                        is_trait_impl: in_trait_impl,
                    },
                );
            }
        }
        let mut cursor = node.walk();
        stack.extend(
            node.children(&mut cursor)
                .map(|child| (child, in_trait_impl, allows_dead)),
        );
    }
    info
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

/// Caller name recorded for calls made at module scope. Paired with
/// `CallReference::from_module_scope`, which is what consumers test.
const MODULE_SCOPE: &str = "<module>";

/// Walk the AST to collect identifier usages and the uses of a function as a value.
/// A use as a value is an identifier that is not the target of a call, as the
/// `used` in `Spec { handler: used }` and in `register("data", used)` are. The
/// index gives the edges of the calls, and these uses give the other edges.
/// Uses iterative traversal with a single TreeCursor for performance.
fn collect_usages_and_calls(result: &parser::ParseResult, fdc: &mut FileDeadCode) {
    let source = &result.source;
    let lang = result.language;
    let mut cursor = result.tree.walk();
    // A stack rather than a single slot: leaving a nested function must
    // restore the enclosing one, otherwise every call made after a nested
    // function in the same body loses its caller.
    let mut scopes: Vec<(u32, Option<String>)> = Vec::new();
    // The depths of the token trees of the Rust macro calls around the node.
    // The walk keeps them, because a question to the tree for the parent of
    // each identifier costs a search from the root.
    let mut token_tree_depths: Vec<u32> = Vec::new();
    let mut value_uses: BTreeSet<(String, String, bool, bool)> = BTreeSet::new();
    let mut path_uses: Vec<PathUse> = Vec::new();

    // Iterative pre-order traversal
    loop {
        let node = cursor.node();
        let kind = node.kind();

        // Arriving at depth d means every scope opened at depth >= d has been
        // left behind.
        while scopes
            .last()
            .is_some_and(|(depth, _)| *depth >= cursor.depth())
        {
            scopes.pop();
        }
        while token_tree_depths
            .last()
            .is_some_and(|depth| *depth >= cursor.depth())
        {
            token_tree_depths.pop();
        }
        if lang == Language::Rust && kind == "token_tree" {
            token_tree_depths.push(cursor.depth());
        }

        // Track function context. The name may live on the binding rather
        // than on the function node, as it does for `const f = () => {}`.
        // Unbound functions are pushed too, with no name, so that a call
        // inside an anonymous callback is not mistaken for module-level code.
        if is_function_node(kind) {
            scopes.push((
                cursor.depth(),
                parser::function_binding_name(&node, source, lang),
            ));
        }

        // Code at module scope runs when the file is loaded, so uses made
        // there have a real caller -- `export const W = memo(Internal)`
        // genuinely reaches `Internal`. An anonymous function with no named
        // ancestor is in code that runs at module scope or that a callback
        // runs, thus its uses count as module scope too.
        let from_module_scope = scopes.is_empty();
        let current_function = if from_module_scope {
            Some(MODULE_SCOPE.to_string())
        } else {
            scopes.iter().rev().find_map(|(_, name)| name.clone())
        };

        // Collect usages from identifiers (excluding definitions)
        if (kind == "identifier" || kind == "type_identifier") && !is_definition_context(&node) {
            if let Ok(name) = node.utf8_text(source) {
                fdc.usages.insert(name.to_string());
                if kind == "identifier" && !is_call_target(&node) {
                    let (caller, module_scope) = match &current_function {
                        Some(caller) => (caller.clone(), from_module_scope),
                        None => (MODULE_SCOPE.to_string(), true),
                    };
                    let in_macro_tokens = !token_tree_depths.is_empty();
                    value_uses.insert((caller, name.to_string(), module_scope, in_macro_tokens));
                }
            }
        }

        // `#[serde(default = "path")]` names a function in a string. The derive calls it.
        if lang == Language::Rust && kind == "attribute_item" {
            if let Ok(text) = node.utf8_text(source) {
                let chain = rust_module_chain(&node, source);
                for path in serde_function_names(text) {
                    let segments: Vec<String> = path.split("::").map(str::to_string).collect();
                    if let Some(name) = segments.last() {
                        fdc.usages.insert(name.clone());
                    }
                    path_uses.push(PathUse {
                        file: fdc.path.clone(),
                        chain: chain.clone(),
                        segments,
                    });
                }
            }
        }

        // A Ruby symbol can name a method: `before_action :authenticate`,
        // `method(:run)`, and `send(:run)` call the method with that name.
        if lang == Language::Ruby && kind == "simple_symbol" {
            if let Ok(text) = node.utf8_text(source) {
                let name = text.trim_start_matches(':');
                fdc.usages.insert(name.to_string());
                let (caller, module_scope) = match &current_function {
                    Some(caller) => (caller.clone(), from_module_scope),
                    None => (MODULE_SCOPE.to_string(), true),
                };
                value_uses.insert((caller, name.to_string(), module_scope, false));
            }
        }

        // `Util.replacer` reads the function `replacer` as a property.
        if kind == "property_identifier" && is_read_as_property(&node) {
            if let Ok(name) = node.utf8_text(source) {
                fdc.usages.insert(name.to_string());
                let (caller, module_scope) = match &current_function {
                    Some(caller) => (caller.clone(), from_module_scope),
                    None => (MODULE_SCOPE.to_string(), true),
                };
                value_uses.insert((caller, name.to_string(), module_scope, false));
            }
        }

        // Move to next node in pre-order traversal
        if cursor.goto_first_child() {
            continue;
        }

        // No children, try siblings or go up
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                fdc.calls = value_uses
                    .into_iter()
                    .map(
                        |(caller, callee, from_module_scope, in_macro_tokens)| CallReference {
                            caller,
                            callee,
                            file: fdc.path.clone(),
                            from_module_scope,
                            in_macro_tokens,
                        },
                    )
                    .collect();
                fdc.path_uses = path_uses;
                return; // Done traversing
            }
        }
    }
}

/// The keys of a serde attribute whose string is the path of a function. For `with`, the
/// path is a module, and the derive calls its `serialize` and `deserialize`.
const SERDE_FUNCTION_KEYS: [&str; 7] = [
    "schema_with",
    "default",
    "skip_serializing_if",
    "serialize_with",
    "deserialize_with",
    "getter",
    "with",
];

/// The paths of the functions that the strings of a `serde` attribute name, with `::`
/// between the segments. A `with` string names a module, so it gives the paths of its
/// `serialize` and `deserialize`.
fn serde_function_names(attribute: &str) -> Vec<String> {
    let Some(start) = ["serde(", "schemars("]
        .iter()
        .filter_map(|name| attribute.find(name))
        .min()
    else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut rest = &attribute[start..];
    while let Some(quote) = rest.find('"') {
        let before = rest[..quote].trim_end();
        let Some(before) = before.strip_suffix('=') else {
            // A string that no `=` precedes. Skip it.
            let Some(end) = rest[quote + 1..].find('"') else {
                break;
            };
            rest = &rest[quote + end + 2..];
            continue;
        };
        let key: String = before
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let Some(end) = rest[quote + 1..].find('"') else {
            break;
        };
        let value = &rest[quote + 1..quote + 1 + end];
        if SERDE_FUNCTION_KEYS.contains(&key.as_str()) {
            if value.is_empty() {
            } else if key == "with" {
                names.push(format!("{value}::serialize"));
                names.push(format!("{value}::deserialize"));
            } else {
                names.push(value.to_string());
            }
        }
        rest = &rest[quote + end + 2..];
    }
    names
}

/// Resolves the path of a Rust attribute through the module tree of the index: the `mod`
/// items, in files and inline, and the `use` bindings, also across files.
struct RustPaths<'a> {
    modules: RustModules<'a>,
    files: HashMap<&'a str, &'a FileSymbols>,
    trees: HashMap<&'a str, &'a parser::ParseResult>,
    /// The keys of the definitions, by name.
    by_name: &'a HashMap<String, Vec<SharedStr>>,
}

impl<'a> RustPaths<'a> {
    /// Whether the file has an inline `mod` with the path `chain`.
    fn has_inline_module(&self, file: &str, chain: &[String]) -> bool {
        let wanted = chain.join(".");
        self.files.get(file).is_some_and(|facts| {
            facts
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Module && key_path(&symbol.key) == wanted)
        })
    }

    /// The module that the segments name from `(file, chain)`. The segments start with
    /// `crate`, `self`, or `super`, or they name a child module.
    fn module(
        &self,
        file: &str,
        chain: &[String],
        segments: &[String],
    ) -> Option<(String, Vec<String>)> {
        let mut file = file.to_string();
        let mut chain = chain.to_vec();
        let mut position = 0;
        match segments.first().map(String::as_str) {
            Some("crate") => {
                // A file outside a crate that the index knows is the root of its own module tree.
                file = self.modules.root(&file).unwrap_or(file);
                chain.clear();
                position = 1;
            }
            Some("self") => position = 1,
            _ => {}
        }
        while segments.get(position).map(String::as_str) == Some("super") {
            if chain.pop().is_none() {
                file = self.modules.parent(&file)?;
            }
            position += 1;
        }
        for segment in &segments[position..] {
            chain.push(segment.clone());
            if !self.has_inline_module(&file, &chain) {
                chain.pop();
                file = self.modules.child(&file, segment)?;
                chain.clear();
            }
        }
        Some((file, chain))
    }

    /// The keys of the functions that the path names from `(file, chain)`. Conditional
    /// definitions of one name, as `#[cfg(...)]` alternatives, have the ordinals `~2`, `~3`.
    /// Each of them is a function that the path can name.
    fn function(&self, file: &str, chain: &[String], segments: &[String]) -> Vec<SharedStr> {
        let Some((name, modules)) = segments.split_last() else {
            return Vec::new();
        };
        let Some((file, chain)) = self.module(file, chain, modules) else {
            return Vec::new();
        };
        let path = chain
            .iter()
            .chain(std::iter::once(name))
            .cloned()
            .collect::<Vec<String>>()
            .join(".");
        let prefix = format!("{file}#");
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|key| key.starts_with(&prefix) && key_path(key) == path)
            .cloned()
            .collect()
    }

    /// The `use` bindings that sit in the module `(file, chain)`.
    fn bindings(
        &self,
        file: &str,
        chain: &[String],
    ) -> Vec<&'a crate::index::model::ImportBinding> {
        let (Some(facts), Some(tree)) = (self.files.get(file), self.trees.get(file)) else {
            return Vec::new();
        };
        facts
            .imports
            .iter()
            .filter(|binding| {
                let at = binding.range.start_byte;
                tree.tree
                    .root_node()
                    .descendant_for_byte_range(at, at)
                    .is_some_and(|node| rust_module_chain(&node, &tree.source) == chain)
            })
            .collect()
    }

    /// The keys of the function that the path names in the module `(file, chain)`: first
    /// through the `use` bindings of the module, then in the module itself, then through
    /// its glob imports.
    fn resolve(&self, path_use: &PathUse) -> Vec<SharedStr> {
        use crate::index::model::ImportKind;
        let file = &path_use.file;
        let chain = &path_use.chain;
        let segments = &path_use.segments;
        let first = segments.first().map(String::as_str).unwrap_or("");
        if matches!(first, "crate" | "self" | "super") {
            return self.function(file, chain, segments);
        }
        let bindings = self.bindings(file, chain);
        for binding in &bindings {
            if binding.local_name.as_deref() != Some(first)
                || !matches!(binding.kind, ImportKind::Named | ImportKind::Module)
            {
                continue;
            }
            // The path of a `use` ends with the name that it brings.
            let mut full: Vec<String> = binding
                .module_path
                .split("::")
                .map(str::to_string)
                .collect();
            full.extend(segments[1..].iter().cloned());
            let found = self.function(file, chain, &full);
            if !found.is_empty() {
                return found;
            }
        }
        let found = self.function(file, chain, segments);
        if !found.is_empty() {
            return found;
        }
        for binding in &bindings {
            if binding.kind != ImportKind::Wildcard {
                continue;
            }
            let glob: Vec<String> = binding
                .module_path
                .split("::")
                .map(str::to_string)
                .collect();
            let Some((glob_file, glob_chain)) = self.module(file, chain, &glob) else {
                continue;
            };
            let found = self.function(&glob_file, &glob_chain, segments);
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }
}

/// The path of a key without the file and the ordinal, for example `codec.serialize`.
fn key_path(key: &str) -> &str {
    let path = key.split_once('#').map_or(key, |(_, path)| path);
    path.split('~').next().unwrap_or(path)
}

/// The names of the `mod` items around a node, from the outermost to the innermost.
fn rust_module_chain(node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item" {
            if let Some(name) = parent
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source).ok())
            {
                chain.push(name.to_string());
            }
        }
        current = parent.parent();
    }
    chain.reverse();
    chain
}

/// Whether a property name is read as a value: it is the member of a member
/// expression, and the expression is not the callee of a call.
fn is_read_as_property(node: &tree_sitter::Node<'_>) -> bool {
    let Some(access) = node.parent().filter(|p| p.kind() == "member_expression") else {
        return false;
    };
    if access
        .child_by_field_name("property")
        .is_none_or(|property| property.id() != node.id())
    {
        return false;
    }
    !access.parent().is_some_and(|call| {
        call.kind() == "call_expression"
            && call
                .child_by_field_name("function")
                .is_some_and(|function| function.id() == access.id())
    })
}

/// Whether the identifier names the callee of a call, or a member of an object,
/// and not a value. The call or the member access has its own reference in
/// the index.
fn is_call_target(node: &tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let kind = parent.kind();
    let is_access = ["member", "access", "attribute", "selector", "scoped"]
        .iter()
        .any(|part| kind.contains(part));
    if is_access
        && parent
            .named_child(0)
            .is_some_and(|object| object.id() != node.id())
    {
        let calls_the_access = parent.parent().is_some_and(|call| {
            let call_kind = call.kind();
            (call_kind.contains("call") || call_kind.contains("invocation"))
                && call
                    .child_by_field_name("function")
                    .is_some_and(|function| function.id() == parent.id())
        });
        if calls_the_access {
            return true;
        }
    }
    if !(kind.contains("call") || kind.contains("invocation") || kind == "command") {
        return false;
    }
    ["function", "name", "method"].iter().any(|field| {
        parent
            .child_by_field_name(field)
            .is_some_and(|child| child.id() == node.id())
    })
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
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
            | "method"
    )
}

fn is_definition_context(node: &tree_sitter::Node<'_>) -> bool {
    if let Some(parent) = node.parent() {
        let parent_kind = parent.kind();
        // C and C++ put the name of a function in a declarator.
        if parent_kind == "function_declarator"
            && parent
                .child_by_field_name("declarator")
                .is_some_and(|declarator| declarator.id() == node.id())
        {
            return true;
        }
        // Check if this identifier is the "name" field of a definition
        if let Some(name_node) = parent.child_by_field_name("name") {
            if name_node.id() == node.id() {
                return matches!(
                    parent_kind,
                    "function_declaration"
                        | "function_definition"
                        | "function_expression"
                        | "generator_function"
                        | "generator_function_declaration"
                        | "method_declaration"
                        | "variable_declarator"
                        | "let_declaration"
                        | "const_item"
                        | "static_item"
                        | "class_declaration"
                        | "struct_item"
                        | "function_item"
                        | "mod_item"
                );
            }
        }
    }
    false
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
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
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

/// Whether the definition is a function that an object literal or an assignment
/// to a property holds, in the JavaScript family. A framework or a caller
/// outside of the analysis reaches it by its name on an object, thus the
/// analysis cannot call it dead.
fn is_member_of_an_object(result: &parser::ParseResult, symbol: &Symbol) -> bool {
    if !matches!(
        result.language,
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx
    ) {
        return false;
    }
    let Some(mut node) = result
        .tree
        .root_node()
        .descendant_for_byte_range(symbol.range.start_byte, symbol.range.end_byte)
    else {
        return false;
    };
    while node.start_byte() == symbol.range.start_byte && node.end_byte() == symbol.range.end_byte {
        let parent = node.parent();
        let holds_a_property = match node.kind() {
            "pair" => true,
            "assignment_expression" => node
                .child_by_field_name("left")
                .is_some_and(|left| left.kind() == "member_expression"),
            "method_definition" => parent.is_some_and(|p| p.kind() == "object"),
            "arrow_function" | "function_expression" | "function" => {
                parent.is_some_and(|p| match p.kind() {
                    "pair" => true,
                    "assignment_expression" => p
                        .child_by_field_name("left")
                        .is_some_and(|left| left.kind() == "member_expression"),
                    _ => false,
                })
            }
            _ => false,
        };
        if holds_a_property {
            return true;
        }
        match parent {
            Some(p) => node = p,
            None => return false,
        }
    }
    false
}

/// Whether the syntax lets a file other than the file of the definition name it.
///
/// A function that has no type around it is visible to other files only when
/// it is exported or when the syntax does not say. A method is visible, except
/// when a keyword makes it private to its class or its file.
fn is_nameable_from_other_files(
    symbol: &Symbol,
    language: Language,
    exported: bool,
    visibility: &str,
) -> bool {
    let has_word = |word: &str| {
        symbol
            .signature
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|part| part == word)
    };
    if has_word("pub") {
        return true;
    }
    if symbol.container.is_none() && symbol.kind == SymbolKind::Function {
        return exported || !matches!(symbol.exported, Exported::NotExported);
    }
    let private_by_keyword = match language {
        Language::Java
        | Language::CSharp
        | Language::Php
        | Language::TypeScript
        | Language::Tsx => has_word("private"),
        Language::C | Language::Cpp => has_word("static"),
        Language::Ruby => visibility == "private" || visibility == "protected",
        _ => false,
    };
    !private_by_keyword
}

/// The keys of the Go methods whose type has each method name of one
/// interface of the repository, when the interface has the name of the
/// method. The rule reads the own methods of the interface and of the type.
/// It does not read the embedded types or the signatures. Thus it can find
/// more methods than the compiler would need.
fn go_interface_methods(files: &[FileSymbols]) -> Vec<SharedStr> {
    let go_files = || files.iter().filter(|facts| facts.language == Language::Go);
    // The method names of each interface.
    let mut interfaces: Vec<BTreeSet<&str>> = Vec::new();
    for facts in go_files() {
        let mut by_interface: HashMap<&str, BTreeSet<&str>> = HashMap::new();
        let interface_names: HashSet<&str> = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Interface)
            .map(|symbol| symbol.name.as_str())
            .collect();
        for symbol in &facts.symbols {
            if let (SymbolKind::Method, None, Some(container)) = (
                symbol.kind,
                &symbol.receiver_type,
                symbol.container.as_deref(),
            ) {
                if interface_names.contains(container) {
                    by_interface
                        .entry(container)
                        .or_default()
                        .insert(symbol.name.as_str());
                }
            }
        }
        interfaces.extend(by_interface.into_values());
    }
    // The methods of each type, by the directory of the package and the type.
    let mut types: HashMap<(&str, &str), Vec<&Symbol>> = HashMap::new();
    for facts in go_files() {
        let package = Path::new(&facts.file)
            .parent()
            .and_then(Path::to_str)
            .unwrap_or_default();
        for symbol in &facts.symbols {
            if let (SymbolKind::Method, Some(owner)) =
                (symbol.kind, symbol.receiver_type.as_deref())
            {
                types.entry((package, owner)).or_default().push(symbol);
            }
        }
    }
    let type_names: Vec<BTreeSet<&str>> = types
        .values()
        .map(|methods| methods.iter().map(|method| method.name.as_str()).collect())
        .collect();
    // A type that implements an interface has each name of the interface, so
    // each interface is checked only for the types with its least common
    // name. Each pair of a type and an interface is checked at most one time.
    let mut types_with_name: HashMap<&str, usize> = HashMap::new();
    for names in &type_names {
        for name in names {
            *types_with_name.entry(name).or_default() += 1;
        }
    }
    let interfaces: Vec<BTreeSet<&str>> = interfaces
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut by_anchor: HashMap<&str, Vec<usize>> = HashMap::new();
    for (position, names) in interfaces.iter().enumerate() {
        let anchor = names
            .iter()
            .min_by_key(|name| (types_with_name.get(*name).copied().unwrap_or(0), **name));
        if let Some(anchor) = anchor {
            by_anchor.entry(anchor).or_default().push(position);
        }
    }
    let mut found = Vec::new();
    for (methods, names) in types.values().zip(&type_names) {
        let implemented: HashSet<usize> = names
            .iter()
            .flat_map(|name| by_anchor.get(name).into_iter().flatten().copied())
            .filter(|position| interfaces[*position].is_subset(names))
            .collect();
        let needed: HashSet<&str> = implemented
            .iter()
            .flat_map(|position| interfaces[*position].iter().copied())
            .collect();
        found.extend(
            methods
                .iter()
                .filter(|method| needed.contains(method.name.as_str()))
                .map(|method| method.key.clone()),
        );
    }
    found.sort();
    found
}

/// Whether a call that the index cannot resolve can name the definition.
///
/// The definition must be in the same group of languages. A syntax that keeps
/// the definition private to its file hides it from a call in a different
/// file. A call with a receiver cannot name a function that has no type around
/// it, except in a language where a module or an object holds functions.
fn can_be_named_by(def: &Definition, call: &Reference, ancestors_of_call: &[String]) -> bool {
    let def_language = Language::detect(Path::new(&def.file));
    let call_language = Language::detect(Path::new(call.file.as_str()));
    let (Some(def_language), Some(call_language)) = (def_language, call_language) else {
        return false;
    };
    if language_group(def_language) != language_group(call_language) {
        return false;
    }
    // A Go name that is not exported is visible in each file of its package.
    let same_go_package = def_language == Language::Go
        && Path::new(&def.file).parent() == Path::new(call.file.as_str()).parent();
    // A private Rust item is visible in its module and in each module below it.
    let is_in_ancestor_module =
        def_language == Language::Rust && ancestors_of_call.contains(&def.file);
    if def.file != call.file
        && !def.nameable_from_other_files
        && !same_go_package
        && !is_in_ancestor_module
    {
        return false;
    }
    let receiver_calls_a_member = matches!(
        call.receiver,
        Some(Receiver::Name(_) | Receiver::Expression | Receiver::SelfValue | Receiver::Super)
    );
    let functions_live_in_objects = matches!(
        def_language,
        Language::Python
            | Language::Ruby
            | Language::JavaScript
            | Language::Jsx
            | Language::TypeScript
            | Language::Tsx
    );
    !(receiver_calls_a_member && def.is_free_function && !functions_live_in_objects)
}

/// The files of the modules above the module of a Rust file, by file.
struct RustAncestors<'a> {
    /// The file that declares the module of a file with `mod name;`. The
    /// path rules of Rust give it, thus a crate root with any name has its
    /// children.
    parents: HashMap<&'a str, &'a str>,
    of_file: HashMap<String, std::rc::Rc<[String]>>,
}

impl<'a> RustAncestors<'a> {
    fn new(files: &'a [FileSymbols]) -> Self {
        use crate::index::model::ImportKind;
        let is_rust = |facts: &&FileSymbols| facts.language == Language::Rust;
        let rust_files: HashMap<&str, &str> = files
            .iter()
            .filter(is_rust)
            .map(|facts| (facts.file.as_str(), facts.file.as_str()))
            .collect();
        let mut parents: HashMap<&str, &str> = HashMap::new();
        for facts in files.iter().filter(is_rust) {
            for binding in &facts.imports {
                if binding.kind != ImportKind::ModuleDeclaration {
                    continue;
                }
                // The path rules of Rust give the module file. They also
                // apply to a crate root that the index does not know.
                let child = crate::index::test_code::rust_module_file(
                    &facts.file,
                    &binding.module_path,
                    |path| rust_files.get(path).copied(),
                );
                if let Some(child) = child {
                    // The files are in path order, thus the first declaration
                    // is the same in each run.
                    parents.entry(child).or_insert(facts.file.as_str());
                }
            }
        }
        Self {
            parents,
            of_file: HashMap::new(),
        }
    }

    /// The file of each module above the module of `file`, from the parent to
    /// the root of the crate. A file that is not Rust has none.
    fn of(&mut self, file: &str) -> std::rc::Rc<[String]> {
        if let Some(found) = self.of_file.get(file) {
            return std::rc::Rc::clone(found);
        }
        let mut chain: Vec<String> = Vec::new();
        let mut current = self.parents.get(file).copied();
        while let Some(parent) = current {
            if parent == file || chain.iter().any(|seen| seen == parent) {
                break;
            }
            chain.push(parent.to_string());
            current = self.parents.get(parent).copied();
        }
        let chain: std::rc::Rc<[String]> = chain.into();
        self.of_file
            .insert(file.to_string(), std::rc::Rc::clone(&chain));
        chain
    }
}

fn language_group(language: Language) -> u8 {
    match language {
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => 1,
        Language::C | Language::Cpp => 2,
        Language::Go => 3,
        Language::Rust => 4,
        Language::Python => 5,
        Language::Java => 6,
        Language::CSharp => 7,
        Language::Ruby => 8,
        Language::Php => 9,
        Language::Bash => 10,
    }
}

fn is_entry_point(name: &str, def: &Definition) -> bool {
    // Standard entry points
    if name == "main" || name == "init" || name == "Main" {
        return true;
    }

    // A test runner calls a test, and the runtime calls the other definitions.
    // `crate::index::test_code` has the rule for a test.
    if def.test_role == TestRole::Entry || def.runtime_entry {
        return true;
    }

    // Python decorators and __name__ == "__main__" calls
    for attr in &def.attributes {
        // Functions called in `if __name__ == "__main__"` blocks
        if attr == "__name__main_call" {
            return true;
        }
        // Python decorator-based entry points
        if is_entry_point_decorator(attr) {
            return true;
        }
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

    // Reduce for test code
    if def.test_role != TestRole::None {
        confidence -= 0.15;
    }

    confidence.clamp(0.0, 1.0)
}

// Internal types for file analysis
struct FileDeadCode {
    path: String,
    definitions: Vec<Definition>,
    usages: HashSet<String>,
    /// The uses of a function as a value, as `register("data", handler)` does.
    /// A call has its own reference in the shared index.
    calls: Vec<CallReference>,
    /// The functions that a Rust attribute names with a path.
    path_uses: Vec<PathUse>,
}

/// A function that a path names, such as the string of `#[serde(default = "paths::fallback")]`.
#[derive(Clone)]
struct PathUse {
    /// The file of the attribute.
    file: String,
    /// The names of the inline `mod` items around the attribute, from the outermost.
    chain: Vec<String>,
    /// The segments of the path.
    segments: Vec<String>,
}

#[cfg(test)]
fn file_data(result: &parser::ParseResult) -> FileDeadCode {
    let facts = crate::index::extract::extract_file(result, &result.path.to_string_lossy());
    collect_file_data(result, &facts)
}

#[cfg(test)]
impl FileDeadCode {
    /// The first definition with this name.
    fn definition(&self, name: &str) -> Option<&Definition> {
        self.definitions.iter().find(|def| def.name == name)
    }
}

#[derive(Clone)]
struct Definition {
    /// The key of the symbol in the shared index.
    key: SharedStr,
    name: String,
    kind: String,
    file: String,
    line: u32,
    end_line: u32,
    visibility: String,
    exported: bool,
    /// The relation to test code, as the shared index gives it.
    test_role: TestRole,
    /// The runtime, the loader, or a tool calls the definition, and no call
    /// site names it. A Rust function with `#[ctor]`, with `#[no_mangle]`,
    /// or with a foreign ABI is such a definition, and a Python special
    /// method is one. A Rust function for which the author permits the lint
    /// `dead_code` is one too, as it is for the compiler.
    runtime_entry: bool,
    /// Attributes on this definition (e.g., "test", "tokio::test", "bench")
    attributes: Vec<String>,
    /// Whether this is a trait implementation method
    is_trait_impl: bool,
    /// The syntax lets a file other than the file of the definition name it.
    /// A definition that is not private, or whose visibility the syntax does not give, is such a definition.
    nameable_from_other_files: bool,
    /// A function that has no type or function around it.
    is_free_function: bool,
}

#[derive(Clone)]
struct CallReference {
    caller: String,
    callee: String,
    file: String,
    /// Whether the call was made at module scope rather than inside a
    /// function. Structural rather than a reserved caller name, which a
    /// property binding could otherwise collide with.
    from_module_scope: bool,
    /// The name is a token of a Rust macro call. The syntax does not show if
    /// a receiver or a path comes before it.
    in_macro_tokens: bool,
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
    /// The definition is test code. See `crate::index::test_code`.
    #[serde(default)]
    pub is_test: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_items: usize,
    pub by_kind: HashMap<String, usize>,
    pub total_definitions: usize,
    pub reachable_count: usize,
    /// The serde paths of attributes that the module tree could not resolve. Deadcode keeps
    /// each function with the last name of such a path alive.
    #[serde(default)]
    pub unresolved_paths: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_uses_content_source_for_historical_commits() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet, FilesystemSource};
        use std::path::PathBuf;
        use std::sync::Arc;

        let current = tempfile::tempdir().unwrap();
        let historical = tempfile::tempdir().unwrap();
        std::fs::write(
            current.path().join("code.ts"),
            "export function currentVersion() {}\n",
        )
        .unwrap();
        std::fs::write(
            historical.path().join("code.ts"),
            "function historicalUnused() {}\n",
        )
        .unwrap();
        let files =
            FileSet::from_files(current.path().to_path_buf(), vec![PathBuf::from("code.ts")]);
        let config = Config::default();
        let source = Arc::new(FilesystemSource::new(historical.path()));
        let ctx =
            AnalysisContext::new(&files, &config, Some(current.path())).with_content_source(source);

        let result = Analyzer::new().analyze(&ctx).unwrap();

        assert!(result
            .items
            .iter()
            .any(|item| item.name == "historicalUnused"));
        assert!(!result
            .items
            .iter()
            .any(|item| item.name == "currentVersion"));
    }

    #[test]
    fn test_a_file_that_fails_to_read_after_the_index_is_an_error() {
        let error = analyze_with_second_read(None).unwrap_err();
        assert!(error.to_string().contains(CHANGED), "{error}");
    }

    #[test]
    fn test_a_file_whose_bytes_changed_after_the_index_is_an_error() {
        let error = analyze_with_second_read(Some("function other() {}\n")).unwrap_err();
        assert!(error.to_string().contains(CHANGED), "{error}");
    }

    const CHANGED: &str =
        "a.ts changed or became unreadable during analysis; run the command again";

    /// The second read of `a.ts` fails, or it gives `second`.
    fn analyze_with_second_read(second: Option<&'static str>) -> Result<Analysis> {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, ContentSource, FileSet};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct SecondRead(AtomicUsize, Option<&'static str>);
        impl ContentSource for SecondRead {
            fn read(&self, path: &Path) -> Result<Vec<u8>> {
                if path == Path::new("a.ts") && self.0.fetch_add(1, Ordering::SeqCst) == 1 {
                    return match self.1 {
                        Some(text) => Ok(text.as_bytes().to_vec()),
                        None => Err(crate::core::Error::analysis("gone")),
                    };
                }
                let stem = path.file_stem().unwrap().to_string_lossy();
                Ok(format!("function unusedIn_{stem}() {{}}\n").into_bytes())
            }
            fn list_directory(&self, _: &Path) -> Result<Vec<PathBuf>> {
                Ok(Vec::new())
            }
        }

        let files = FileSet::from_files(
            PathBuf::from("."),
            vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
        );
        let config = Config::default();
        let ctx = AnalysisContext::new(&files, &config, Some(Path::new(".")))
            .with_content_source(Arc::new(SecondRead(AtomicUsize::new(0), second)));

        Analyzer::new().analyze(&ctx)
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "deadcode");
    }

    #[test]
    fn test_cargo_check_is_opt_in() {
        assert!(!Analyzer::new().cargo_check);
        assert!(Analyzer::new().with_cargo_check(true).cargo_check);
    }

    #[test]
    fn test_is_entry_point() {
        let def = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "main".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("main", &def));

        let def2 = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "TestSomething".to_string(),
            kind: "function".to_string(),
            file: "foo_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            test_role: TestRole::Support,
            runtime_entry: false,
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
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        let conf = calculate_confidence(&private_def, true, true);
        assert!(conf > 0.9); // High confidence for private, unreachable, unused

        let exported_def = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "Public".to_string(),
            kind: "function".to_string(),
            file: "lib.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            test_role: TestRole::None,
            runtime_entry: false,
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
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "src/util.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };

        let def2 = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "src/parser.go".to_string(),
            line: 5,
            end_line: 15,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
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
        let fdc = file_data(&result);

        // The #[test] function should be recognized as an entry point
        let test_fn = fdc.definition("my_weird_test_name").unwrap();
        assert!(
            is_entry_point("my_weird_test_name", test_fn),
            "Function with #[test] attribute should be an entry point"
        );

        // The helper function should NOT be an entry point
        let helper_fn = fdc.definition("helper_not_a_test").unwrap();
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
        let fdc = file_data(&result);

        // The #[tokio::test] function should be recognized as an entry point
        let test_fn = fdc.definition("my_async_test").unwrap();
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
        let fdc = file_data(&result);

        // Functions inside #[cfg(test)] should be recognized as being in test context
        let helper_fn = fdc.definition("helper_in_test_module");
        assert!(
            helper_fn.is_some(),
            "helper_in_test_module should be found in definitions"
        );

        let helper = helper_fn.unwrap();
        // The helper function should be marked as in a test context
        assert!(
            helper.test_role == TestRole::Support,
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
        let fdc = file_data(&result);

        // The trait impl method should be found and marked as a trait impl
        let trait_method = fdc.definition("trait_method");
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
        let fdc = file_data(&result);

        // The pub function should be marked as exported and be an entry point
        let pub_fn = fdc.definition("public_api_function").unwrap();
        assert!(pub_fn.exported, "pub function should be marked as exported");
        assert!(
            is_entry_point("public_api_function", pub_fn),
            "pub function should be an entry point"
        );

        // The private function should NOT be an entry point
        let private_fn = fdc.definition("private_helper").unwrap();
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
        let fdc = file_data(&result);

        let names: HashSet<&str> = fdc
            .definitions
            .iter()
            .map(|def| def.name.as_str())
            .collect();
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
        let functions = file_data(&result).definitions;

        assert_eq!(
            functions.len(),
            5,
            "the index should find all 5 Rust functions, got: {:?}",
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
        let fdc = file_data(&result);
        let internal_fn = fdc.definition("_internal_api").unwrap();
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
        let fdc = file_data(&result);

        let main_fn = fdc.definition("main").unwrap();
        assert!(
            is_entry_point("main", main_fn),
            "main() called in __name__ block should be an entry point"
        );

        let setup_fn = fdc.definition("setup").unwrap();
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
        let fdc = file_data(&result);

        let hello_fn = fdc.definition("hello").unwrap();
        assert!(
            is_entry_point("hello", hello_fn),
            "Flask @app.route decorated function should be an entry point, attrs: {:?}",
            hello_fn.attributes
        );

        let fixture_fn = fdc.definition("my_fixture").unwrap();
        assert!(
            is_entry_point("my_fixture", fixture_fn),
            "@pytest.fixture decorated function should be an entry point, attrs: {:?}",
            fixture_fn.attributes
        );

        let task_fn = fdc.definition("process_data").unwrap();
        assert!(
            is_entry_point("process_data", task_fn),
            "@celery.task decorated function should be an entry point, attrs: {:?}",
            task_fn.attributes
        );

        let prop_fn = fdc.definition("name").unwrap();
        assert!(
            is_entry_point("name", prop_fn),
            "@property decorated function should be an entry point, attrs: {:?}",
            prop_fn.attributes
        );

        // plain_function has no decorator -- it is still "public" by Python naming,
        // so it will be exported via is_exported. Check that it has no decorator attrs.
        let plain_fn = fdc.definition("plain_function").unwrap();
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
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "PublicFunc".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            test_role: TestRole::None,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(is_entry_point("PublicFunc", &def));

        let private_def = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "privateFunc".to_string(),
            kind: "function".to_string(),
            file: "main.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        assert!(!is_entry_point("privateFunc", &private_def));
    }

    #[test]
    fn test_go_benchmark_and_example_entry_points() {
        let def = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "BenchmarkSort".to_string(),
            kind: "function".to_string(),
            file: "sort_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "public".to_string(),
            exported: true,
            test_role: TestRole::Support,
            runtime_entry: false,
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
        let fdc = file_data(&result);

        assert!(
            fdc.definition("publicHelper").is_some(),
            "Should find publicHelper, got: {:?}",
            fdc.definitions
                .iter()
                .map(|def| &def.name)
                .collect::<Vec<_>>()
        );
        assert!(
            fdc.definition("exportedFunc").is_some(),
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
        let fdc = file_data(&result);

        assert!(
            fdc.definition("processData").is_some(),
            "Should find processData, got: {:?}",
            fdc.definitions
                .iter()
                .map(|def| &def.name)
                .collect::<Vec<_>>()
        );
        assert!(
            fdc.definition("helperMethod").is_some(),
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
        let fdc = file_data(&result);

        assert!(fdc.definition("public_func").is_some());
        assert!(fdc.definition("_helper").is_some());
        assert!(fdc.definition("__private").is_some());

        // Check visibility
        assert_eq!(fdc.definition("_helper").unwrap().visibility, "internal");
        assert_eq!(fdc.definition("__private").unwrap().visibility, "private");
        assert_eq!(fdc.definition("public_func").unwrap().visibility, "public");
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
        let fdc = file_data(&result);

        assert!(fdc.definition("public_api").is_some());
        assert!(fdc.definition("internal_helper").is_some());
        assert!(fdc.definition("nested_helper").is_some());

        assert!(fdc.definition("public_api").unwrap().exported);
        assert!(!fdc.definition("internal_helper").unwrap().exported);
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
        let fdc = file_data(&result);

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
    fn test_cross_file_function_as_value_creates_call_edge() {
        // When a function identifier is passed as an argument but is NOT
        // defined in the same file, a candidate call edge should still be
        // emitted. Phase 3 global resolution will handle matching it.
        use std::path::Path;

        let parser = crate::parser::Parser::new();
        let content = br#"
package main

func setup() {
    registerTool("data", handleData)
}
"#;
        // handleData and registerTool are NOT defined in this file.
        let result = parser
            .parse(content, Language::Go, Path::new("setup.go"))
            .unwrap();
        let fdc = file_data(&result);

        let has_edge = fdc
            .calls
            .iter()
            .any(|c| c.caller == "setup" && c.callee == "handleData");
        assert!(
            has_edge,
            "Should create candidate call edge for cross-file function-as-value. Calls: {:?}",
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
        let fdc = file_data(&result);

        assert!(
            fdc.definition("PublicFunc").is_some(),
            "Should find PublicFunc, got: {:?}",
            fdc.definitions
                .iter()
                .map(|def| &def.name)
                .collect::<Vec<_>>()
        );
        assert!(fdc.definition("privateHelper").is_some());

        assert!(fdc.definition("PublicFunc").unwrap().exported);
        assert!(!fdc.definition("privateHelper").unwrap().exported);
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
    fn test_generated_file_definitions_collected_but_not_reported() {
        // Generated files should have their definitions collected (so the call
        // graph is complete for reachability), but Phase 5 should skip them
        // when building the dead code report.
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
        let fdc = file_data(&result);

        // Definitions ARE collected for call graph participation
        assert!(
            !fdc.definitions.is_empty(),
            "Generated file definitions should be collected for call graph"
        );

        // But is_generated_file returns true so Phase 5 will skip them
        assert!(
            is_generated_file("conductor_grpc.pb.go"),
            "Should detect generated file"
        );
    }

    #[test]
    fn test_handler_patterns_are_entry_points() {
        let base_def = Definition {
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "".to_string(),
            kind: "function".to_string(),
            file: "handler.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::None,
            runtime_entry: false,
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
            key: SharedStr::default(),
            nameable_from_other_files: false,
            is_free_function: false,
            name: "helper".to_string(),
            kind: "function".to_string(),
            file: "helper_test.go".to_string(),
            line: 1,
            end_line: 10,
            visibility: "private".to_string(),
            exported: false,
            test_role: TestRole::Support,
            runtime_entry: false,
            attributes: vec![],
            is_trait_impl: false,
        };
        let non_test_def = Definition {
            test_role: TestRole::None,
            runtime_entry: false,
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
        let fdc = file_data(&result);

        let public_fn = fdc.definition("public_method").unwrap();
        assert_eq!(
            public_fn.visibility, "public",
            "Method before 'private' should be public"
        );
        assert!(public_fn.exported, "Public Ruby method should be exported");

        let secret_fn = fdc.definition("secret_method").unwrap();
        assert_eq!(
            secret_fn.visibility, "private",
            "Method after 'private' should be private"
        );
        assert!(
            !secret_fn.exported,
            "Private Ruby method should not be exported"
        );

        let another_fn = fdc.definition("another_secret").unwrap();
        assert_eq!(
            another_fn.visibility, "private",
            "Second method after 'private' should also be private"
        );

        let protected_fn = fdc.definition("protected_method").unwrap();
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

#[cfg(test)]
mod arrow_function_tests {
    use super::*;

    // --- Issue #495: arrows as first-class functions -----------------------

    fn js_file_data(source: &str) -> FileDeadCode {
        use std::path::Path;
        let result = crate::parser::Parser::new()
            .parse(source.as_bytes(), Language::TypeScript, Path::new("t.ts"))
            .unwrap();
        file_data(&result)
    }

    #[test]
    fn test_anonymous_callbacks_are_not_definitions() {
        // An anonymous callback can never be referenced by name, so treating
        // it as a definition would report it as dead with high confidence.
        let fdc = js_file_data("xs.map(x => x + 1);\nys.forEach(function () {});\n");
        assert!(
            fdc.definitions.is_empty(),
            "anonymous callbacks became definitions: {:?}",
            fdc.definitions
                .iter()
                .map(|def| &def.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_bound_arrow_is_a_definition() {
        let fdc = js_file_data("const helper = () => {};\n");
        assert!(
            fdc.definition("helper").is_some(),
            "got {:?}",
            fdc.definitions
                .iter()
                .map(|def| &def.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_exported_arrow_is_marked_exported() {
        // Without this, every exported React component and hook would be
        // reported as dead code.
        let fdc = js_file_data("export const useThing = () => {};\n");
        let definition = fdc
            .definition("useThing")
            .expect("useThing should be a definition");
        assert!(definition.exported, "exported arrow not marked exported");
    }

    #[test]
    fn test_named_function_expression_name_is_not_a_usage() {
        // The intrinsic name of `function dead() {}` is visible only inside
        // its own body, so counting it as a usage would mask the finding.
        let fdc = js_file_data("const dead = function dead() {};\n");
        assert!(
            !fdc.usages.contains("dead"),
            "the intrinsic name leaked into usages: {:?}",
            fdc.usages
        );
    }

    #[test]
    fn test_module_scope_calls_are_flagged_structurally() {
        // The marker is a field, not a reserved caller name, so a property
        // literally called `<module>` cannot forge module reachability.
        let fdc = js_file_data("const o = { \"<module>\": () => forged() };\n");
        assert!(
            fdc.calls
                .iter()
                .all(|call| !call.from_module_scope || call.callee != "forged"),
            "a property key forged a module-scope call: {:?}",
            fdc.calls
                .iter()
                .map(|c| (&c.caller, &c.callee, c.from_module_scope))
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use crate::config::Config;
    use crate::core::FileSet;

    /// The names of the definitions that the analysis reports as dead.
    fn dead_names(files: &[(&str, &str)]) -> Vec<String> {
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        let mut names: Vec<String> = Analyzer::new()
            .with_confidence(0.0)
            .analyze(&ctx)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.name)
            .collect();
        names.sort();
        names
    }

    #[test]
    fn test_a_called_helper_is_alive_in_each_language() {
        let sources: [(&str, &str); 14] = [
            ("m.rs", "fn main() { helper(); }\nfn helper() {}\nfn unused_one() {}\n"),
            (
                "m.go",
                "package main\nfunc main() { helper() }\nfunc helper() {}\nfunc unusedOne() {}\n",
            ),
            (
                "m.py",
                "def main():\n    helper()\n\ndef helper():\n    pass\n\ndef _unused_one():\n    pass\n",
            ),
            (
                "m.ts",
                "function main() { helper(); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.tsx",
                "function main() { helper(); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.js",
                "function main() { helper(); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.jsx",
                "function main() { helper(); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "M.java",
                "class M {\n  static void main(String[] a) { helper(); }\n  static void helper() {}\n  static void unusedOne() {}\n}\n",
            ),
            (
                "M.cs",
                "class M {\n  static void Main() { Helper(); }\n  static void Helper() {}\n  static void UnusedOne() {}\n}\n",
            ),
            ("m.c", "static void helper(void) {}\nstatic void unused_one(void) {}\nint main(void) { helper(); return 0; }\n"),
            (
                "m.cpp",
                "static void helper() {}\nstatic void unused_one() {}\nint main() { helper(); return 0; }\n",
            ),
            (
                "m.rb",
                "def main\n  helper\nend\n\ndef helper\nend\n\ndef unused_one\nend\n",
            ),
            (
                "m.php",
                "<?php\nfunction main() { helper(); }\nfunction helper() {}\nfunction unused_one() {}\n",
            ),
            ("m.sh", "main() {\n  helper\n}\nhelper() {\n  :\n}\nunused_one() {\n  :\n}\n"),
        ];
        for (name, text) in sources {
            let dead = dead_names(&[(name, text)]);
            assert!(
                !dead.contains(&"helper".to_string()) && !dead.contains(&"Helper".to_string()),
                "{name}: a called helper is dead: {dead:?}"
            );
            // A function of Ruby, PHP, or Bash is visible from each file, thus the
            // index marks it as exported and the analysis never reports it.
            if !name.ends_with(".rb") && !name.ends_with(".php") && !name.ends_with(".sh") {
                assert!(
                    dead.iter()
                        .any(|found| found.to_lowercase().replace('_', "") == "unusedone"),
                    "{name}: the unused function is not dead: {dead:?}"
                );
            }
        }
    }

    /// A function that a definition uses as a value can run. It is not dead.
    #[test]
    fn test_a_function_that_is_only_a_value_is_alive_in_each_language() {
        let sources: [(&str, &str); 14] = [
            ("m.rs", "fn main() { spawn(helper); }\nfn helper() {}\nfn unused_one() {}\n"),
            (
                "m.go",
                "package main\nfunc main() { submit(helper) }\nfunc helper() {}\nfunc unusedOne() {}\n",
            ),
            (
                "m.py",
                "def main(pool):\n    pool.submit(_helper)\n\ndef _helper():\n    pass\n\ndef _unused_one():\n    pass\n",
            ),
            (
                "m.ts",
                "function main(pool: Pool) { pool.submit(helper); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.tsx",
                "function main() { return <Button onClick={helper} />; }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.js",
                "function main(pool) { pool.submit(helper); }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "m.jsx",
                "function main() { return <Button onClick={helper} />; }\nfunction helper() {}\nfunction unusedOne() {}\n",
            ),
            (
                "M.java",
                "class M {\n  static void main(String[] a) { run(M::helper); }\n  private static void helper() {}\n  private static void unusedOne() {}\n}\n",
            ),
            (
                "M.cs",
                "class M {\n  static void Main() { Run(Helper); }\n  static void Helper() {}\n  static void UnusedOne() {}\n}\n",
            ),
            ("m.c", "static void helper(void) {}\nstatic void unused_one(void) {}\nint main(void) { atexit(helper); return 0; }\n"),
            (
                "m.cpp",
                "static void helper() {}\nstatic void unused_one() {}\nint main() { atexit(&helper); return 0; }\n",
            ),
            (
                "m.rb",
                "class M\n  def main(list)\n    list.each(&method(:helper))\n  end\n\n  private\n\n  def helper(item)\n  end\n\n  def unused_one\n  end\nend\n",
            ),
            (
                "m.php",
                "<?php\nclass M {\n    public function main($list) {\n        $held = function () {};\n        dispatch($held);\n        array_map([$this, 'helper'], $list);\n    }\n    private function helper($item) {}\n    private function unused_one() {}\n}\n",
            ),
            ("m.sh", "main() {\n  trap helper EXIT\n}\nhelper() {\n  :\n}\nunused_one() {\n  :\n}\n"),
        ];
        for (name, text) in sources {
            let dead = dead_names(&[(name, text)]);
            let alive = ["helper", "_helper", "Helper", "held"];
            assert!(
                !dead.iter().any(|found| alive.contains(&found.as_str())),
                "{name}: a function that is a value is dead: {dead:?}"
            );
            if !name.ends_with(".rb") && !name.ends_with(".sh") {
                assert!(
                    dead.iter()
                        .any(|found| found.to_lowercase().replace('_', "") == "unusedone"),
                    "{name}: the unused function is not dead: {dead:?}"
                );
            }
        }
    }

    #[test]
    fn test_a_callback_at_module_scope_keeps_its_callee_alive() {
        let dead = dead_names(&[(
            "m.js",
            "function helper(x) { return x; }\nfunction unusedOne() {}\n[1, 2].map(x => helper(x));\n",
        )]);

        assert!(!dead.contains(&"helper".to_string()), "{dead:?}");
        assert!(dead.contains(&"unusedOne".to_string()), "{dead:?}");
    }

    #[test]
    fn test_a_method_that_no_call_can_name_is_dead_when_a_same_name_method_is_called() {
        let dead = dead_names(&[(
            "m.ts",
            "class A { run() {} }\nclass B { run() {} }\nfunction main(a: A) { a.run(); }\n",
        )]);

        let run_count = dead.iter().filter(|name| name.as_str() == "run").count();
        assert_eq!(run_count, 1, "{dead:?}");
    }

    #[test]
    fn test_a_call_that_the_index_cannot_resolve_keeps_each_same_name_definition_alive() {
        let dead = dead_names(&[(
            "m.py",
            "class A:\n    def process(self):\n        pass\n\nclass B:\n    def process(self):\n        pass\n\ndef main(thing):\n    thing.process()\n",
        )]);

        assert!(!dead.contains(&"process".to_string()), "{dead:?}");
    }

    #[test]
    fn test_an_external_call_keeps_each_same_name_definition_alive() {
        let dead = dead_names(&[
            (
                "util.py",
                "class Repo:\n    def _append(self, value):\n        return value\n\n    def _unused(self):\n        return 1\n\nlist = Repo()\n",
            ),
            (
                "app.py",
                "from util import *\n\ndef main():\n    list._append(1)\n",
            ),
        ]);

        assert!(dead.contains(&"_unused".to_string()), "{dead:?}");
        assert!(!dead.contains(&"_append".to_string()), "{dead:?}");
    }

    #[test]
    fn test_an_unresolved_call_names_an_exported_definition_of_another_file() {
        let dead = dead_names(&[
            (
                "a.py",
                "def process():\n    pass\n\ndef _hidden():\n    pass\n",
            ),
            (
                "b.py",
                "def main(thing):\n    thing.process()\n    thing._hidden()\n",
            ),
        ]);

        assert!(!dead.contains(&"process".to_string()), "{dead:?}");
        assert!(dead.contains(&"_hidden".to_string()), "{dead:?}");
    }

    #[test]
    fn test_a_member_of_an_object_is_not_a_definition_in_the_javascript_family() {
        // A framework or a caller outside of the analysis reaches such a member
        // by its name on an object.
        for name in ["m.js", "m.jsx", "m.ts", "m.tsx"] {
            let dead = dead_names(&[(
                name,
                "export function make() { return { handler() {}, get prop() { return 1; }, run: () => {} }; }\nexport function install(inst) { inst.fn = () => {}; }\nfunction unusedOne() {}\n",
            )]);

            assert_eq!(dead, ["unusedOne"], "{name}");
        }
    }

    #[test]
    fn test_a_function_read_as_a_property_is_a_use_in_the_javascript_family() {
        let dead = dead_names(&[(
            "m.ts",
            "namespace Util { export const replacer = (x: any) => x; }\nexport function main() { return JSON.stringify({}, Util.replacer); }\nfunction unusedOne() {}\n",
        )]);

        assert_eq!(dead, ["unusedOne"]);
    }

    #[test]
    fn test_a_constructor_is_not_a_definition() {
        let dead = dead_names(&[(
            "m.ts",
            "class A { constructor() {} }\nexport function main() { return A; }\n",
        )]);

        assert!(dead.is_empty(), "{dead:?}");
    }

    #[test]
    fn test_a_method_of_an_exported_class_stays_a_definition() {
        let dead = dead_names(&[(
            "m.ts",
            "class Tool { private hidden(): void {} }\nexport function main(): void { new Tool(); }\n",
        )]);

        assert!(dead.contains(&"hidden".to_string()), "{dead:?}");
    }

    #[test]
    fn test_an_ambiguous_call_keeps_each_candidate_alive() {
        let dead = dead_names(&[
            ("main.py", "def main():\n    helper()\n"),
            ("a.py", "def helper():\n    pass\n"),
            ("b.py", "def helper():\n    pass\n"),
        ]);

        assert!(!dead.contains(&"helper".to_string()), "{dead:?}");
    }

    #[test]
    fn test_a_call_in_an_arrow_belongs_to_the_arrow() {
        let alive = dead_names(&[(
            "m.js",
            "export const a = () => { helper(); };\nfunction helper() {}\n",
        )]);
        let dead = dead_names(&[(
            "m.js",
            "const a = () => { helper(); };\nfunction helper() {}\n",
        )]);

        assert!(!alive.contains(&"helper".to_string()), "{alive:?}");
        assert!(dead.contains(&"helper".to_string()), "{dead:?}");
    }

    #[test]
    fn test_a_function_given_to_a_call_at_module_scope_is_alive() {
        // `export const W = memo(Internal)` reaches `Internal`: the React
        // `memo` wrapper pattern.
        let dead = dead_names(&[(
            "m.js",
            "const Internal = () => {};\nexport const W = memo(Internal);\n",
        )]);

        assert!(!dead.contains(&"Internal".to_string()), "{dead:?}");
    }

    #[test]
    fn test_leaving_a_nested_function_restores_the_enclosing_function() {
        // `after` is called by `outer`. `deep` is called only by `inner`,
        // which nothing calls.
        let dead = dead_names(&[(
            "m.js",
            "export function outer() { const inner = () => { deep(); }; after(); }\nfunction deep() {}\nfunction after() {}\n",
        )]);

        assert!(!dead.contains(&"after".to_string()), "{dead:?}");
        assert!(dead.contains(&"deep".to_string()), "{dead:?}");
    }

    #[test]
    fn test_a_declaration_without_a_body_is_not_a_definition() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("m.rs"),
            "trait Shape {\n    fn area(&self) -> u32;\n}\nstruct Square;\nimpl Shape for Square {\n    fn area(&self) -> u32 { 1 }\n}\n",
        )
        .unwrap();
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));

        let analysis = Analyzer::new().analyze(&ctx).unwrap();

        assert_eq!(analysis.summary.total_definitions, 1);
    }

    #[test]
    fn test_a_function_stored_as_a_value_is_alive_when_its_holder_is_alive() {
        let sources: [(&str, &str); 4] = [
            (
                "m.rs",
                "struct Spec { handler: fn() }\nfn used() {}\nfn unused_one() {}\nfn main() { let _ = Spec { handler: used }; }\n",
            ),
            (
                "m.go",
                "package main\ntype Spec struct{ Handler func() }\nfunc used() {}\nfunc unusedOne() {}\nfunc main() { _ = Spec{Handler: used} }\n",
            ),
            (
                "m.py",
                "def used():\n    pass\n\ndef _unused_one():\n    pass\n\ndef main():\n    table = {\"a\": used}\n    return table\n",
            ),
            (
                "m.js",
                "function used() {}\nfunction unusedOne() {}\nexport function main() { return { handler: used }; }\n",
            ),
        ];
        for (name, text) in sources {
            let dead = dead_names(&[(name, text)]);
            assert!(!dead.contains(&"used".to_string()), "{name}: {dead:?}");
            assert!(
                dead.iter()
                    .any(|found| found.to_lowercase().replace('_', "") == "unusedone"),
                "{name}: {dead:?}"
            );
        }
    }

    #[test]
    fn test_a_function_stored_as_a_value_in_dead_code_is_dead() {
        let dead = dead_names(&[(
            "m.js",
            "function used() {}\nfunction holder() { return { handler: used }; }\n",
        )]);

        assert!(dead.contains(&"used".to_string()), "{dead:?}");
        assert!(dead.contains(&"holder".to_string()), "{dead:?}");
    }

    #[test]
    fn test_the_two_definitions_of_one_name_in_one_file_are_two_definitions() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("m.ts"),
            "class A { run() {} }\nclass B { run() {} }\n",
        )
        .unwrap();
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));

        let analysis = Analyzer::new().analyze(&ctx).unwrap();

        assert_eq!(analysis.summary.total_definitions, 2);
    }

    struct EntryCase {
        language: &'static str,
        files: &'static [(&'static str, &'static str)],
        alive: &'static [&'static str],
        dead: &'static [&'static str],
    }

    const ENTRY_CASES: &[EntryCase] = &[
        EntryCase {
            language: "python fastapi",
            files: &[(
                "api.py",
                "from fastapi import FastAPI\n\napi = FastAPI()\n\n\ndef helper():\n    return 1\n\n\n@api.get(\"/items\")\nasync def read():\n    return helper()\n\n\ndef _unused_control():\n    return 2\n",
            )],
            alive: &["read", "helper"],
            dead: &["_unused_control"],
        },
        EntryCase {
            language: "python flask",
            files: &[(
                "app.py",
                "from flask import Flask\n\napp = Flask(__name__)\n\n\ndef helper():\n    return 1\n\n\n@app.route(\"/items\")\ndef show():\n    return helper()\n\n\ndef _unused_control():\n    return 2\n",
            )],
            alive: &["show", "helper"],
            dead: &["_unused_control"],
        },
        EntryCase {
            language: "javascript express",
            files: &[(
                "server.js",
                "const express = require('express');\nconst app = express();\n\nfunction helper() { return 1; }\nfunction show(req, res) { res.send(helper()); }\nfunction register() { app.get('/items', show); }\nfunction unusedControl() { return 2; }\n",
            )],
            alive: &["show", "helper"],
            dead: &["unusedControl"],
        },
        EntryCase {
            language: "typescript express",
            files: &[(
                "server.ts",
                "import express from 'express';\nconst app = express();\n\nfunction helper(): number { return 1; }\nfunction show(req: any, res: any) { res.send(helper()); }\nfunction register() { app.get('/items', show); }\nfunction unusedControl(): number { return 2; }\n",
            )],
            alive: &["show", "helper"],
            dead: &["unusedControl"],
        },
        EntryCase {
            language: "java spring",
            files: &[(
                "ItemController.java",
                "import org.springframework.web.bind.annotation.GetMapping;\nimport org.springframework.web.bind.annotation.RestController;\n\n@RestController\nclass ItemController {\n    @GetMapping(\"/items\")\n    String show() { return helper(); }\n\n    private String helper() { return \"x\"; }\n\n    private String unusedControl() { return \"y\"; }\n}\n",
            )],
            alive: &["show", "helper"],
            dead: &["unusedControl"],
        },
        EntryCase {
            language: "go net/http",
            files: &[(
                "main.go",
                "package main\n\nimport \"net/http\"\n\nfunc helper() int { return 1 }\n\nfunc show(w http.ResponseWriter, r *http.Request) { helper() }\n\nfunc register() { http.HandleFunc(\"/items\", show) }\n\nfunc unusedControl() int { return 2 }\n\nfunc main() {}\n",
            )],
            alive: &["show", "helper"],
            dead: &["unusedControl"],
        },
        EntryCase {
            language: "rust axum",
            files: &[(
                "routes.rs",
                "use axum::{routing::get, Router};\n\nfn helper() -> u32 { 1 }\n\nasync fn show() -> u32 { helper() }\n\nfn routes() -> Router { Router::new().route(\"/items\", get(show)) }\n\nfn unused_control() -> u32 { 2 }\n",
            )],
            alive: &["show", "helper"],
            dead: &["unused_control"],
        },
        EntryCase {
            language: "rust actix",
            files: &[(
                "routes.rs",
                "use actix_web::get;\n\nfn helper() -> u32 { 1 }\n\n#[get(\"/items\")]\nasync fn show() -> u32 { helper() }\n\nfn unused_control() -> u32 { 2 }\n",
            )],
            alive: &["show", "helper"],
            dead: &["unused_control"],
        },
        EntryCase {
            language: "csharp aspnet",
            files: &[(
                "ItemsController.cs",
                "using Microsoft.AspNetCore.Mvc;\n\n[ApiController]\n[Route(\"api/items\")]\nclass ItemsController : ControllerBase {\n    [HttpGet]\n    int Show() { return Helper(); }\n\n    private int Helper() { return 1; }\n\n    private int UnusedControl() { return 2; }\n}\n",
            )],
            alive: &["Show", "Helper"],
            dead: &["UnusedControl"],
        },
        EntryCase {
            language: "php laravel",
            files: &[
                (
                    "routes/web.php",
                    "<?php\nuse App\\Http\\Controllers\\ItemController;\n\nRoute::get('/items', [ItemController::class, 'show']);\n",
                ),
                (
                    "app/Http/Controllers/ItemController.php",
                    "<?php\nclass ItemController {\n    private function helper() { return 1; }\n\n    private function show() { return $this->helper(); }\n\n    private function unusedControl() { return 2; }\n}\n",
                ),
            ],
            alive: &["show", "helper"],
            dead: &["unusedControl"],
        },
        EntryCase {
            language: "ruby rails",
            files: &[
                (
                    "config/routes.rb",
                    "Rails.application.routes.draw do\n  get '/items', to: 'items#show'\nend\n",
                ),
                (
                    "app/controllers/items_controller.rb",
                    "class ItemsController < ApplicationController\n  def show\n    helper\n  end\n\n  private\n\n  def helper\n    1\n  end\n\n  def unused_control\n    2\n  end\nend\n",
                ),
            ],
            alive: &["show", "helper"],
            dead: &["unused_control"],
        },
        EntryCase {
            language: "c main",
            files: &[(
                "main.c",
                "static int helper(void) { return 1; }\n\nstatic int unused_control(void) { return 2; }\n\nint main(void) { return helper(); }\n",
            )],
            alive: &["main", "helper"],
            dead: &["unused_control"],
        },
        EntryCase {
            language: "cpp main",
            files: &[(
                "main.cpp",
                "static int helper() { return 1; }\n\nstatic int unused_control() { return 2; }\n\nint main() { return helper(); }\n",
            )],
            alive: &["main", "helper"],
            dead: &["unused_control"],
        },
        EntryCase {
            language: "bash script",
            files: &[(
                "run.sh",
                "#!/bin/bash\n\nhelper() {\n    echo 1\n}\n\n_unused_control() {\n    echo 2\n}\n\nhelper\n",
            )],
            alive: &["helper"],
            dead: &[],
        },
    ];

    #[test]
    fn test_a_configured_entry_point_is_alive_in_deadcode() {
        use crate::config::{Config, SecurityEntryConfig};
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let source = "def _helper():\n    return 1\n\n\ndef _handle_request():\n    return _helper()\n\n\ndef _unused():\n    return 2\n";
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("app.py"), source).unwrap();
        let files = FileSet::from_files(
            directory.path().to_path_buf(),
            vec![PathBuf::from("app.py")],
        );
        let dead = |config: &Config| {
            let ctx = AnalysisContext::new(&files, config, Some(directory.path()));
            let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
            result
                .items
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<String>>()
        };
        let without = dead(&Config::default());
        assert!(
            without.contains(&"_handle_request".to_string()),
            "{without:?}"
        );
        let mut config = Config::default();
        config.security.entry_points.push(SecurityEntryConfig {
            kind: "http_handler".to_string(),
            framework: "acme-web".to_string(),
            languages: vec!["python".to_string()],
            symbols: vec!["_handle_request".to_string()],
        });
        let with = dead(&config);
        assert_eq!(with, ["_unused"], "{with:?}");
    }

    #[test]
    fn test_a_serde_path_roots_only_the_function_that_it_names() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let source = "mod codec {\n    fn serialize() {}\n    fn deserialize() {}\n}\n\nmod unused {\n    fn serialize() {}\n    fn deserialize() {}\n}\n\nmod paths {\n    fn fallback() -> u32 {\n        1\n    }\n}\n\nmod other {\n    fn fallback() -> u32 {\n        2\n    }\n}\n\nstruct Holder {\n    #[serde(with = \"codec\", default = \"paths::fallback\")]\n    value: u32,\n}\n";
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("holder.rs"), source).unwrap();
        let files = FileSet::from_files(
            directory.path().to_path_buf(),
            vec![PathBuf::from("holder.rs")],
        );
        let config = Config::default();
        let ctx = AnalysisContext::new(&files, &config, Some(directory.path()));
        let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
        let mut dead: Vec<(u32, &str)> = result
            .items
            .iter()
            .filter(|item| item.kind != "class" && item.kind != "struct")
            .map(|item| (item.line, item.name.as_str()))
            .collect();
        dead.sort();
        assert_eq!(
            dead,
            [(7, "serialize"), (8, "deserialize"), (18, "fallback")],
            "only the functions that no path names"
        );
    }

    fn dead_functions_of(source: &str) -> Vec<(u32, String)> {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("holder.rs"), source).unwrap();
        let files = FileSet::from_files(
            directory.path().to_path_buf(),
            vec![PathBuf::from("holder.rs")],
        );
        let config = Config::default();
        let ctx = AnalysisContext::new(&files, &config, Some(directory.path()));
        let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
        let mut dead: Vec<(u32, String)> = result
            .items
            .iter()
            .filter(|item| item.kind == "function")
            .map(|item| (item.line, item.name.clone()))
            .collect();
        dead.sort();
        dead
    }

    fn dead_of_files(files: &[(&str, &str)]) -> (Vec<(String, u32)>, usize) {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let directory = tempfile::tempdir().unwrap();
        let mut names: Vec<PathBuf> = Vec::new();
        for (name, text) in files {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            names.push(PathBuf::from(name));
        }
        names.sort();
        let set = FileSet::from_files(directory.path().to_path_buf(), names);
        let config = Config::default();
        let ctx = AnalysisContext::new(&set, &config, Some(directory.path()));
        let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
        let mut dead: Vec<(String, u32)> = result
            .items
            .iter()
            .filter(|item| item.kind == "function")
            .map(|item| (item.file.clone(), item.line))
            .collect();
        dead.sort();
        (dead, result.summary.unresolved_paths)
    }

    #[test]
    fn test_a_serde_path_roots_each_conditional_definition_of_the_function() {
        let source = "#[cfg(feature = \"a\")]\nfn fallback() -> u32 {\n    1\n}\n\n#[cfg(not(feature = \"a\"))]\nfn fallback() -> u32 {\n    2\n}\n\nfn unused() {}\n\nstruct Holder {\n    #[serde(default = \"crate::fallback\")]\n    value: u32,\n}\n";
        let (dead, unresolved) = dead_of_files(&[("lib.rs", source)]);
        assert_eq!(dead, [("lib.rs".to_string(), 11)]);
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn test_a_serde_path_resolves_through_the_module_tree_of_other_files() {
        let holder = "mod config;\nmod unused;\n\nstruct Holder {\n    #[serde(default = \"crate::config::defaults::fallback\")]\n    value: u32,\n}\n";
        let config = "mod defaults;\n";
        let defaults = "fn fallback() -> u32 {\n    1\n}\n";
        let unused = "mod defaults {\n    fn fallback() -> u32 {\n        2\n    }\n}\n";
        let (dead, unresolved) = dead_of_files(&[
            ("lib.rs", holder),
            ("config.rs", config),
            ("config/defaults.rs", defaults),
            ("unused.rs", unused),
        ]);
        assert_eq!(dead, [("unused.rs".to_string(), 2)]);
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn test_a_glob_import_brings_the_function_that_a_serde_name_gives() {
        let root =
            "mod child;\n\nfn fallback() -> u32 {\n    1\n}\n\nfn unused() -> u32 {\n    2\n}\n";
        let child = "use super::*;\n\nstruct Holder {\n    #[serde(default = \"fallback\")]\n    value: u32,\n}\n";
        let (dead, unresolved) = dead_of_files(&[("lib.rs", root), ("child.rs", child)]);
        assert_eq!(dead, [("lib.rs".to_string(), 7)]);
        assert_eq!(unresolved, 0);
        let inline = "fn fallback() -> u32 {\n    1\n}\n\nmod inner {\n    use super::*;\n\n    struct Holder {\n        #[serde(default = \"fallback\")]\n        value: u32,\n    }\n}\n";
        let (dead, _) = dead_of_files(&[("lib.rs", inline)]);
        assert!(dead.is_empty(), "{dead:?}");
    }

    #[test]
    fn test_a_use_binding_brings_the_function_that_a_serde_path_names() {
        let root = "mod other;\nmod config;\n\nstruct Holder {\n    #[serde(default = \"defaults::fallback\")]\n    value: u32,\n}\n\nuse config::defaults;\n";
        let other = "mod defaults {\n    fn fallback() -> u32 {\n        2\n    }\n}\n";
        let config = "mod defaults;\n";
        let defaults = "fn fallback() -> u32 {\n    1\n}\n";
        let (dead, _) = dead_of_files(&[
            ("lib.rs", root),
            ("other.rs", other),
            ("config.rs", config),
            ("config/defaults.rs", defaults),
        ]);
        assert_eq!(dead, [("other.rs".to_string(), 2)]);
    }

    #[test]
    fn test_a_path_that_does_not_resolve_keeps_each_function_with_that_name_alive() {
        let root = "mod a;\nmod b;\n\nstruct Holder {\n    #[serde(default = \"missing::fallback\")]\n    value: u32,\n}\n";
        let a = "fn fallback() -> u32 {\n    1\n}\n";
        let b = "fn fallback() -> u32 {\n    2\n}\n\nfn unused() {}\n";
        let (dead, unresolved) = dead_of_files(&[("lib.rs", root), ("a.rs", a), ("b.rs", b)]);
        assert_eq!(dead, [("b.rs".to_string(), 5)]);
        assert_eq!(unresolved, 1);
    }

    #[test]
    fn test_a_crate_path_resolves_from_the_crate_root_exactly() {
        let source = "mod paths {\n    fn fallback() -> u32 {\n        1\n    }\n}\n\nmod unused {\n    mod paths {\n        fn fallback() -> u32 {\n            2\n        }\n    }\n}\n\nstruct Holder {\n    #[serde(default = \"crate::paths::fallback\")]\n    value: u32,\n}\n";
        assert_eq!(
            dead_functions_of(source),
            [(9, "fallback".to_string())],
            "`unused::paths::fallback` is not `crate::paths::fallback`"
        );
    }

    #[test]
    fn test_self_super_and_bare_paths_resolve_from_the_current_module() {
        let source = "mod paths {\n    fn fallback() -> u32 {\n        0\n    }\n}\n\nmod outer {\n    mod paths {\n        fn fallback() -> u32 {\n            1\n        }\n    }\n\n    struct Own {\n        #[serde(default = \"self::paths::fallback\")]\n        value: u32,\n    }\n\n    mod inner {\n        struct Child {\n            #[serde(default = \"super::paths::fallback\")]\n            value: u32,\n        }\n    }\n\n    struct Bare {\n        #[serde(default = \"paths::fallback\")]\n        value: u32,\n    }\n}\n";
        assert_eq!(
            dead_functions_of(source),
            [(2, "fallback".to_string())],
            "only the function of the root module is dead"
        );
    }

    #[test]
    fn test_a_function_named_in_a_serde_attribute_string_is_alive() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let source = "use serde::{Deserialize, Serialize};\n\nfn is_zero(value: &u32) -> bool {\n    *value == 0\n}\n\nfn fallback() -> u32 {\n    7\n}\n\nfn encode_count(value: &u32) -> String {\n    value.to_string()\n}\n\nfn decode_count(text: &str) -> u32 {\n    text.len() as u32\n}\n\nmod codec {\n    fn serialize(value: &u32) -> u32 {\n        *value\n    }\n\n    fn deserialize(value: u32) -> u32 {\n        value\n    }\n}\n\nfn unused_control() {}\n\n#[derive(Serialize, Deserialize)]\nstruct Holder {\n    #[serde(default = \"fallback\", skip_serializing_if = \"is_zero\")]\n    count: u32,\n    #[serde(serialize_with = \"crate::encode_count\", deserialize_with = \"decode_count\")]\n    text: u32,\n    #[serde(with = \"codec\", rename = \"unused_control\")]\n    coded: u32,\n}\n";
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("holder.rs"), source).unwrap();
        let files = FileSet::from_files(
            directory.path().to_path_buf(),
            vec![PathBuf::from("holder.rs")],
        );
        let config = Config::default();
        let ctx = AnalysisContext::new(&files, &config, Some(directory.path()));
        let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
        let reported: Vec<&str> = result.items.iter().map(|item| item.name.as_str()).collect();
        for name in [
            "is_zero",
            "fallback",
            "encode_count",
            "decode_count",
            "serialize",
            "deserialize",
        ] {
            assert!(
                !reported.contains(&name),
                "`{name}` is reported: {reported:?}"
            );
        }
        assert!(reported.contains(&"unused_control"), "{reported:?}");
    }

    #[test]
    fn test_an_entry_point_is_alive_and_its_callees_are_reachable() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};
        use std::path::PathBuf;

        let mut failures: Vec<String> = Vec::new();
        for case in ENTRY_CASES {
            let directory = tempfile::tempdir().unwrap();
            let mut names: Vec<PathBuf> = Vec::new();
            for (name, text) in case.files {
                let path = directory.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, text).unwrap();
                names.push(PathBuf::from(name));
            }
            names.sort();
            let files = FileSet::from_files(directory.path().to_path_buf(), names);
            let config = Config::default();
            let ctx = AnalysisContext::new(&files, &config, Some(directory.path()));
            let result = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
            let reported: Vec<&str> = result.items.iter().map(|item| item.name.as_str()).collect();
            for name in case.alive {
                if reported.contains(name) {
                    failures.push(format!("{}: `{name}` is reported as dead", case.language));
                }
            }
            for name in case.dead {
                if !reported.contains(name) {
                    failures.push(format!("{}: `{name}` is not reported", case.language));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The names of the reported definitions, each with its `is_test` value.
    fn dead_rows(files: &[(&str, &str)]) -> Vec<(String, bool)> {
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        let mut rows: Vec<(String, bool)> = Analyzer::new()
            .with_confidence(0.0)
            .analyze(&ctx)
            .unwrap()
            .items
            .into_iter()
            .map(|item| (item.name, item.is_test))
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn test_a_rust_test_is_never_dead_and_its_helper_is_used() {
        let source = r#"
fn main() {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    fn helper_of_a_test() {}

    fn unused_helper() {}

    #[test]
    /// The documentation is between the attribute and the function.
    fn documented() {
        helper_of_a_test();
    }

    #[test]
    // A comment is between the attribute and the function.
    fn commented() {}

    #[testlog(tokio::test)]
    async fn wrapped() {}

    #[gpui::test]
    async fn of_a_framework() {}

    #[perf]
    fn measured() {}
}
"#;
        assert_eq!(
            dead_rows(&[("src/main.rs", source)]),
            [("unused_helper".to_string(), true)]
        );
    }

    #[test]
    fn test_a_test_is_never_dead_in_each_language() {
        // The files of one case, and the names that the analysis reports for them.
        type Case = (
            &'static [(&'static str, &'static str)],
            &'static [&'static str],
        );
        let cases: [Case; 9] = [
            (
                &[(
                    "tests/test_service.py",
                    "import pytest\n\n@pytest.fixture\ndef database():\n    return _connect()\n\ndef _connect():\n    return 1\n\ndef test_runs(database):\n    pass\n\ndef _unused():\n    pass\n",
                )],
                &["_unused"],
            ),
            (
                &[(
                    "service.test.ts",
                    "function helper() {}\nfunction unusedOne() {}\ndescribe(\"service\", () => {\n  it(\"runs\", function named() { helper(); });\n});\n",
                )],
                &["unusedOne"],
            ),
            (
                &[(
                    "service_test.go",
                    "package service\n\nimport \"testing\"\n\nfunc TestRuns(t *testing.T) { helper() }\nfunc BenchmarkRuns(b *testing.B) {}\nfunc helper() {}\nfunc unusedOne() {}\n",
                )],
                &["unusedOne"],
            ),
            (
                &[(
                    "src/Checks.java",
                    "class Checks {\n    @Test\n    void runs() { helper(); }\n    @BeforeEach\n    void prepare() {}\n    private void helper() {}\n    private void unusedOne() {}\n}\n",
                )],
                &["unusedOne"],
            ),
            (
                &[(
                    "src/Checks.cs",
                    "public class Checks {\n    [Fact]\n    private void Runs() { Helper(); }\n    private void Helper() {}\n    private void UnusedOne() {}\n}\n",
                )],
                &["UnusedOne"],
            ),
            (
                &[(
                    "lib/checks.rb",
                    "class ServiceTest < Minitest::Test\n  def test_runs\n    helper\n  end\n\n  private\n\n  def helper\n  end\n\n  def unused_one\n  end\nend\n",
                )],
                &["unused_one"],
            ),
            (
                &[(
                    "src/ServiceTest.php",
                    "<?php\nclass ServiceTest extends TestCase {\n    public function testRuns(): void { $this->helper(); }\n    /** @test */\n    private function it_runs(): void {}\n    private function helper(): void {}\n    private function unusedOne(): void {}\n}\n",
                )],
                &["unusedOne"],
            ),
            (
                &[(
                    "src/checks.cc",
                    "static int helper() { return 1; }\nstatic int unused_one() { return 2; }\nTEST(Suite, Runs) {\n    helper();\n}\n",
                )],
                &["unused_one"],
            ),
            (
                &[(
                    "test/run.sh",
                    "helper() {\n  true\n}\nunused_one() {\n  true\n}\ntest_runs() {\n  helper\n}\n",
                )],
                &[],
            ),
        ];
        let mut failures = Vec::new();
        for (files, expected) in cases {
            let found = dead_names(files);
            if found != *expected {
                failures.push(format!("{}: {found:?}, not {expected:?}", files[0].0));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_a_name_that_starts_with_test_is_dead_in_production_code() {
        assert_eq!(
            dead_names(&[(
                "src/main.rs",
                "fn main() {}\nfn test_connection() {}\nfn TestThing() {}\nfn BenchmarkThing() {}\n"
            )]),
            ["BenchmarkThing", "TestThing", "test_connection"]
        );
    }

    #[test]
    fn test_rust_runtime_and_lint_attributes_keep_a_function_alive() {
        let source = r#"
fn main() {}

#[ctor(unsafe)]
unsafe fn build_classes() {
    registered();
}

fn registered() {}

#[no_mangle]
fn exported_symbol() {}

#[unsafe(no_mangle)]
fn exported_symbol_2024() {}

#[export_name = "other"]
fn exported_name() {}

extern "C" fn callback(value: u32) {}

#[allow(dead_code)]
fn kept_by_the_author() {
    used_by_kept();
}

fn used_by_kept() {}

#[expect(dead_code, reason = "the next change uses it")]
fn expected_dead() {}

#[allow(unused, reason = "retained")]
fn allowed_unused() {}

#[allow(dead_code)]
mod kept {
    fn in_kept_module() {}
}

mod inner {
    #![allow(dead_code)]
    fn under_inner_attribute() {}
}

#[allow(clippy::too_many_arguments)]
fn other_lint() {}

fn plain_dead() {}
"#;
        assert_eq!(
            dead_names(&[("src/main.rs", source)]),
            ["other_lint", "plain_dead"]
        );
    }

    #[test]
    fn test_rust_private_function_is_alive_when_a_child_module_file_calls_it() {
        let files = [
            ("src/main.rs", "pub mod db;\nfn main() {}\n"),
            (
                "src/db.rs",
                "pub mod queries;\npub struct Database;\nfn helper(value: u32) -> u32 {\n    value\n}\nfn unused_one() {}\n",
            ),
            ("src/db/queries.rs", "use super::*;\npub mod projects;\n"),
            (
                "src/db/queries/projects.rs",
                "use super::*;\n\nimpl Database {\n    pub fn run(&self, value: u32) -> u32 {\n        helper(value)\n    }\n}\n",
            ),
            (
                "src/other.rs",
                "pub fn elsewhere() -> u32 {\n    not_visible_here(1)\n}\n",
            ),
            ("src/db/tables.rs", "fn not_visible_here(value: u32) -> u32 {\n    value\n}\n"),
        ];
        assert_eq!(dead_names(&files), ["not_visible_here", "unused_one"]);
    }

    #[test]
    fn test_rust_call_without_a_receiver_in_a_method_of_the_same_name_reaches_the_function() {
        let files = [
            (
                "src/main.rs",
                "mod panel;\nfn main() {}\nfn render_button(label: &str) -> usize {\n    label.len()\n}\n",
            ),
            (
                "src/panel.rs",
                "use crate::render_button;\n\npub struct Panel;\n\nimpl Panel {\n    pub fn render_button(&self) -> usize {\n        render_button(\"push\")\n    }\n}\n",
            ),
        ];
        assert_eq!(dead_names(&files), Vec::<String>::new());
    }

    #[test]
    fn test_rust_private_function_is_alive_below_a_crate_root_with_a_different_name() {
        let files = [
            (
                "Cargo.toml",
                "[package]\nname = \"editor\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/editor.rs\"\n",
            ),
            (
                "src/editor.rs",
                "mod completions;\nmod panel;\nmod term;\nuse term::{other_mode, toggle_mode as toggle_term_mode};\npub struct Editor;\nimpl Editor {\n    pub fn toggle_mode(&self) -> u32 {\n        other_mode(toggle_term_mode(1))\n    }\n}\nfn process(value: u32) -> u32 {\n    value\n}\nfn render_button(label: &str) -> usize {\n    label.len()\n}\n",
            ),
            (
                "src/term.rs",
                "pub(super) fn toggle_mode(value: u32) -> u32 {\n    value\n}\npub(super) fn other_mode(value: u32) -> u32 {\n    value\n}\n",
            ),
            (
                "src/completions.rs",
                "use super::*;\n\nimpl Editor {\n    pub fn complete(&self) -> u32 {\n        process(1)\n    }\n}\n",
            ),
            (
                "src/panel.rs",
                "use crate::render_button;\n\npub struct Panel;\n\nimpl Panel {\n    pub fn render_button(&self) -> usize {\n        render_button(\"push\")\n    }\n}\n",
            ),
        ];
        assert_eq!(dead_names(&files), Vec::<String>::new());
    }

    #[test]
    fn test_call_of_an_import_alias_reaches_the_function() {
        let cases: [&[(&str, &str)]; 3] = [
            &[
                ("src/main.rs", "mod term;\nmod view;\nfn main() {}\n"),
                (
                    "src/term.rs",
                    "pub(super) fn toggle_mode(value: u32) -> u32 {\n    value\n}\n\npub(super) fn unused_one() {}\n",
                ),
                (
                    "src/view.rs",
                    "use crate::term::toggle_mode as toggle_term_mode;\n\npub struct View;\n\nimpl View {\n    pub fn toggle_mode(&self) -> u32 {\n        toggle_term_mode(1)\n    }\n}\n",
                ),
            ],
            &[
                (
                    "app/term.py",
                    "def _toggle_mode(value):\n    return value\n\n\ndef _unused_one():\n    return 1\n",
                ),
                (
                    "app/view.py",
                    "from app.term import _toggle_mode as toggle_term_mode\n\n\ndef main():\n    return toggle_term_mode(1)\n",
                ),
            ],
            &[
                (
                    "src/term.ts",
                    "function toggleMode(value: number): number {\n  return value;\n}\nfunction unusedOne(): number {\n  return 1;\n}\nexport { toggleMode as toggleTermMode };\n",
                ),
                (
                    "src/view.ts",
                    "import { toggleTermMode as toggle } from \"./term\";\n\nexport function main(): number {\n  return toggle(1);\n}\n",
                ),
            ],
        ];
        let mut failures = Vec::new();
        for files in cases {
            let found = dead_names(files);
            let unused: Vec<&String> = found
                .iter()
                .filter(|name| name.to_lowercase().contains("unused"))
                .collect();
            if found.len() != 1 || unused.len() != 1 {
                failures.push(format!("{}: {found:?}", files[0].0));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_rust_method_name_in_a_macro_reaches_each_method_with_the_name() {
        let files = [
            ("src/main.rs", "mod engine;\nmod file;\nfn main() {}\n"),
            (
                "src/engine.rs",
                "pub struct Engine;\n\npub fn get() -> Engine {\n    Engine\n}\n\nimpl Engine {\n    pub(super) fn write_at(&self, offset: u64) -> u64 {\n        offset\n    }\n\n    fn unused_one(&self) {}\n}\n",
            ),
            (
                "src/file.rs",
                "use crate::engine;\n\npub struct File;\n\nimpl File {\n    pub fn write_all(&self) -> u64 {\n        self.write_at(1)\n    }\n\n    fn write_at(&self, offset: u64) -> u64 {\n        observe!(Write, { engine::get().write_at(offset) })\n    }\n}\n",
            ),
        ];
        assert_eq!(dead_names(&files), ["unused_one"]);
    }

    #[test]
    fn test_ruby_symbol_names_a_method() {
        let source = "class Receive\n  def call\n    only_if(:test_request) do\n      model :nodes, :fetch_nodes\n    end\n  end\n\n  private\n\n  def test_request\n    true\n  end\n\n  def fetch_nodes\n    []\n  end\n\n  def unused_one\n    1\n  end\nend\n";
        assert_eq!(dead_names(&[("app/receive.rb", source)]), ["unused_one"]);
    }

    #[test]
    fn test_python_enum_hook_is_not_dead() {
        let source = "import enum\n\n\nclass _Version(enum.Enum):\n    V1 = \"1\"\n\n    @classmethod\n    def _missing_(cls, value):\n        return cls.V1\n\n    def _unused(self):\n        return 1\n";
        assert_eq!(dead_names(&[("app/version.py", source)]), ["_unused"]);
    }

    #[test]
    fn test_confidence_has_two_decimal_places() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("main.rs"),
            "fn main() {}\nfn unused_one() {}\n",
        )
        .unwrap();
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        let analysis = Analyzer::new().analyze(&ctx).unwrap();
        let json = serde_json::to_value(&analysis.items[0]).unwrap();
        assert_eq!(json["confidence"].to_string(), "0.95");
    }

    #[test]
    fn test_items_are_in_the_order_of_the_confidence() {
        let files = [
            ("b.rs", "fn main() {}\nfn unused_b() {}\n"),
            ("a/tests/t.rs", "fn helper_in_tests() {}\n"),
            (
                "a.rs",
                "fn unused_a() {}\nfn caller_of_dead() { dead_callee(); }\nfn dead_callee() {}\n",
            ),
        ];
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        let analysis = Analyzer::new().with_confidence(0.0).analyze(&ctx).unwrap();
        let rows: Vec<(&str, f64)> = analysis
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.confidence))
            .collect();
        assert_eq!(
            rows,
            [
                ("unused_a", 0.95),
                ("caller_of_dead", 0.95),
                ("unused_b", 0.95),
                ("dead_callee", 0.9),
                ("helper_in_tests", 0.8),
            ]
        );
    }

    #[test]
    fn test_schemars_schema_with_names_a_function() {
        let source = "fn main() {}\n\n#[derive(JsonSchema)]\nstruct Profile {\n    #[schemars(schema_with = \"tools_schema\")]\n    tools: u32,\n}\n\nfn tools_schema() {}\n\nfn not_named() {}\n";
        assert_eq!(dead_names(&[("src/main.rs", source)]), ["not_named"]);
    }

    #[test]
    fn test_python_dunder_methods_and_stub_files_are_not_dead() {
        let files = [
            (
                "app/model.py",
                "class _Size:\n    def __eq__(self, other):\n        return self._same(other)\n\n    def __repr__(self):\n        return \"size\"\n\n    def _same(self, other):\n        return True\n\n    def _unused(self):\n        return False\n\n    def __private(self):\n        return 1\n",
            ),
            (
                "stubs/config.pyi",
                "class _Setting:\n    def __get__(self, instance, owner): ...\n    def _helper(self): ...\n",
            ),
        ];
        assert_eq!(dead_names(&files), ["__private", "_unused"]);
    }
}

#[cfg(test)]
mod go_tests {
    use super::*;

    /// The time of `go_interface_methods` for `size` types and `size`
    /// interfaces that all have the method `Close`. Type `i` has the second
    /// method of interface `i` only, thus it implements one interface.
    fn interface_methods_found_at(size: usize) -> std::time::Duration {
        let mut source = String::from("package shop\n");
        for position in 0..size {
            source.push_str(&format!(
                "\ntype I{position} interface {{\n\tClose()\n\tM{position}()\n}}\n\ntype T{position} struct{{}}\n\nfunc (t T{position}) Close() {{}}\n\nfunc (t T{position}) M{position}() {{}}\n"
            ));
        }
        let files = [
            ("go.mod", "module example.com/shop\n"),
            ("shop/shop.go", source.as_str()),
        ];
        let (facts, _) = crate::index::imports::testing::index_with_manifests(&files);
        let started = std::time::Instant::now();
        let found = go_interface_methods(&facts);
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size * 2);
        elapsed
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_interface_methods_of_many_types_are_found_in_linear_time() {
        crate::index::timing::assert_scales_linearly("types", 4_000, interface_methods_found_at);
    }
    use crate::config::Config;
    use crate::core::FileSet;

    fn dead_names(files: &[(&str, &str)]) -> Vec<String> {
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        let config = Config::default();
        let file_set = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()));
        let mut names: Vec<String> = Analyzer::new()
            .with_confidence(0.0)
            .analyze(&ctx)
            .unwrap()
            .items
            .into_iter()
            .map(|item| format!("{}:{}:{}", item.file, item.line, item.name))
            .collect();
        names.sort();
        names
    }

    /// A Go file can call a function that is not exported in each file of
    /// its package. Two files with build constraints can each define it.
    #[test]
    fn test_unexported_function_of_a_different_file_of_the_package_is_alive() {
        let dead = dead_names(&[
            ("go.mod", "module example.com/shop\n"),
            (
                "process/command.go",
                "package process\n\nfunc Start() {\n\tsetAttributes()\n}\n",
            ),
            (
                "process/command_unix.go",
                "//go:build unix\n\npackage process\n\nfunc setAttributes() {}\n\nfunc unusedUnix() {}\n",
            ),
            (
                "process/command_windows.go",
                "package process\n\nfunc setAttributes() {}\n",
            ),
        ]);
        assert_eq!(dead, ["process/command_unix.go:7:unusedUnix"]);
    }

    /// A method that a type needs to implement an interface of the
    /// repository is alive, also when no call names it.
    #[test]
    fn test_method_that_implements_an_interface_is_alive() {
        let dead = dead_names(&[
            ("go.mod", "module example.com/shop\n"),
            (
                "check/check.go",
                "package check\n\ntype Check interface {\n\tinternalOnly()\n}\n\ntype Prefix string\n\nfunc (Prefix) internalOnly() {}\n\nfunc (Prefix) unused() {}\n\nfunc Run(check Check) {}\n",
            ),
        ]);
        assert_eq!(dead, ["check/check.go:11:unused"]);
    }
}
