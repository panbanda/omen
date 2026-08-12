//! Shared import-specifier resolver used by both `graph` and `smells`.
//!
//! Both analyzers build a dependency graph from the same parsed imports, and
//! it is critical that they resolve specifiers to on-disk files identically -
//! otherwise the two commands can disagree on edges and therefore on cycles
//! for the same input (GitHub issue #479, where `smells` fabricated a
//! self-cycle that `graph` correctly did not report). This module is the one
//! resolver both call, so they can never diverge again.
//!
//! Relative specifiers (`./foo`, `../foo`) are resolved strictly against the
//! importing file's directory: every candidate extension is tried, but if
//! none of them exist on disk the specifier fails to resolve. It never falls
//! back to matching some other file elsewhere in the repo that happens to
//! share a file stem -- that fallback is what produced phantom cycles.
//! Non-relative specifiers (Rust `crate::`/module paths, Ruby constants, Go
//! import paths, ...) still use stem/segment heuristics, since they have no
//! directory to resolve against.

use std::collections::HashMap;
use std::path::Path;

use petgraph::graph::{DiGraph, NodeIndex};
use rayon::prelude::*;

use crate::core::{AnalysisContext, Language};
use crate::parser::{extract_imports, ImportKind, Parser};

/// Extensions tried, in priority order, once a relative specifier's own
/// (possibly absent) extension has been stripped. Used for plain `.js`,
/// `.jsx`, and extensionless specifiers.
const JS_TS_SOURCE_EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mts", ".cts"];

/// An `.mjs` specifier is Node's explicit ESM marker, so its own `.mts`
/// source (the TypeScript equivalent) is tried before the extensionless
/// `.ts`/`.js` -- otherwise a directory with both `foo.ts` and `foo.mts`
/// would resolve an `.mjs` import to the wrong module-system source.
const MJS_SOURCE_EXTS: &[&str] = &[".mts", ".ts", ".tsx", ".js", ".jsx", ".cts"];

/// Symmetric with `MJS_SOURCE_EXTS` for CommonJS.
const CJS_SOURCE_EXTS: &[&str] = &[".cts", ".ts", ".tsx", ".js", ".jsx", ".mts"];

/// Extensions for other supported languages, tried after JS/TS so that a
/// relative import without a recognized JS/TS extension can still resolve.
const OTHER_SOURCE_EXTS: &[&str] = &[
    ".rs", ".go", ".py", ".java", ".rb", ".php", ".c", ".h", ".cpp", ".hpp", ".cs",
];

/// Pre-built index for O(1) file path lookups during import resolution.
pub struct ImportIndex {
    /// Full relative path -> relative path (identity mapping for exact matches).
    by_full_path: HashMap<String, String>,
    /// File stem (without extension) -> list of relative paths.
    by_stem: HashMap<String, Vec<String>>,
}

impl ImportIndex {
    pub fn new(files: &[std::path::PathBuf], root: &Path) -> Self {
        let mut by_full_path = HashMap::with_capacity(files.len());
        let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();

        for file in files {
            let rel = file.strip_prefix(root).unwrap_or(file);
            let rel_str = rel.to_string_lossy().to_string();

            by_full_path.insert(rel_str.clone(), rel_str.clone());

            if let Some(stem) = rel.file_stem() {
                let stem_str = stem.to_string_lossy().to_string();
                by_stem.entry(stem_str).or_default().push(rel_str.clone());
            }
        }

        Self {
            by_full_path,
            by_stem,
        }
    }

