//! The handler that a registration names with a path, as `get(routes::list)` or
//! `views.detail` names it.
//!
//! The index gives the facts: the module declarations and the imports with their
//! resolved files, and the container path of each symbol. A path resolves to a
//! symbol only when the facts select exactly one function or method. The last name
//! of the path alone never selects a symbol.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{syntax, Context};
use crate::core::Language;
use crate::index::extract::FileSymbols;
use crate::index::model::{ImportKind, ImportTarget};

/// The facts of the index that the resolution of a handler path reads. One analysis
/// builds them one time.
#[derive(Default)]
pub(super) struct HandlerPaths {
    by_file: HashMap<String, usize>,
    /// The file of a Rust module, by the file that declares it and its name.
    modules: HashMap<(usize, String), usize>,
    /// Each module file with the name that a declaration gives it.
    module_names: HashSet<(usize, String)>,
    /// The file that declares a Rust module file. A file that two files declare has no entry.
    parents: HashMap<usize, usize>,
    /// The positions of the functions, the methods, and the constructors, by the file and the
    /// container path with the name, joined with `.`.
    callables: HashMap<(usize, String), Vec<usize>>,
}

impl HandlerPaths {
    pub(super) fn new(files: &[FileSymbols]) -> Self {
        let by_file: HashMap<String, usize> = files
            .iter()
            .enumerate()
            .map(|(position, file)| (file.file.clone(), position))
            .collect();
        let mut modules = HashMap::new();
        let mut module_names = HashSet::new();
        let mut declared_by: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut callables: HashMap<(usize, String), Vec<usize>> = HashMap::new();
        for (position, file) in files.iter().enumerate() {
            for binding in &file.imports {
                let (ImportKind::ModuleDeclaration, Some(name), ImportTarget::Resolved(child)) =
                    (binding.kind, &binding.local_name, &binding.target_file)
                else {
                    continue;
                };
                if let Some(child) = by_file.get(child) {
                    modules.insert((position, name.clone()), *child);
                    module_names.insert((*child, name.clone()));
                    declared_by.entry(*child).or_default().push(position);
                }
            }
            for (index, symbol) in file.symbols.iter().enumerate() {
                if !symbol.is_callable() {
                    continue;
                }
                let mut path = file
                    .link_facts
                    .get(index)
                    .map(|facts| facts.container_path.join("."))
                    .unwrap_or_default();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(&symbol.name);
                callables.entry((position, path)).or_default().push(index);
            }
        }
        let parents = declared_by
            .into_iter()
            .filter_map(|(child, mut parents)| {
                parents.sort_unstable();
                parents.dedup();
                (parents.len() == 1).then(|| (child, parents[0]))
            })
            .collect();
        Self {
            by_file,
            modules,
            module_names,
            parents,
            callables,
        }
    }

    /// The root module of the crate of `file`.
    fn crate_root(&self, file: usize) -> usize {
        let mut current = file;
        // A cycle of declarations is not valid Rust. The bound stops the walk on one.
        for _ in 0..=self.parents.len() {
            match self.parents.get(&current) {
                Some(parent) => current = *parent,
                None => break,
            }
        }
        current
    }

    /// The one callable at the path in the file, after the modules that the path names.
    fn callable(&self, files: &[FileSymbols], module: usize, path: &[&str]) -> Option<String> {
        let mut module = module;
        let mut rest = path;
        while let [first, tail @ ..] = rest {
            if tail.is_empty() {
                break;
            }
            match self.modules.get(&(module, first.to_string())) {
                Some(child) => {
                    module = *child;
                    rest = tail;
                }
                None => break,
            }
        }
        match self
            .callables
            .get(&(module, rest.join(".")))
            .map(Vec::as_slice)
        {
            Some([position]) => Some(files[module].symbols[*position].key.to_string()),
            _ => None,
        }
    }
}

/// The names of a path, without generic arguments. `None` means a path that the
/// resolution does not read, as a qualified path with a type in angle brackets.
fn path_segments(text: &str, separator: &str) -> Option<Vec<String>> {
    let compact: String = text.split_whitespace().collect();
    let without_generics = compact.split("::<").next().unwrap_or("");
    if without_generics.contains(['<', '>', '(', ')']) {
        return None;
    }
    let segments: Vec<String> = without_generics
        .split(separator)
        .map(str::to_string)
        .collect();
    segments
        .iter()
        .all(|segment| !segment.is_empty())
        .then_some(segments)
}

/// The file and the container path of the symbol with the key.
fn key_path(key: &str) -> Option<(&str, &str)> {
    let (file, path) = key.rsplit_once('#')?;
    let path = path.split('~').next().unwrap_or(path);
    Some((file, path))
}