    /// Try to find a file matching the import path.
    pub fn find_match(&self, import_path: &str, from_file: &Path) -> Option<String> {
        // 0. Handle Rust crate-relative imports (crate::, super::, self::)
        if import_path.starts_with("crate::")
            || import_path.starts_with("super::")
            || import_path.starts_with("self::")
        {
            if let Some(found) = self.find_rust_crate_relative(import_path, from_file) {
                return Some(found);
            }
            // Fall through to other matching strategies below.
        }

        // 1. Relative imports (./foo, ../foo) resolve strictly against the
        //    importing file's directory. No fallback to global stem matching:
        //    a relative specifier names one specific file, and picking a
        //    different same-named file elsewhere is exactly what produced
        //    phantom cycles (issue #479).
        if import_path.starts_with("./") || import_path.starts_with("../") {
            let parent = from_file.parent()?;
            let resolved = parent.join(import_path);
            // `normalize_path` returns None if the specifier traverses
            // above the analyzed root (more `..` than preceding path
            // components) -- such a specifier points outside the repo and
            // must not be clamped into some unrelated in-repo file.
            let normalized = normalize_path(&resolved)?;
            return self.resolve_relative(&normalized);
        }

        // 2. Try exact stem match
        if let Some(matches) = self.by_stem.get(import_path) {
            if let Some(found) = shortest(matches) {
                return Some(found);
            }
        }

        // 3. Try snake_case conversion for Ruby CamelCase constants
        //    e.g., "OrderSearcher" -> "order_searcher", "ActiveModel::Validations" -> "active_model/validations"
        if import_path.contains("::") {
            // Scoped constant: split on ::, convert each segment, join with /
            // e.g., ActiveModel::Validations -> active_model/validations
            let snake_path: String = import_path
                .split("::")
                .map(camel_to_snake)
                .collect::<Vec<_>>()
                .join("/");
            // Match on path segment boundaries (must be preceded by / or start of string).
            // `HashMap::keys()` iteration order is randomized per-instance
            // (a fresh random hasher seed each `HashMap::new()`), so two
            // `ImportIndex`es built from identical files -- as `graph` and
            // `smells` each build their own -- could pick different targets
            // for the same specifier. Collect every match and pick the
            // shortest (most specific), tie-broken lexicographically, so
            // the result is stable regardless of hash iteration order.
            let with_slash = format!("/{}", snake_path);
            let mut candidates: Vec<&String> = self
                .by_full_path
                .keys()
                .filter(|k| k.starts_with(&snake_path) || k.contains(&with_slash))
                .collect();
            candidates.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            if let Some(matched) = candidates.first() {
                return Some((*matched).clone());
            }
            // Also try the last segment as a stem match
            if let Some(last) = snake_path.rsplit('/').next() {
                if let Some(matches) = self.by_stem.get(last) {
                    for candidate in matches {
                        if candidate.contains(&snake_path) {
                            return Some(candidate.clone());
                        }
                    }
                }
            }
        } else {
            let snake = camel_to_snake(import_path);
            if snake != import_path {
                if let Some(matches) = self.by_stem.get(&snake) {
                    if let Some(found) = shortest(matches) {
                        return Some(found);
                    }
                }
            }
        }

        // 4. Try segment-based matching for module paths like "utils/helpers"
        let segments: Vec<&str> = import_path.split('/').filter(|s| !s.is_empty()).collect();
        if let Some(last_segment) = segments.last() {
            if let Some(candidates) = self.by_stem.get(*last_segment) {
                // Find candidates that match the full path pattern
                for candidate in candidates {
                    if candidate.contains(import_path) {
                        return Some(candidate.clone());
                    }
                }
                // Fall back to first match with the stem
                if !candidates.is_empty() {
                    return Some(candidates[0].clone());
                }
            }
        }

        None
    }

    fn find_rust_crate_relative(&self, import_path: &str, from_file: &Path) -> Option<String> {
        let stripped = import_path
            .strip_prefix("crate::")
            .or_else(|| import_path.strip_prefix("super::"))
            .or_else(|| import_path.strip_prefix("self::"))
            .unwrap_or(import_path);

        // Take the module path part (strip trailing type names which are CamelCase)
        // e.g., crate::config::Config -> config, crate::analyzers::graph -> analyzers/graph
        let segments: Vec<&str> = stripped.split("::").collect();

        // Find the last segment that looks like a module (lowercase/snake_case)
        let module_segments: Vec<&str> = segments
            .iter()
            .take_while(|s| {
                s.chars()
                    .next()
                    .map(|c| c.is_lowercase() || c == '_')
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        let module_path = if module_segments.is_empty() {
            // All segments start with uppercase - use all as path
            segments.join("/")
        } else {
            module_segments.join("/")
        };

        // Handle super:: relative to current file. `mod.rs`/`lib.rs`/
        // `main.rs` stand in for their OWN directory (that directory IS the
        // module they define), so `super::` -- the module's parent -- is one
        // level further up than that. Every other file (e.g. `src/a/b.rs`,
        // module `crate::a::b`) already lives in its own module's directory
        // (`src/a/`), so `super::` is just one level up from the file, not
        // two -- using two levels for a plain leaf module file resolves
        // `super::x` against the wrong (grandparent) directory and drops or
        // mis-resolves the edge.
        if import_path.starts_with("super::") {
            let is_directory_stand_in = from_file
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == "mod.rs" || n == "lib.rs" || n == "main.rs");
            let super_base = if is_directory_stand_in {
                from_file.parent().and_then(|p| p.parent())
            } else {
                from_file.parent()
            };

            if let Some(parent) = super_base {
                let resolved = parent.join(&module_path);
                if let Some(normalized) = normalize_path(&resolved) {
                    for ext in &["", ".rs"] {
                        let candidate = if ext.is_empty() {
                            normalized.clone()
                        } else {
                            format!("{}{}", normalized, ext)
                        };
                        if self.by_full_path.contains_key(&candidate) {
                            return Some(candidate);
                        }
                    }
                    let mod_rs = format!("{}/mod.rs", normalized);
                    if self.by_full_path.contains_key(&mod_rs) {
                        return Some(mod_rs);
                    }
                }
            }
        }

        // Try common Rust source roots with the module path, then
        // progressively shorter prefixes of it (dropping trailing
        // segments). `crate::a::b` may name a submodule file `a/b.rs`, or
        // it may name an item (`fn`/`struct`/`const`) defined directly in
        // `a.rs` -- syntactically indistinguishable from here. Falling back
        // to the parent path when the full path doesn't match a file is
        // what makes the latter resolve to `a.rs` instead of silently
        // dropping the dependency (which hides real cycles).
        let path_segments: Vec<&str> = module_path.split('/').filter(|s| !s.is_empty()).collect();
        for take in (1..=path_segments.len()).rev() {
            let candidate_path = path_segments[..take].join("/");
            let source_roots = ["src", "lib", ""];
            for root in &source_roots {
                let base = if root.is_empty() {
                    candidate_path.clone()
                } else {
                    format!("{}/{}", root, candidate_path)
                };

                // Try direct file match (e.g., src/config.rs)
                let rs_path = format!("{}.rs", base);
                if self.by_full_path.contains_key(&rs_path) {
                    return Some(rs_path);
                }

                // Try mod.rs (e.g., src/config/mod.rs)
                let mod_path = format!("{}/mod.rs", base);
                if self.by_full_path.contains_key(&mod_path) {
                    return Some(mod_path);
                }
            }
        }

        None
    }

    /// Resolve a relative specifier (already joined onto the importer's
    /// directory and normalized) against the file index. Tries the exact
    /// path, then extension substitution (with JS/TS compiled-output
    /// awareness), then `index.*` barrel files -- and nothing else.
    fn resolve_relative(&self, normalized: &str) -> Option<String> {
        if self.by_full_path.contains_key(normalized) {
            return Some(normalized.to_string());
        }

        // Pick the source-extension priority list based on the specifier's
        // own (stripped) extension, so an explicit `.mjs`/`.cjs` marker
        // prefers its matching `.mts`/`.cts` module-system source over a
        // plain `.ts` sibling.
        let (stem, source_exts): (&str, &[&str]) = if let Some(s) = normalized.strip_suffix(".mjs")
        {
            (s, MJS_SOURCE_EXTS)
        } else if let Some(s) = normalized.strip_suffix(".cjs") {
            (s, CJS_SOURCE_EXTS)
        } else if let Some(s) = normalized.strip_suffix(".js") {
            (s, JS_TS_SOURCE_EXTS)
        } else if let Some(s) = normalized.strip_suffix(".jsx") {
            (s, JS_TS_SOURCE_EXTS)
        } else {
            (normalized, JS_TS_SOURCE_EXTS)
        };

        for ext in source_exts.iter().chain(OTHER_SOURCE_EXTS) {
            let candidate = format!("{stem}{ext}");
            if self.by_full_path.contains_key(&candidate) {
                return Some(candidate);
            }
        }

        for ext in source_exts.iter().chain(OTHER_SOURCE_EXTS) {
            let candidate = format!("{stem}/index{ext}");
            if self.by_full_path.contains_key(&candidate) {
                return Some(candidate);
            }
        }

        None
    }
}

/// Prefer the shortest (most specific) path among stem matches.
fn shortest(matches: &[String]) -> Option<String> {
    matches.iter().min_by_key(|s| s.len()).cloned()
}

/// Convert CamelCase to snake_case for Ruby constant-to-filename resolution.
/// Handles consecutive uppercase (e.g., HTTPClient -> http_client).
pub fn camel_to_snake(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
                // Insert underscore before: uppercase after lowercase, or
                // start of new word in consecutive uppercase (e.g., the P in HTTPParser)
                if prev.is_lowercase() || (prev.is_uppercase() && next_is_lower) {
                    result.push('_');
                }
            }
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Normalize a path by removing `.` and resolving `..`. Returns `None` if
/// the path traverses above its own starting point (more `..` components
/// than preceding path segments) -- such a specifier points outside the
/// directory it was resolved against and must not be silently clamped into
/// some unrelated in-repo file.
pub fn normalize_path(path: &Path) -> Option<String> {
    let mut components: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(c) => {
                components.push(c.to_string_lossy().to_string());
            }
            std::path::Component::ParentDir => {
                components.pop()?;
            }
            std::path::Component::CurDir => {}
            _ => {}
        }
    }
    Some(components.join("/"))
}