impl Context<'_> {
    fn handler_paths(&self) -> &HandlerPaths {
        self.all_parsed
            .shared()
            .handler_paths
            .get_or_init(|| HandlerPaths::new(self.all))
    }

    fn position_of_file(&self) -> Option<usize> {
        self.handler_paths()
            .by_file
            .get(self.file.file.as_str())
            .copied()
    }

    /// The handler that a Rust path names: `module::handler`, `crate::a::handler`,
    /// `Self::handler`, or `module::handler::<T>`.
    pub(super) fn rust_path_handler(&self, handler: Node<'_>) -> Option<String> {
        let handler = if handler.kind() == "generic_function" {
            handler.child_by_field_name("function")?
        } else {
            handler
        };
        if handler.kind() != "scoped_identifier" {
            return None;
        }
        let segments = path_segments(self.text(handler), "::")?;
        let (head, rest) = segments.split_first()?;
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        let paths = self.handler_paths();
        let here = self.position_of_file()?;
        match head.as_str() {
            "Self" => {
                let [name] = rest.as_slice() else {
                    return None;
                };
                self.symbol_named(name, Some(&enclosing_impl_type(handler, self.source)?))
            }
            "crate" => paths.callable(self.all, paths.crate_root(here), &rest),
            "self" => paths.callable(self.all, here, &rest),
            "super" => {
                let mut module = *paths.parents.get(&here)?;
                let mut rest = rest.as_slice();
                while let ["super", tail @ ..] = rest {
                    module = *paths.parents.get(&module)?;
                    rest = tail;
                }
                paths.callable(self.all, module, rest)
            }
            _ => {
                if let Some(module) = paths.modules.get(&(here, head.clone())) {
                    return paths.callable(self.all, *module, &rest);
                }
                match self.imported_head(head) {
                    ImportedHead::None => {
                        let whole: Vec<&str> = segments.iter().map(String::as_str).collect();
                        paths.callable(self.all, here, &whole)
                    }
                    ImportedHead::Module(module) => paths.callable(self.all, module, &rest),
                    ImportedHead::Item(module, path) => {
                        let mut whole: Vec<&str> = path.split('.').collect();
                        whole.extend(rest);
                        paths.callable(self.all, module, &whole)
                    }
                    ImportedHead::Unknown => None,
                }
            }
        }
    }

    /// The handler that a path of names with `.` gives through the import of its first name,
    /// as `views.detail` after `from . import views` gives. `None` when the import does not
    /// select exactly one symbol.
    pub(super) fn imported_path_handler(&self, handler: Node<'_>) -> Option<String> {
        let segments = path_segments(self.text(handler), ".")?;
        let (head, rest) = segments.split_first()?;
        if rest.is_empty() {
            return None;
        }
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        let paths = self.handler_paths();
        match self.imported_head(head) {
            ImportedHead::Module(module) => paths.callable(self.all, module, &rest),
            ImportedHead::Item(module, path) => {
                let mut whole: Vec<&str> = path.split('.').collect();
                whole.extend(rest);
                paths.callable(self.all, module, &whole)
            }
            ImportedHead::None | ImportedHead::Unknown => None,
        }
    }

    /// What the imports of the file bind to the first name of a path.
    fn imported_head(&self, head: &str) -> ImportedHead {
        let paths = self.handler_paths();
        let mut found: Vec<ImportedHead> = Vec::new();
        for binding in &self.file.imports {
            if binding.local_name.as_deref() != Some(head)
                || !matches!(
                    binding.kind,
                    ImportKind::Module | ImportKind::Named | ImportKind::ReExportNamed
                )
            {
                continue;
            }
            let ImportTarget::Resolved(target) = &binding.target_file else {
                return ImportedHead::Unknown;
            };
            let Some(module) = paths.by_file.get(target).copied() else {
                return ImportedHead::Unknown;
            };
            let imported = binding.imported_name.as_deref().unwrap_or(head);
            // A Rust module and a function can have one name, because they are in
            // different namespaces. The head of a path is never the function.
            let names_module = match self.file.language {
                Language::Rust => paths.module_names.contains(&(module, imported.to_string())),
                _ => binding.target_symbol.is_none(),
            };
            let head = if names_module {
                ImportedHead::Module(module)
            } else {
                match binding.target_symbol.as_deref().and_then(key_path) {
                    Some((file, path)) => match paths.by_file.get(file) {
                        Some(file) => ImportedHead::Item(*file, path.to_string()),
                        None => ImportedHead::Unknown,
                    },
                    None => ImportedHead::Unknown,
                }
            };
            if !found.contains(&head) {
                found.push(head);
            }
        }
        match found.len() {
            0 => ImportedHead::None,
            1 => found.remove(0),
            _ => ImportedHead::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ImportedHead {
    /// No import binds the name.
    None,
    /// The import binds the name to a module, the file at the position.
    Module(usize),
    /// The import binds the name to a symbol: its file and its container path with its name.
    Item(usize, String),
    /// An import binds the name, but the index does not select one module or symbol.
    Unknown,
}

/// The name of the type of the `impl` block that contains the node, without its path
/// and its generic arguments.
fn enclosing_impl_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" {
            let text = syntax::text(parent.child_by_field_name("type")?, source);
            let name = text.split('<').next().unwrap_or(text);
            return Some(name.rsplit("::").next().unwrap_or(name).trim().to_string());
        }
        current = parent.parent();
    }
    None
}