/// Parse every file's imports (in parallel) and resolve each through the
/// shared `ImportIndex`. Module containment declarations (Rust `mod foo;`)
/// are not dependency edges, only real `use`/`import` statements are, so
/// they are filtered out here. An import that fails to resolve produces no
/// edge, unless `include_external` requests a node for it anyway.
///
/// Returns `(path, resolved_targets)` per file. This extraction step -- not
/// just the resolver it calls -- is shared by `graph` and `smells`, so the
/// two analyzers can never disagree on the edges they build for the same
/// input (issue #479).
pub fn extract_resolved_imports(
    files: &[std::path::PathBuf],
    ctx: &AnalysisContext<'_>,
    file_index: &ImportIndex,
    include_external: bool,
) -> Vec<(String, Vec<String>)> {
    files
        .par_iter()
        .filter_map(|file| {
            let rel_path = file.strip_prefix(ctx.root).unwrap_or(file);
            let path_str = rel_path.to_string_lossy().to_string();

            let content = ctx.read_file(file).ok()?;
            let lang = Language::detect(file)?;

            let parser = Parser::new();
            let result = parser.parse(&content, lang, file).ok()?;

            let resolved: Vec<String> = extract_imports(&result)
                .into_iter()
                .filter(|imp| imp.kind == ImportKind::Use)
                .filter_map(|imp| {
                    file_index.find_match(&imp.path, rel_path).or_else(|| {
                        if include_external {
                            Some(imp.path.clone())
                        } else {
                            None
                        }
                    })
                })
                .collect();

            Some((path_str, resolved))
        })
        .collect()
}

/// Build a dependency graph from resolved `(path, imports)` pairs, applying
/// the shared edge rules: a file is never wired to itself (a self-import is
/// not a real dependency cycle between two components) and a repeated
/// import produces one edge, not a parallel edge. `include_external` gives
/// unresolved import targets their own node instead of dropping them.
///
/// Sharing this step (not just the resolver) is what guarantees `graph` and
/// `smells` build the identical edge set for the same input.
pub fn build_dependency_graph(
    file_imports: &[(String, Vec<String>)],
    include_external: bool,
) -> (DiGraph<String, ()>, HashMap<String, NodeIndex>) {
    let mut graph: DiGraph<String, ()> =
        DiGraph::with_capacity(file_imports.len(), file_imports.len() * 4);
    let mut node_indices: HashMap<String, NodeIndex> = HashMap::with_capacity(file_imports.len());

    for (path, _) in file_imports {
        node_indices
            .entry(path.clone())
            .or_insert_with(|| graph.add_node(path.clone()));
    }

    for (from_path, imports) in file_imports {
        let from_idx = node_indices[from_path];

        for import in imports {
            let to_idx = if let Some(&idx) = node_indices.get(import) {
                idx
            } else if include_external {
                *node_indices
                    .entry(import.clone())
                    .or_insert_with(|| graph.add_node(import.clone()))
            } else {
                continue;
            };

            if from_idx != to_idx && !graph.contains_edge(from_idx, to_idx) {
                graph.add_edge(from_idx, to_idx, ());
            }
        }
    }

    (graph, node_indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("OrderSearcher"), "order_searcher");
        assert_eq!(camel_to_snake("ApplicationRecord"), "application_record");
        assert_eq!(camel_to_snake("HTTPClient"), "http_client");
        assert_eq!(camel_to_snake("Foo"), "foo");
        assert_eq!(camel_to_snake("FooBar"), "foo_bar");
        assert_eq!(camel_to_snake("already_snake"), "already_snake");
        assert_eq!(camel_to_snake("JSON"), "json");
        assert_eq!(camel_to_snake("HTMLParser"), "html_parser");
    }

    #[test]
    fn test_find_match_ruby_constant_to_snake_case() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/app/models/concerns/order_searcher.rb"),
            std::path::PathBuf::from("/project/app/models/concerns/feature_gate.rb"),
            std::path::PathBuf::from("/project/app/models/application_record.rb"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("OrderSearcher", Path::new("app/models/order.rb")),
            Some("app/models/concerns/order_searcher.rb".to_string())
        );
        assert_eq!(
            index.find_match("FeatureGate", Path::new("app/models/order.rb")),
            Some("app/models/concerns/feature_gate.rb".to_string())
        );
        assert_eq!(
            index.find_match("ApplicationRecord", Path::new("app/models/order.rb")),
            Some("app/models/application_record.rb".to_string())
        );
    }

    #[test]
    fn test_find_match_ruby_scoped_constant() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/app/models/concerns/active_model/validations.rb"),
            std::path::PathBuf::from("/project/lib/active_record/base.rb"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("ActiveModel::Validations", Path::new("app/models/user.rb")),
            Some("app/models/concerns/active_model/validations.rb".to_string())
        );
        assert_eq!(
            index.find_match("ActiveRecord::Base", Path::new("app/models/user.rb")),
            Some("lib/active_record/base.rb".to_string())
        );
    }

    #[test]
    fn test_find_match_scoped_constant_no_substring_collision() {
        let root = Path::new("/project");
        let files = vec![std::path::PathBuf::from(
            "/project/packages/connect/app/controllers/connect/sign_up_controller.rb",
        )];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("T::Sig", Path::new("app/models/user.rb")),
            None
        );
    }

    #[test]
    fn test_find_match_rust_crate_import() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/config/mod.rs"),
            std::path::PathBuf::from("/project/src/utils.rs"),
            std::path::PathBuf::from("/project/src/analyzers/graph.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("crate::config", Path::new("src/main.rs")),
            Some("src/config/mod.rs".to_string())
        );
        assert_eq!(
            index.find_match("crate::utils", Path::new("src/main.rs")),
            Some("src/utils.rs".to_string())
        );
        assert_eq!(
            index.find_match("crate::analyzers::graph", Path::new("src/main.rs")),
            Some("src/analyzers/graph.rs".to_string())
        );
    }

    #[test]
    fn test_find_match_rust_mod_declaration() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/config.rs"),
            std::path::PathBuf::from("/project/src/config/mod.rs"),
            std::path::PathBuf::from("/project/src/utils.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        let result = index.find_match("config", Path::new("src/lib.rs"));
        assert!(
            result == Some("src/config.rs".to_string())
                || result == Some("src/config/mod.rs".to_string()),
            "Expected config.rs or config/mod.rs, got {:?}",
            result
        );

        assert_eq!(
            index.find_match("utils", Path::new("src/lib.rs")),
            Some("src/utils.rs".to_string())
        );
    }

    #[test]
    fn test_relative_esm_js_specifier_resolves_to_ts_source() {
        // Issue #479 repro: an ESM `.js` specifier pointing at a `.ts` source
        // file located via a relative path, not a global stem match.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/packages/brain/src/types.ts"),
            std::path::PathBuf::from("/project/packages/mcp/src/types.ts"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match(
                "../../mcp/src/types.js",
                Path::new("packages/brain/src/types.ts")
            ),
            Some("packages/mcp/src/types.ts".to_string())
        );
    }

    #[test]
    fn test_relative_import_never_falls_back_to_global_stem() {
        // A relative specifier that does not actually resolve to any file on
        // disk must fail, even though a same-named file exists elsewhere in
        // the repo. Falling back to that other file is exactly the phantom
        // self-cycle bug from issue #479.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/a/types.ts"),
            std::path::PathBuf::from("/project/b/types.ts"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("./nonexistent.js", Path::new("a/types.ts")),
            None
        );
    }

    #[test]
    fn test_relative_import_resolves_index_barrel() {
        let root = Path::new("/project");
        let files = vec![std::path::PathBuf::from("/project/src/utils/index.ts")];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("./utils", Path::new("src/main.ts")),
            Some("src/utils/index.ts".to_string())
        );
    }

    #[test]
    fn test_rust_crate_path_to_item_falls_back_to_parent_module_file() {
        // `use crate::a::{b};` where `b` is a value/fn/type defined directly
        // in a.rs (not a submodule) must still resolve to a.rs. Regression:
        // resolving only the full `a/b` path and giving up when `b.rs`
        // doesn't exist silently drops a real dependency edge, which hides
        // real cycles (e.g. a.rs <-> c.rs).
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/a.rs"),
            std::path::PathBuf::from("/project/src/c.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("crate::a::b", Path::new("src/c.rs")),
            Some("src/a.rs".to_string())
        );
    }

    #[test]
    fn test_rust_crate_path_to_item_falls_back_through_multiple_levels() {
        // crate::analyzers::graph::helper_fn -- "helper_fn" is an item in
        // graph.rs, not a submodule of graph. Must fall back past both
        // "analyzers/graph/helper_fn" and land on "analyzers/graph.rs".
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/analyzers/graph.rs"),
            std::path::PathBuf::from("/project/src/analyzers/mod.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match(
                "crate::analyzers::graph::helper_fn",
                Path::new("src/main.rs")
            ),
            Some("src/analyzers/graph.rs".to_string())
        );
    }

    #[test]
    fn test_scoped_constant_match_is_deterministic_across_instances() {
        // The scoped-constant matcher (Ruby-style `Foo::Bar`) used to pick
        // an arbitrary `HashMap::keys()` match via `.find()`, whose
        // iteration order is randomized per-`HashMap` instance (a fresh
        // random hasher seed per `HashMap::new()`) -- so two `ImportIndex`es
        // built from identical files, as `graph` and `smells` each build
        // their own, could pick different targets for the same specifier in
        // the same run. Two same-length, same-prefix candidates make the
        // old code's pick depend on hash iteration order; build many
        // independent instances and require every one to agree.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/reports/generator_aaa.rb"),
            std::path::PathBuf::from("/project/reports/generator_bbb.rb"),
        ];

        let mut picks = std::collections::HashSet::new();
        for _ in 0..50 {
            let index = ImportIndex::new(&files, root);
            let result = index.find_match("Reports::Generator", Path::new("app/user.rb"));
            picks.insert(result);
        }

        assert_eq!(
            picks.len(),
            1,
            "expected a single stable pick across instances, got {:?}",
            picks
        );
        assert_eq!(
            picks.into_iter().next().unwrap(),
            Some("reports/generator_aaa.rb".to_string()),
            "expected a deterministic (e.g. lexicographically first) pick"
        );
    }

    #[test]
    fn test_mjs_specifier_prefers_mts_over_ts() {
        // main.mts importing `./foo.mjs` with both foo.ts and foo.mts
        // present must resolve to the `.mts` sibling (the matching
        // module-system source), not `.ts`.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/foo.ts"),
            std::path::PathBuf::from("/project/src/foo.mts"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("./foo.mjs", Path::new("src/main.mts")),
            Some("src/foo.mts".to_string())
        );
    }

    #[test]
    fn test_cjs_specifier_prefers_cts_over_ts() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/foo.ts"),
            std::path::PathBuf::from("/project/src/foo.cts"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("./foo.cjs", Path::new("src/main.cts")),
            Some("src/foo.cts".to_string())
        );
    }

    #[test]
    fn test_relative_import_above_root_does_not_resolve() {
        // `../../foo.js` from `src/main.ts` traverses above the analyzed
        // root. It must not be silently clamped into a root-level file --
        // that fabricates an edge to a file the specifier cannot actually
        // reach.
        let root = Path::new("/project");
        let files = vec![std::path::PathBuf::from("/project/foo.ts")];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("../../foo.js", Path::new("src/main.ts")),
            None
        );
    }

    #[test]
    fn test_build_dependency_graph_rejects_self_loops_and_dedups() {
        let file_imports = vec![
            // a.ts "imports itself" and imports b.ts twice.
            (
                "a.ts".to_string(),
                vec!["a.ts".to_string(), "b.ts".to_string(), "b.ts".to_string()],
            ),
            ("b.ts".to_string(), vec![]),
        ];

        let (graph, node_indices) = build_dependency_graph(&file_imports, false);

        let a = node_indices["a.ts"];
        let b = node_indices["b.ts"];
        assert!(!graph.contains_edge(a, a), "self-loop must be rejected");
        assert_eq!(
            graph.edges_connecting(a, b).count(),
            1,
            "repeated import must produce exactly one edge, not a parallel edge"
        );
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_super_resolution_from_mod_rs_file() {
        // `super::x` from `src/a/mod.rs` (module `crate::a`) means
        // `crate::x` -- a's own directory `src/a` IS module a's directory,
        // so its parent module's directory is one level further up, `src`.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/a/mod.rs"),
            std::path::PathBuf::from("/project/src/x.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("super::x", Path::new("src/a/mod.rs")),
            Some("src/x.rs".to_string())
        );
    }

    #[test]
    fn test_super_resolution_from_nested_non_mod_rs_file() {
        // `super::x` from `src/a/b.rs` (module `crate::a::b`) means
        // `crate::a::x` -- a sibling of `b` within module `a`, which lives
        // in a's directory `src/a/`. `b.rs` is a leaf file, not a directory
        // stand-in like `mod.rs`, so its OWN directory (one level up from
        // the file) is the `super::` base -- not two levels up.
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/a/b.rs"),
            std::path::PathBuf::from("/project/src/a/x.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("super::x", Path::new("src/a/b.rs")),
            Some("src/a/x.rs".to_string())
        );
    }

    #[test]
    fn test_super_resolution_from_lib_rs_and_main_rs() {
        let root = Path::new("/project");
        let files = vec![
            std::path::PathBuf::from("/project/src/a/lib.rs"),
            std::path::PathBuf::from("/project/src/a/main.rs"),
            std::path::PathBuf::from("/project/src/x.rs"),
        ];
        let index = ImportIndex::new(&files, root);

        assert_eq!(
            index.find_match("super::x", Path::new("src/a/lib.rs")),
            Some("src/x.rs".to_string())
        );
        assert_eq!(
            index.find_match("super::x", Path::new("src/a/main.rs")),
            Some("src/x.rs".to_string())
        );
    }
}
