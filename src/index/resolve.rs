//! Call resolution. The rules run in a fixed order, and the first rule that
//! applies gives the basis. A rule that needs a guess gives no edge.
//!
//! The order is: a lexical name, a member of the object itself or of its
//! parent type, a qualifier, a receiver with a known type, and then a bare
//! name. A qualifier or a receiver that fails does not continue to the rules
//! for a bare name.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use rayon::prelude::*;

use super::calls::{element_type, one_result, required_type, split_list, split_type, Argument};
use super::extract::{name_path, FileSymbols, LinkFacts, Local};
use super::model::{
    ImportBinding, ImportKind, ImportTarget, Receiver, Reference, ReferenceKind, Resolution,
    ResolutionBasis as Basis, ResolutionStatus as Status, Symbol, SymbolKind,
};
use crate::core::Language;

/// Languages that can call the definitions of each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Go,
    Rust,
    Python,
    Ecma,
    Java,
    CSharp,
    C,
    Ruby,
    Php,
    Bash,
}

fn family(language: Language) -> Family {
    match language {
        Language::Go => Family::Go,
        Language::Rust => Family::Rust,
        Language::Python => Family::Python,
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => Family::Ecma,
        Language::Java => Family::Java,
        Language::CSharp => Family::CSharp,
        Language::C | Language::Cpp => Family::C,
        Language::Ruby => Family::Ruby,
        Language::Php => Family::Php,
        Language::Bash => Family::Bash,
    }
}

/// A symbol: the position of its file and its position in that file.
type Id = (usize, usize);

#[derive(Debug, Clone)]
struct Outcome {
    status: Status,
    basis: Option<Basis>,
    candidates: Vec<Id>,
    /// The definitions that a later definition with the same name replaces.
    superseded: Vec<Id>,
}

impl Outcome {
    fn none(status: Status) -> Self {
        Self {
            status,
            basis: None,
            candidates: Vec::new(),
            superseded: Vec::new(),
        }
    }

    /// One candidate gives an edge. More than one candidate is ambiguous.
    fn of(basis: Basis, mut candidates: Vec<Id>) -> Option<Self> {
        candidates.sort();
        candidates.dedup();
        let status = match candidates.len() {
            0 => return None,
            1 => Status::UniqueCandidate,
            _ => Status::Ambiguous,
        };
        Some(Self {
            status,
            basis: Some(basis),
            candidates,
            superseded: Vec::new(),
        })
    }

    /// The candidates without an edge, because the rule cannot select one.
    fn offered(basis: Basis, mut candidates: Vec<Id>) -> Self {
        candidates.sort();
        candidates.dedup();
        if candidates.is_empty() {
            return Self::none(Status::Unresolved);
        }
        Self {
            status: Status::Ambiguous,
            basis: Some(basis),
            candidates,
            superseded: Vec::new(),
        }
    }
}

/// The result of the lookup of a C++ namespace alias.
enum AliasLookup<'a> {
    /// No visible alias has the name.
    None,
    /// The namespace that the alias names.
    Namespace(&'a str),
    /// More than one alias, or an alias and a different binding, have the
    /// name at the call.
    Uncertain,
}

/// The words that name the object of a method.
const SELF_WORDS: &[&str] = &["self", "this", "$this", "cls"];

/// The members that an object that is a function value has, and that do not
/// run the function. The name of the one method of a functional interface,
/// as Java `applyAsInt`, is not in this list: it runs the function.
const NON_INVOCATION_MEMBERS: &[&str] = &[
    "toString",
    "equals",
    "hashCode",
    "getClass",
    "clone",
    "finalize",
    "wait",
    "notify",
    "notifyAll",
    "bind",
    "valueOf",
    "toLocaleString",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "constructor",
    "length",
    "name",
    "andThen",
    "compose",
    "negate",
    "and",
    "or",
    "reversed",
    "thenComparing",
    "__str__",
    "__repr__",
    "__eq__",
    "__hash__",
    "to_s",
    "inspect",
    "class",
];

/// Tells if the text of a type has `|` that is not in brackets.
fn is_union(text: &str) -> bool {
    let mut depth = 0usize;
    text.chars().any(|character| {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        character == '|' && depth == 0
    })
}

fn is_callable(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

fn is_container(symbol: &Symbol) -> bool {
    symbol.kind.is_type() && symbol.kind != SymbolKind::TypeAlias
}

fn directory(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn segments(path: &str) -> Vec<&str> {
    path.split(|character| ".:\\".contains(character))
        .filter(|segment| !segment.is_empty())
        .collect()
}

struct Resolver<'a> {
    files: &'a [FileSymbols],
    by_name: HashMap<&'a str, Vec<Id>>,
    by_file: HashMap<&'a str, usize>,
    /// For each file, the positions of the local names by name, in source order.
    locals_by_name: Vec<HashMap<&'a str, Vec<usize>>>,
    /// For each file, the positions of the references by start byte, in source order.
    references_by_start: Vec<HashMap<usize, Vec<usize>>>,
    /// For each Ruby type, the types that have it as a direct parent type.
    ruby_children: OnceLock<BTreeMap<Id, Vec<Id>>>,
    /// The results of `types`, `package_files`, and `visible_files`. The
    /// resolution of one call asks for the same facts many times.
    types_cache: RwLock<HashMap<TypeQuery, Vec<Id>>>,
    package_cache: RwLock<HashMap<usize, Arc<Vec<usize>>>>,
    visible_cache: RwLock<HashMap<usize, Arc<Vec<usize>>>>,
}

/// The file, the name, and the qualifier of one type lookup.
type TypeQuery = (usize, String, Option<String>);

impl<'a> Resolver<'a> {
    fn new(files: &'a [FileSymbols]) -> Self {
        let mut by_name: HashMap<&str, Vec<Id>> = HashMap::new();
        let mut by_file = HashMap::new();
        for (file, facts) in files.iter().enumerate() {
            by_file.insert(facts.file.as_str(), file);
            for (position, symbol) in facts.symbols.iter().enumerate() {
                by_name
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push((file, position));
            }
        }
        let locals_by_name = files
            .iter()
            .map(|facts| {
                let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
                for (position, local) in facts.locals.iter().enumerate() {
                    positions
                        .entry(local.name.name.as_str())
                        .or_default()
                        .push(position);
                }
                positions
            })
            .collect();
        let references_by_start = files
            .iter()
            .map(|facts| {
                let mut positions: HashMap<usize, Vec<usize>> = HashMap::new();
                for (position, reference) in facts.reference_facts.iter().enumerate() {
                    positions
                        .entry(reference.start_byte)
                        .or_default()
                        .push(position);
                }
                positions
            })
            .collect();
        Self {
            files,
            by_name,
            by_file,
            locals_by_name,
            references_by_start,
            ruby_children: OnceLock::new(),
            types_cache: RwLock::new(HashMap::new()),
            package_cache: RwLock::new(HashMap::new()),
            visible_cache: RwLock::new(HashMap::new()),
        }
    }

    fn symbol(&self, id: Id) -> &'a Symbol {
        &self.files[id.0].symbols[id.1]
    }

    fn facts(&self, id: Id) -> &'a LinkFacts {
        &self.files[id.0].link_facts[id.1]
    }

    fn family(&self, file: usize) -> Family {
        family(self.files[file].language)
    }

    /// The symbols with this name in the language family of `file`.
    /// The symbols with this name in one file. The symbols of a name are in
    /// the order of the files, thus a binary search finds the file.
    fn named_in(&self, name: &str, file: usize) -> impl Iterator<Item = Id> + '_ {
        let all = self.by_name.get(name).map_or(&[][..], Vec::as_slice);
        let start = all.partition_point(|id| id.0 < file);
        let end = all.partition_point(|id| id.0 <= file);
        all[start..end].iter().copied()
    }

    fn named(&self, name: &str, file: usize) -> impl Iterator<Item = Id> + '_ {
        let family = self.family(file);
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |id| self.family(id.0) == family)
    }

    fn same_package(&self, left: usize, right: usize) -> bool {
        let (left_file, right_file) = (&self.files[left], &self.files[right]);
        match self.family(left) {
            Family::Go => directory(&left_file.file) == directory(&right_file.file),
            Family::Java | Family::CSharp | Family::Php => left_file
                .modules
                .iter()
                .any(|module| right_file.modules.contains(module)),
            _ => false,
        }
    }

    /// The files of a certain import target.
    fn target_files(&self, binding: &ImportBinding) -> Vec<usize> {
        let paths: &[String] = match &binding.target_file {
            ImportTarget::Resolved(path) => std::slice::from_ref(path),
            ImportTarget::Package(paths) => paths,
            _ => &[],
        };
        paths
            .iter()
            .filter_map(|path| self.by_file.get(path.as_str()).copied())
            .collect()
    }

    /// The files that `file` loads or imports with a wildcard, and the files
    /// that those files load.
    fn visible_files(&self, file: usize) -> Arc<Vec<usize>> {
        if let Some(found) = self.visible_cache.read().get(&file) {
            return Arc::clone(found);
        }
        let found = Arc::new(self.files_through_loads(file));
        self.visible_cache.write().insert(file, Arc::clone(&found));
        found
    }

    /// The files of the same package as `file`, with `file`.
    fn package_files(&self, file: usize) -> Arc<Vec<usize>> {
        if let Some(found) = self.package_cache.read().get(&file) {
            return Arc::clone(found);
        }
        let found: Arc<Vec<usize>> = Arc::new(
            (0..self.files.len())
                .filter(|other| self.same_package(file, *other))
                .collect(),
        );
        self.package_cache.write().insert(file, Arc::clone(&found));
        found
    }

    fn files_through_loads(&self, file: usize) -> Vec<usize> {
        let mut seen = BTreeSet::from([file]);
        let mut queue = vec![file];
        let mut first = true;
        while let Some(current) = queue.pop() {
            for binding in &self.files[current].imports {
                let follows = match binding.kind {
                    ImportKind::Load => true,
                    ImportKind::Wildcard | ImportKind::ReExportWildcard => first,
                    _ => false,
                };
                if !follows {
                    continue;
                }
                for target in self.target_files(binding) {
                    if seen.insert(target) && binding.kind == ImportKind::Load {
                        queue.push(target);
                    }
                }
            }
            first = false;
        }
        seen.remove(&file);
        seen.into_iter().collect()
    }

    /// The definitions that are not in a type or a function.
    fn top_level(&self, name: &str, files: &[usize]) -> Vec<Id> {
        let mut found = Vec::new();
        for &file in files {
            for (position, symbol) in self.files[file].symbols.iter().enumerate() {
                let facts = &self.files[file].link_facts[position];
                if symbol.name == name && symbol.container.is_none() && facts.parent.is_none() {
                    found.push((file, position));
                }
            }
        }
        found
    }

    /// The top-level definition `name` of a file, or the definition that the
    /// re-exports of the file give.
    fn exported_name(
        &self,
        file: usize,
        name: &str,
        seen: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Id> {
        if !seen.insert((file, name.to_string())) {
            return Vec::new();
        }
        if name == "default" {
            let facts = &self.files[file];
            let defaults: Vec<Id> = (0..facts.symbols.len())
                .filter(|position| facts.link_facts[*position].default_export)
                .map(|position| (file, position))
                .collect();
            if !defaults.is_empty() {
                return defaults;
            }
        }
        let found = self.top_level(name, &[file]);
        if !found.is_empty() {
            return found;
        }
        // `export { original as name }` gives the name to a symbol of the file.
        let aliased: Vec<Id> = (0..self.files[file].symbols.len())
            .filter(|position| {
                self.files[file].link_facts[*position]
                    .export_aliases
                    .iter()
                    .any(|alias| alias == name)
            })
            .map(|position| (file, position))
            .collect();
        if !aliased.is_empty() {
            return aliased;
        }
        // A name in a module in the file, as a Rust `mod testing { ... }` has it.
        let in_module: Vec<Id> = self
            .named_in(name, file)
            .filter(|id| {
                id.0 == file
                    && self.facts(*id).parent.is_some_and(|parent| {
                        self.symbol((file, parent)).kind == SymbolKind::Module
                    })
            })
            .collect();
        if let [only] = in_module.as_slice() {
            return vec![*only];
        }
        let python = self.family(file) == Family::Python;
        let mut found = Vec::new();
        for binding in &self.files[file].imports {
            let named = binding.kind == ImportKind::ReExportNamed
                || (python && binding.kind == ImportKind::Named);
            let wildcard = binding.kind == ImportKind::ReExportWildcard
                || (python && binding.kind == ImportKind::Wildcard);
            for target in self.target_files(binding) {
                if named && binding.local_name.as_deref() == Some(name) {
                    let source = binding.imported_name.as_deref().unwrap_or(name);
                    found.extend(self.exported_name(target, source, seen));
                } else if wildcard {
                    found.extend(self.exported_name(target, name, seen));
                }
            }
        }
        found
    }

    /// The types that a name can refer to in `file`. The first rule that
    /// finds a type gives the result.
    fn types(&self, name: &str, qualifier: Option<&str>, file: usize) -> Vec<Id> {
        let query = (file, name.to_string(), qualifier.map(str::to_string));
        if let Some(found) = self.types_cache.read().get(&query) {
            return found.clone();
        }
        let found = self.types_of_query(name, qualifier, file);
        self.types_cache.write().insert(query, found.clone());
        found
    }

    fn types_of_query(&self, name: &str, qualifier: Option<&str>, file: usize) -> Vec<Id> {
        let mut found = self.types_by_name(name, qualifier, file);
        let Some(path) = qualifier.map(segments).filter(|path| !path.is_empty()) else {
            return found;
        };
        let containers = |id: &Id| -> Vec<&str> {
            self.facts(*id)
                .container_path
                .iter()
                .flat_map(|container| segments(container))
                .collect()
        };
        // In these languages the syntax gives the complete namespace of a
        // type. A qualifier that is not an import and that does not end the
        // namespace of a type names a different type, also when the
        // repository has only one type with the name.
        let complete_namespaces = matches!(
            self.family(file),
            Family::Java | Family::CSharp | Family::C | Family::Php
        );
        if complete_namespaces && !self.is_import_qualifier(qualifier.unwrap_or_default(), file) {
            found.retain(|id| {
                let declared = self.files[id.0].modules.iter().any(|module| {
                    let mut namespace = segments(module);
                    namespace.extend(containers(id));
                    namespace.ends_with(&path)
                });
                declared || containers(id).ends_with(&path)
            });
            return found;
        }
        // A Rust qualifier names a module. Evidence in the repository must
        // show that the module has the type, also when only one type has the name.
        if self.family(file) == Family::Rust
            && !self.is_import_qualifier(qualifier.unwrap_or_default(), file)
        {
            let relative: Vec<&str> = path
                .iter()
                .copied()
                .skip_while(|segment| ["crate", "self", "super"].contains(segment))
                .collect();
            let module_file = self.files[file].path_targets.get(&path.join("::"));
            found.retain(|id| {
                (!relative.is_empty() && containers(id).ends_with(&relative))
                    || module_file.is_some_and(|module| *module == self.files[id.0].file)
            });
            return found;
        }
        // More than one type can have the name. The qualifier selects the
        // types whose containers have the names of the qualifier, or whose
        // file declares the package that the qualifier names.
        if found.len() > 1 {
            let within = |id: &Id| {
                let declared = self.files[id.0]
                    .modules
                    .iter()
                    .any(|module| segments(module) == path);
                declared || containers(id).ends_with(&path)
            };
            if found.iter().any(within) {
                found.retain(within);
            }
        }
        found
    }

    /// Tells if the last name of `qualifier` is the local name of an import
    /// of `file`. Then the import selects the files of the type.
    fn is_import_qualifier(&self, qualifier: &str, file: usize) -> bool {
        let last = segments(qualifier).last().copied().unwrap_or(qualifier);
        self.files[file]
            .imports
            .iter()
            .any(|binding| binding.local_name.as_deref() == Some(last))
    }

    fn types_by_name(&self, name: &str, qualifier: Option<&str>, file: usize) -> Vec<Id> {
        let is_type = |id: &Id| is_container(self.symbol(*id)) || self.facts(*id).defined_type;
        let in_files = |files: &[usize]| -> Vec<Id> {
            self.named(name, file)
                .filter(|id| files.contains(&id.0) && is_type(id))
                .collect()
        };
        let facts = &self.files[file];
        if let Some(qualifier) = qualifier {
            let last = segments(qualifier).last().copied().unwrap_or(qualifier);
            let mut bindings = facts
                .imports
                .iter()
                .filter(|binding| binding.local_name.as_deref() == Some(last))
                .peekable();
            // A qualifier that is an import selects only the files of that import.
            if bindings.peek().is_some() {
                let imported: Vec<usize> = bindings
                    .flat_map(|binding| self.target_files(binding))
                    .collect();
                return in_files(&imported);
            }
        }
        let found = in_files(&[file]);
        if !found.is_empty() {
            return found;
        }
        let mut found = Vec::new();
        let mut outside = false;
        // An import of a name does not apply to a name with a qualifier.
        for binding in facts.imports.iter().filter(|_| qualifier.is_none()) {
            if binding.local_name.as_deref() != Some(name) {
                continue;
            }
            let imported = binding.imported_name.as_deref().unwrap_or(name);
            let targets = self.target_files(binding);
            outside |= targets.is_empty();
            for target in targets {
                found.extend(
                    self.exported_name(target, imported, &mut BTreeSet::new())
                        .into_iter()
                        .filter(is_type),
                );
            }
        }
        // An import of this name from a file outside the repository owns the
        // name. A type of the repository with the same name is a different type.
        if !found.is_empty() || outside {
            return found;
        }
        let found = in_files(&self.package_files(file));
        if !found.is_empty() {
            return found;
        }
        let found = in_files(&self.visible_files(file));
        if !found.is_empty() {
            return found;
        }
        self.named(name, file).filter(is_type).collect()
    }

    /// The container path that a member of the type `owner` has.
    fn member_path(&self, owner: Id) -> Vec<String> {
        let mut path = self.facts(owner).container_path.clone();
        path.push(self.symbol(owner).name.clone());
        path
    }

    /// The callable members `name` of a type. The search goes to the parent
    /// types when the type has no such member.
    fn members(&self, owner: Id, name: &str, arity: usize, seen: &mut BTreeSet<Id>) -> Vec<Id> {
        if !seen.insert(owner) {
            return Vec::new();
        }
        // A Ruby `prepend` puts a module before the class in the search.
        let prepended: Vec<Id> = self
            .facts(owner)
            .prepends
            .iter()
            .flat_map(|module| {
                let (qualifier, name) = split_type(module);
                self.types(&name, qualifier.as_deref(), owner.0)
            })
            .flat_map(|module| self.members(module, name, arity, seen))
            .collect();
        if !prepended.is_empty() {
            return prepended;
        }
        let type_symbol = self.symbol(owner);
        let namespace = type_symbol.kind == SymbolKind::Module;
        let owner_path = self.member_path(owner);
        let mut found: Vec<Id> = self
            .named(name, owner.0)
            .filter(|id| {
                let member = self.symbol(*id);
                let member_path = &self.facts(*id).container_path;
                is_callable(member)
                    && member.container.as_deref() == Some(type_symbol.name.as_str())
                    // Two types can have one name. A member in the same file
                    // belongs to this type when its path is the path of the
                    // type. A member in a different file belongs to this type
                    // only when that file gives this type for the name.
                    && (namespace
                        || (id.0 == owner.0
                            && (member_path.is_empty() || *member_path == owner_path))
                        || (id.0 != owner.0
                            && self.types(&type_symbol.name, None, id.0) == [owner]))
            })
            .collect();
        found = self.preferred(found, arity);
        if !found.is_empty() {
            return found;
        }
        self.parents(owner)
            .into_iter()
            .flat_map(|parent| self.members(parent, name, arity, seen))
            .collect()
    }

    /// Removes the candidates that a more specific candidate replaces:
    /// a trait implementation when the type has its own member, a declaration
    /// when a definition exists, and a different number of parameters.
    fn preferred(&self, mut found: Vec<Id>, arity: usize) -> Vec<Id> {
        let keep = |found: &mut Vec<Id>, test: &dyn Fn(&Id) -> bool| {
            if found.len() > 1 && found.iter().any(test) {
                found.retain(test);
            }
        };
        keep(&mut found, &|id| !self.facts(*id).trait_implementation);
        // A definition replaces the declaration that it defines. A
        // declaration of a different overload stays.
        let defined = found.clone();
        found.retain(|id| {
            self.symbol(*id).body_range.is_some()
                || !defined.iter().any(|other| {
                    self.symbol(*other).body_range.is_some() && self.same_parameters(*id, *other)
                })
        });
        // Only a language with overloads selects by the number of arguments.
        let overloads = found.first().is_some_and(|id| {
            matches!(
                self.family(id.0),
                Family::Java | Family::CSharp | Family::C | Family::Ecma
            )
        });
        if overloads {
            keep(&mut found, &|id| {
                self.facts(*id).parameter_count == Some(arity)
            });
        }
        found
    }

    /// Tells if two definitions can have the same parameters. The types
    /// decide when both definitions have them. Otherwise the number of
    /// parameters decides. A definition with no known parameters can match.
    fn same_parameters(&self, left: Id, right: Id) -> bool {
        let (left, right) = (self.facts(left), self.facts(right));
        // `(void)` and `()` both declare no parameter.
        let parameters = |types: &Vec<String>| -> Vec<String> {
            match types.as_slice() {
                [only] if only == "void" => Vec::new(),
                _ => types.clone(),
            }
        };
        match (
            (&left.parameter_types, &right.parameter_types),
            (left.parameter_count, right.parameter_count),
        ) {
            ((Some(left), Some(right)), _) => parameters(left) == parameters(right),
            (_, (Some(left), Some(right))) => left == right,
            _ => true,
        }
    }

    fn parents(&self, owner: Id) -> Vec<Id> {
        self.symbol(owner)
            .supertypes
            .iter()
            .flat_map(|parent| {
                let (qualifier, name) = split_type(parent);
                self.types(&name, qualifier.as_deref(), owner.0)
            })
            .collect()
    }

    /// The constructors of a type or of its nearest parent type that has one.
    /// A type without a constructor is its own constructor.
    fn constructors(&self, owner: Id, arity: usize) -> Vec<Id> {
        let mut seen = BTreeSet::new();
        let mut level = vec![owner];
        while !level.is_empty() {
            let found: Vec<Id> = level
                .iter()
                .flat_map(|owner| self.own_constructors(*owner, arity))
                .collect();
            if !found.is_empty() {
                return found;
            }
            level = level
                .iter()
                .flat_map(|owner| self.parents(*owner))
                .filter(|parent| seen.insert(*parent))
                .collect();
        }
        vec![owner]
    }

    fn own_constructors(&self, owner: Id, arity: usize) -> Vec<Id> {
        let type_symbol = self.symbol(owner);
        let owner_path = self.member_path(owner);
        let found: Vec<Id> = (0..self.files[owner.0].symbols.len())
            .map(|position| (owner.0, position))
            .chain(
                self.named(&type_symbol.name, owner.0)
                    .filter(|id| id.0 != owner.0 && self.symbol(*id).receiver_type.is_some()),
            )
            .filter(|id| {
                let member = self.symbol(*id);
                let path = &self.facts(*id).container_path;
                member.kind == SymbolKind::Constructor
                    && member.container.as_deref() == Some(type_symbol.name.as_str())
                    && (id.0 != owner.0 || path.is_empty() || *path == owner_path)
            })
            .collect();
        self.preferred(found, arity)
    }

    /// The definitions that contain a definition, from the nearest one.
    fn chain(&self, file: usize, owner: Option<usize>) -> Vec<Id> {
        let mut chain = Vec::new();
        let mut current = owner;
        while let Some(position) = current {
            chain.push((file, position));
            current = self.files[file].link_facts[position].parent;
        }
        chain
    }

    /// The type whose member contains the reference.
    fn enclosing_type(&self, chain: &[Id]) -> Option<Id> {
        for id in chain {
            let symbol = self.symbol(*id);
            if is_container(symbol) {
                return Some(*id);
            }
            if let Some(container) = &symbol.container {
                let parent = self.facts(*id).parent.map(|position| (id.0, position));
                if let Some(parent) = parent.filter(|parent| is_container(self.symbol(*parent))) {
                    return Some(parent);
                }
                let types = self.types(container, None, id.0);
                if let [only] = types.as_slice() {
                    return Some(*only);
                }
            }
        }
        None
    }

    /// The local names `name` of `file`, in source order.
    fn locals_named(&self, file: usize, name: &str) -> impl Iterator<Item = &'a Local> + '_ {
        let locals = &self.files[file].locals;
        self.locals_by_name[file]
            .get(name)
            .into_iter()
            .flatten()
            .map(move |position| &locals[*position])
    }

    /// The references of `file` that start at the byte `start`, in source order.
    fn references_at(&self, file: usize, start: usize) -> &[usize] {
        self.references_by_start[file]
            .get(&start)
            .map_or(&[], Vec::as_slice)
    }

    /// The local name `name` that a reference at the byte `at` sees. When two
    /// names of one owner have the name, the one in the nearest block that
    /// contains `at` hides the others.
    fn local(&self, chain: &[Id], file: usize, name: &str, at: Option<usize>) -> Option<&'a Local> {
        let owners = chain.iter().map(|id| Some(id.1)).chain([None]);
        for owner in owners {
            let found = self
                .locals_named(file, name)
                .filter(|local| {
                    local.owner == owner
                        && at.is_none_or(|at| {
                            local.block.as_ref().is_none_or(|block| block.contains(&at))
                        })
                })
                .min_by_key(|local| local.block.as_ref().map_or(usize::MAX, |block| block.len()));
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The methods with this name in the language family. The rules offer
    /// them when the type of a receiver is not known.
    fn methods_by_name(&self, name: &str, file: usize) -> Vec<Id> {
        self.named(name, file)
            .filter(|id| is_callable(self.symbol(*id)) && self.symbol(*id).container.is_some())
            .collect()
    }

    /// The types of the value of a local name. `None` means that the syntax
    /// does not give a type.
    fn local_types(&self, local: &Local, file: usize, depth: usize) -> Option<Vec<Id>> {
        let element = local.name.element;
        if let Some(type_name) = &local.name.type_name {
            let (_, name) = split_type(type_name);
            // The declared type of a loop variable is the type of the element.
            if !name.is_empty() && !["var", "auto", "let", "_"].contains(&name.as_str()) {
                return Some(self.types_of_text(type_name, file, false));
            }
        }
        if depth > 4 {
            return None;
        }
        let start = local.name.value_start?;
        if let Some(position) = self.references_at(file, start).first() {
            let index = local.name.result_index;
            return self.result_types_of(file, *position, depth, element, index);
        }
        let path = local.value_path.as_ref()?;
        let chain = self.chain(file, local.owner);
        self.path_types(
            file,
            &chain,
            path,
            element,
            local.name.value_start,
            depth + 1,
        )
    }

    /// The types that the text of a declared type names in `file`. A slice,
    /// an array, or a list gives the type of its elements when `element` is
    /// set, and no type when it is not set.
    fn types_of_text(&self, text: &str, file: usize, element: bool) -> Vec<Id> {
        let text = required_type(text);
        let text = match (element_type(text), element) {
            (Some(inner), true) if element_type(inner).is_none() => required_type(inner),
            (None, false) => text,
            // A name can be an alias of a collection type, as Go
            // `type byName []*Item` is.
            (None, true) => {
                let (_, name) = split_type(text);
                let aliases: Vec<Id> = self
                    .named(&name, file)
                    .filter(|id| {
                        self.symbol(*id).kind == SymbolKind::TypeAlias
                            && (id.0 == file || self.package_files(file).contains(&id.0))
                    })
                    .collect();
                let [alias] = aliases.as_slice() else {
                    return Vec::new();
                };
                return match self.facts(*alias).return_type.as_deref() {
                    Some(aliased) if element_type(aliased).is_some() => {
                        self.types_of_text(aliased, alias.0, true)
                    }
                    _ => Vec::new(),
                };
            }
            _ => return Vec::new(),
        };
        // A union of two or more types does not tell which type the value has.
        if is_union(text) {
            return Vec::new();
        }
        let (qualifier, name) = split_type(text);
        if name.is_empty() {
            return Vec::new();
        }
        self.types(&name, qualifier.as_deref(), file)
    }

    /// The text of the declared type of the field `name` of a type, and the
    /// file that has the text. The search goes to the parent types when the
    /// type has no such field.
    fn field_text(
        &self,
        owner: Id,
        name: &str,
        seen: &mut BTreeSet<Id>,
    ) -> Option<(&'a str, usize)> {
        if !seen.insert(owner) {
            return None;
        }
        let type_name = self.symbol(owner).name.as_str();
        let mut owner_path = self.facts(owner).container_path.clone();
        owner_path.push(type_name.to_string());
        // Two or more assignments can give one field. They must agree on the type.
        let declared: BTreeSet<(&'a str, usize)> = self
            .named(name, owner.0)
            .filter(|id| {
                let field = self.symbol(*id);
                field.kind == SymbolKind::Field
                    && field.container.as_deref() == Some(type_name)
                    && if id.0 == owner.0 {
                        self.facts(*id).container_path == owner_path
                    } else {
                        self.types(type_name, None, id.0) == [owner]
                    }
            })
            .map(|id| (self.facts(id).return_type.as_deref().unwrap_or(""), id.0))
            .collect();
        let texts: BTreeSet<&str> = declared.iter().map(|(text, _)| *text).collect();
        match (texts.len(), declared.first()) {
            (1, Some((text, text_file))) if !text.is_empty() => return Some((text, *text_file)),
            (0, _) => {}
            _ => return None,
        }
        let inherited: Vec<(&'a str, usize)> = self
            .parents(owner)
            .into_iter()
            .filter_map(|parent| self.field_text(parent, name, seen))
            .collect();
        match inherited.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The declared type of the field `name` of the object, in a language
    /// where a method names a field of its object without `this`.
    fn own_field(&self, file: usize, chain: &[Id], name: &str) -> Option<(&'a str, usize)> {
        if !matches!(self.family(file), Family::Java | Family::CSharp | Family::C) {
            return None;
        }
        let owner = self.enclosing_type(chain)?;
        self.field_text(owner, name, &mut BTreeSet::new())
    }

    /// The types of a chain of member accesses: a local name or the object
    /// of the method, and then one field for each next name. `None` means
    /// that one name of the chain has no declared type.
    fn path_types(
        &self,
        file: usize,
        chain: &[Id],
        path: &[String],
        element: bool,
        at: Option<usize>,
        depth: usize,
    ) -> Option<Vec<Id>> {
        if depth > 4 {
            return None;
        }
        let (root, fields) = path.split_first()?;
        let mut owners: Vec<Id> = if SELF_WORDS.contains(&root.as_str()) {
            self.enclosing_type(chain).into_iter().collect()
        } else if let Some(local) = self.local(chain, file, root, at) {
            if fields.is_empty() && element {
                let declared = local.name.type_name.as_deref()?;
                return Some(self.types_of_text(declared, file, true));
            }
            self.local_types(local, file, depth + 1)?
        } else {
            let (text, text_file) = self.own_field(file, chain, root)?;
            self.types_of_text(text, text_file, element && fields.is_empty())
        };
        for (position, field) in fields.iter().enumerate() {
            let [owner] = owners.as_slice() else {
                return None;
            };
            let (text, text_file) = self.field_text(*owner, field, &mut BTreeSet::new())?;
            let last = position + 1 == fields.len();
            owners = self.types_of_text(text, text_file, element && last);
        }
        Some(owners)
    }

    /// Tells if the last name of a chain of member accesses is a field with a
    /// pointer type.
    fn ends_in_pointer(
        &self,
        file: usize,
        chain: &[Id],
        path: &[String],
        at: Option<usize>,
        depth: usize,
    ) -> bool {
        let Some((last, before)) = path.split_last().filter(|(_, before)| !before.is_empty())
        else {
            return false;
        };
        let owners = self.path_types(file, chain, before, false, at, depth + 1);
        let Some([owner]) = owners.as_deref() else {
            return false;
        };
        self.field_text(*owner, last, &mut BTreeSet::new())
            .is_some_and(|(declared, _)| declared.contains('*'))
    }

    /// The types of the value that a call gives. `None` means that the syntax
    /// does not give a type: the call has no one callee, or the callee has no
    /// declared result type.
    fn result_types(&self, file: usize, position: usize, depth: usize) -> Option<Vec<Id>> {
        self.result_types_of(file, position, depth, false, None)
    }

    /// The types of one result of a call, or of the elements of that result.
    /// `index` selects one result of a call that gives two or more results.
    fn result_types_of(
        &self,
        file: usize,
        position: usize,
        depth: usize,
        element: bool,
        index: Option<usize>,
    ) -> Option<Vec<Id>> {
        let facts = &self.files[file];
        let reference = &facts.references[position];
        let whole = !element && index.is_none();
        if reference.kind == ReferenceKind::Construct {
            if !whole {
                return None;
            }
            return Some(self.types(&reference.name, reference.qualifier.as_deref(), file));
        }
        if depth > 4 {
            return None;
        }
        // Ruby `self.class` gives the class of the object.
        if whole
            && self.family(file) == Family::Ruby
            && reference.name == "class"
            && reference.receiver == Some(Receiver::SelfValue)
        {
            let chain = self.chain(file, facts.reference_facts[position].owner);
            return Some(self.enclosing_type(&chain).into_iter().collect());
        }
        let outcome = self.resolve(file, position, depth + 1);
        let [callee] = outcome.candidates.as_slice() else {
            return None;
        };
        if outcome.status != Status::UniqueCandidate {
            return None;
        }
        let symbol = self.symbol(*callee);
        let own_type = || {
            let container = symbol.container.as_deref()?;
            Some(self.types(container, None, callee.0))
        };
        if symbol.kind == SymbolKind::Constructor || is_container(symbol) {
            if !whole {
                return None;
            }
            return if is_container(symbol) {
                Some(vec![*callee])
            } else {
                own_type()
            };
        }
        let declared = self.facts(*callee).return_type.as_deref()?;
        let mut declared = one_result(declared, index)?;
        // Rust `call()?` gives the value of a `Result` or of an `Option`.
        // The other ways to get the value give no type.
        if self.family(file) == Family::Rust {
            let wrapper = declared
                .strip_prefix("Result<")
                .or_else(|| declared.strip_prefix("Option<"))
                .and_then(|rest| rest.strip_suffix('>'));
            match (wrapper, facts.reference_facts[position].tried) {
                (Some(inside), true) => declared = split_list(inside).first().copied()?,
                (Some(_), false) => return None,
                (None, _) => {}
            }
        }
        let (_, name) = split_type(declared);
        if ["Self", "self", "static"].contains(&name.as_str()) {
            return if element { None } else { own_type() };
        }
        Some(self.types_of_text(declared, callee.0, element))
    }

    fn on_types(
        &self,
        types: &[Id],
        name: &str,
        arity: usize,
        basis: Basis,
        file: usize,
    ) -> Outcome {
        // Ruby `Type.new` makes an object. It runs `initialize`.
        if let ([only], Family::Ruby, "new") = (types, self.family(file), name) {
            return Outcome::of(basis, self.constructors(*only, arity))
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        if types.is_empty() {
            return Outcome::none(Status::Unresolved);
        }
        let dynamic = types.iter().any(|id| {
            matches!(
                self.symbol(*id).kind,
                SymbolKind::Interface | SymbolKind::Trait
            )
        });
        if dynamic || types.len() > 1 {
            return Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file));
        }
        let members = self.members(types[0], name, arity, &mut BTreeSet::new());
        // A member without a body in each candidate is abstract. A subtype
        // gives the definition, and the syntax does not tell which subtype.
        // Ruby, Python, and Bash have no declaration without a definition: a
        // method with an empty body, as `def name; end`, is a definition.
        let declares = !matches!(
            self.family(types[0].0),
            Family::Ruby | Family::Python | Family::Bash
        );
        let is_abstract = |id: &Id| self.symbol(*id).body_range.is_none();
        if declares && !members.is_empty() && members.iter().all(is_abstract) {
            return Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file));
        }
        Outcome::of(basis, members).unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }

    /// The namespace that the C++ alias `alias` names at the call that starts
    /// at the byte `start`, in the enclosing definitions `chain`.
    ///
    /// The resolver does not model the C++ name lookup. An alias gives its
    /// namespace only when it is the only alias declaration with the name
    /// that is visible at the call, and no local name, parameter, or symbol
    /// with the name can be visible at the call. An alias that is declared
    /// after the call, or in a block that does not contain the call, is not
    /// visible.
    fn namespace_alias(
        &self,
        file: usize,
        chain: &[Id],
        alias: &str,
        start: usize,
    ) -> AliasLookup<'a> {
        let visible: Vec<&'a Local> = self
            .locals_named(file, alias)
            .filter(|local| {
                local
                    .name
                    .visible
                    .as_ref()
                    .is_some_and(|range| range.start < start && start < range.end)
            })
            .collect();
        let [declaration] = visible[..] else {
            return match visible.is_empty() {
                true => AliasLookup::None,
                false => AliasLookup::Uncertain,
            };
        };
        let owners: Vec<Option<usize>> = chain.iter().map(|id| Some(id.1)).chain([None]).collect();
        let other_local = self
            .locals_named(file, alias)
            .any(|local| local.name.visible.is_none() && owners.contains(&local.owner));
        let files = self.visible_files(file);
        let other_symbol = self
            .named(alias, file)
            .any(|id| id.0 == file || files.contains(&id.0));
        match declaration.name.type_name.as_deref() {
            Some(namespace) if !other_local && !other_symbol => AliasLookup::Namespace(namespace),
            _ => AliasLookup::Uncertain,
        }
    }

    /// The definitions `name` in the modules that a Rust path names inside
    /// the file of the binding, as `self::outer::name` does. The path has no
    /// module of the file when the result is `None`.
    fn inline_module_items(&self, binding: &ImportBinding, name: &str) -> Option<Vec<Id>> {
        let file = *self.by_file.get(binding.file.as_str())?;
        if self.family(file) != Family::Rust {
            return None;
        }
        let mut path: Vec<&str> = binding.module_path.split("::").collect();
        if path.first() != Some(&"self") {
            return None;
        }
        path.remove(0);
        if path.last() == Some(&name) {
            path.pop();
        }
        let inline = |position: usize| {
            self.named_in(path[position], file).any(|id| {
                id.0 == file
                    && self.symbol(id).kind == SymbolKind::Module
                    && self.facts(id).container_path == path[..position]
            })
        };
        if path.is_empty() || !(0..path.len()).all(inline) {
            return None;
        }
        Some(
            self.named_in(name, file)
                .filter(|id| {
                    id.0 == file
                        && self.facts(*id).container_path == path
                        && self.facts(*id).parent.is_some_and(|parent| {
                            self.symbol((file, parent)).kind == SymbolKind::Module
                        })
                })
                .collect(),
        )
    }

    fn imported(&self, binding: &ImportBinding, name: &str, arity: usize) -> Outcome {
        let targets = self.target_files(binding);
        if targets.is_empty() {
            return Outcome::none(match binding.target_file {
                ImportTarget::External => Status::External,
                ImportTarget::Ambiguous(_) => Status::Ambiguous,
                _ => Status::Unresolved,
            });
        }
        let aliased = binding.local_name != binding.imported_name;
        let basis = match binding.kind {
            ImportKind::Module => Basis::ImportedModule,
            _ if aliased => Basis::ImportedAlias,
            _ => Basis::ImportedName,
        };
        let mut found: Vec<Id> = match self.inline_module_items(binding, name) {
            Some(inside) => inside,
            None => targets
                .iter()
                .flat_map(|target| self.exported_name(*target, name, &mut BTreeSet::new()))
                .collect(),
        };
        found.retain(|id| is_callable(self.symbol(*id)) || is_container(self.symbol(*id)));
        if found.is_empty() {
            // A static member, as Java `import static a.Type.member` gives.
            let path = segments(&binding.module_path);
            if let [.., owner, _] = path.as_slice() {
                found = targets
                    .iter()
                    .flat_map(|target| self.types(owner, None, *target))
                    .flat_map(|owner| self.members(owner, name, arity, &mut BTreeSet::new()))
                    .collect();
            }
        }
        Outcome::of(basis, self.preferred(found, arity))
            .unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }

    fn with_receiver(
        &self,
        file: usize,
        reference: &Reference,
        chain: &[Id],
        x: &str,
        at: usize,
        depth: usize,
    ) -> Outcome {
        let name = reference.name.as_str();
        let arity = reference.arity;
        // A function value that a local name holds. Only a member that runs
        // the function is a call of it.
        let closure = self.named_in(x, file).find(|id| {
            !NON_INVOCATION_MEMBERS.contains(&name)
                && id.0 == file
                && is_callable(self.symbol(*id))
                && self.symbol(*id).container.is_some()
                // A member of a type with the name is not the value of a local name.
                && self.facts(*id).parent.is_some_and(|parent| {
                    chain.contains(&(file, parent)) && !is_container(self.symbol((file, parent)))
                })
        });
        if let Some(closure) = closure {
            return Outcome::of(Basis::LexicalScope, vec![closure])
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        let binding = self.files[file]
            .imports
            .iter()
            .find(|binding| binding.local_name.as_deref() == Some(x));
        // `const a = require('b')` is an import and a local name of the file.
        // The import gives the files.
        let local = self
            .local(chain, file, x, Some(at))
            .filter(|local| binding.is_none() || local.owner.is_some());
        if let Some(local) = local {
            // The members of an object literal have the name of the variable in their key.
            let literal: Vec<Id> = self
                .named_in(name, file)
                .filter(|id| {
                    id.0 == file
                        && self.symbol(*id).container.is_none()
                        && self.facts(*id).container_path.last().map(String::as_str) == Some(x)
                })
                .collect();
            if let Some(outcome) = Outcome::of(Basis::LexicalScope, literal) {
                return outcome;
            }
            return match self.local_types(local, file, depth) {
                Some(types) => {
                    self.on_types(&types, name, arity, Basis::ReceiverDeclaredType, file)
                }
                None => Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file)),
            };
        }
        if let Some(binding) = binding {
            // A default import of a module without a default export is the
            // object of the module, as a CommonJS module gives it.
            let object = binding.imported_name.as_deref() == Some("default")
                && !self.target_files(binding).iter().any(|target| {
                    self.files[*target]
                        .link_facts
                        .iter()
                        .any(|facts| facts.default_export)
                });
            let module = object
                || binding.kind == ImportKind::Module
                || binding.imported_name.as_deref() == Some("*");
            if module || self.target_files(binding).is_empty() {
                return self.imported(binding, name, arity);
            }
            // The imported name can be a namespace that the module exports
            // again, as `export * as name from "./file"` gives.
            let imported = binding.imported_name.as_deref();
            let spaces: Vec<usize> = self
                .target_files(binding)
                .into_iter()
                .flat_map(|target| &self.files[target].imports)
                .filter(|inner| {
                    inner.local_name.as_deref() == imported
                        && inner.imported_name.as_deref() == Some("*")
                })
                .flat_map(|inner| self.target_files(inner))
                .collect();
            if !spaces.is_empty() {
                let found: Vec<Id> = spaces
                    .iter()
                    .flat_map(|space| self.exported_name(*space, name, &mut BTreeSet::new()))
                    .collect();
                return Outcome::of(Basis::ImportedModule, self.preferred(found, arity))
                    .unwrap_or_else(|| Outcome::none(Status::Unresolved));
            }
        }
        let types = self.types(x, None, file);
        if types.is_empty() {
            return match self.own_field(file, chain, x) {
                Some((text, text_file)) => {
                    let types = self.types_of_text(text, text_file, false);
                    self.on_types(&types, name, arity, Basis::ReceiverDeclaredType, file)
                }
                None => Outcome::none(Status::Unresolved),
            };
        }
        self.on_types(&types, name, arity, Basis::QualifiedPath, file)
    }

    fn with_qualifier(
        &self,
        file: usize,
        reference: &Reference,
        qualifier: &str,
        chain: &[Id],
        start: usize,
    ) -> Outcome {
        let name = reference.name.as_str();
        // A C++ namespace alias can be the qualifier, or its first name.
        let mut expanded = None;
        if self.family(file) == Family::C {
            let (first, rest) = match qualifier.split_once("::") {
                Some((first, rest)) => (first, Some(rest)),
                None => (qualifier, None),
            };
            match self.namespace_alias(file, chain, first, start) {
                AliasLookup::None => {}
                AliasLookup::Namespace(namespace) => {
                    expanded = Some(match rest {
                        Some(rest) => format!("{namespace}::{rest}"),
                        None => namespace.to_string(),
                    });
                }
                AliasLookup::Uncertain => return Outcome::none(Status::Unresolved),
            }
        }
        let qualifier = expanded.as_deref().unwrap_or(qualifier);
        let path = segments(qualifier);
        let facts = &self.files[file];
        let binding = facts.imports.iter().find(|binding| {
            binding.local_name.as_deref() == Some(qualifier)
                || segments(&binding.module_path) == path
                || (path.len() == 1 && binding.local_name.as_deref() == Some(path[0]))
        });
        if let Some(binding) = binding {
            // The import can name a type, for example with an alias. Then
            // the call is a call of a member of that type.
            let imported = binding.imported_name.as_deref().unwrap_or(qualifier);
            let aliased: Vec<Id> = self
                .target_files(binding)
                .into_iter()
                .flat_map(|target| self.exported_name(target, imported, &mut BTreeSet::new()))
                .filter(|id| is_container(self.symbol(*id)))
                .collect();
            if !aliased.is_empty() {
                return self.on_types(&aliased, name, reference.arity, Basis::QualifiedPath, file);
            }
            let mut outcome = self.imported(binding, name, reference.arity);
            if outcome.status != Status::Unresolved || self.target_files(binding).is_empty() {
                outcome.basis = outcome.basis.map(|_| Basis::QualifiedPath);
                return outcome;
            }
        }
        if let Some(last) = path.last() {
            let before = (path.len() > 1).then(|| path[..path.len() - 1].join("."));
            let types = self.types(last, before.as_deref(), file);
            if !types.is_empty() {
                return self.on_types(&types, name, reference.arity, Basis::QualifiedPath, file);
            }
        }
        if let Some(target) = facts.path_targets.get(qualifier) {
            let found: Vec<Id> = self
                .by_file
                .get(target.as_str())
                .into_iter()
                .flat_map(|target| self.exported_name(*target, name, &mut BTreeSet::new()))
                .filter(|id| is_callable(self.symbol(*id)) || is_container(self.symbol(*id)))
                .collect();
            return Outcome::of(Basis::QualifiedPath, self.preferred(found, reference.arity))
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        let modules: Vec<usize> = (0..self.files.len())
            .filter(|other| {
                self.family(*other) == self.family(file)
                    && self.files[*other]
                        .modules
                        .iter()
                        .any(|module| segments(module) == path)
            })
            .collect();
        let found = self.preferred(self.top_level(name, &modules), reference.arity);
        Outcome::of(Basis::QualifiedPath, found)
            .unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }

    /// Resolves a call of a bare name. `start` is the start byte of the call.
    fn bare(&self, file: usize, reference: &Reference, chain: &[Id], start: usize) -> Outcome {
        let name = reference.name.as_str();
        // A local name is not in scope in its own first value or before it.
        let in_scope = |local: &Local| local.name.value_start.is_none_or(|value| value < start);
        let arity = reference.arity;
        let family = self.family(file);
        let callable = |id: &Id| {
            let symbol = self.symbol(*id);
            is_callable(symbol)
                || (matches!(family, Family::Python | Family::C) && is_container(symbol))
        };
        // A PHP variable and a PHP function do not share names. `$name()`
        // calls the function value that the local name holds.
        let php_value = |id: &Id| self.facts(*id).function_value;
        if let (Family::Php, Some(variable)) = (family, name.strip_prefix('$')) {
            let held: Vec<Id> = self
                .named_in(variable, file)
                .filter(|id| {
                    id.0 == file
                        && php_value(id)
                        && self
                            .facts(*id)
                            .parent
                            .is_some_and(|parent| chain.contains(&(file, parent)))
                })
                .collect();
            return Outcome::of(Basis::LexicalScope, held)
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        if family == Family::Ruby {
            // A Ruby name without parentheses is a value when the method
            // binds the name. A function value needs `.call`.
            let bound = self.locals_named(file, name).any(|local| {
                in_scope(local)
                    && local
                        .owner
                        .is_some_and(|owner| chain.contains(&(file, owner)))
            });
            if bound {
                return Outcome::none(Status::Unresolved);
            }
            // `new` in a class method makes an object of the class.
            if let Some(owner) = self.enclosing_type(chain).filter(|_| name == "new") {
                return Outcome::of(Basis::EnclosingType, self.constructors(owner, arity))
                    .unwrap_or_else(|| Outcome::none(Status::Unresolved));
            }
        }
        // Java cannot call a local variable with the syntax of a function call.
        if family != Family::Java {
            let lexical: Vec<Id> = self
                .named_in(name, file)
                .filter(|id| {
                    id.0 == file
                        && callable(id)
                        && !(family == Family::Php && php_value(id))
                        && self.facts(*id).parent.is_some_and(|parent| {
                            // A function of an enclosing function or module
                            // is in scope. A method of an enclosing type is not.
                            let scope = self.symbol((file, parent));
                            chain.contains(&(file, parent))
                                && (!is_container(scope) || scope.kind == SymbolKind::Module)
                        })
                })
                .collect();
            // A later definition of a name in one function hides an earlier
            // one. The call binds to the last definition before the call.
            let mut lexical = self.preferred(lexical, arity);
            let last_before = lexical
                .iter()
                .filter(|id| self.symbol(**id).range.start_byte <= start)
                .max_by_key(|id| self.symbol(**id).range.start_byte)
                .copied();
            if let (true, Some(last)) = (lexical.len() > 1, last_before) {
                lexical = vec![last];
            }
            if let Some(outcome) = Outcome::of(Basis::LexicalScope, lexical) {
                return outcome;
            }
            let value = self.locals_named(file, name).any(|local| {
                in_scope(local)
                    && local
                        .owner
                        .is_some_and(|owner| chain.contains(&(file, owner)))
            });
            if value {
                return Outcome::none(Status::Unresolved);
            }
        }
        if matches!(
            family,
            Family::Java | Family::CSharp | Family::C | Family::Ruby
        ) {
            if let Some(owner) = self.enclosing_type(chain) {
                let members = self.members(owner, name, arity, &mut BTreeSet::new());
                if let Some(outcome) = Outcome::of(Basis::EnclosingType, members) {
                    return outcome;
                }
            }
        }
        let mut same_file = self.preferred(
            self.top_level(name, &[file])
                .into_iter()
                .filter(callable)
                .collect(),
            arity,
        );
        // A later definition of a Bash function replaces an earlier one.
        if family == Family::Bash && same_file.len() > 1 {
            let last = same_file.iter().max().copied();
            let mut outcome = Outcome::offered(Basis::SameFile, same_file.clone());
            outcome.status = Status::UniqueCandidate;
            outcome.candidates = last.into_iter().collect();
            outcome.superseded = same_file
                .into_iter()
                .filter(|id| Some(*id) != last)
                .collect();
            return outcome;
        }
        let declared: Vec<Id> = same_file
            .iter()
            .copied()
            .filter(|id| family == Family::C && self.symbol(*id).body_range.is_none())
            .collect();
        same_file.retain(|id| self.symbol(*id).body_range.is_some() || family != Family::C);
        // A declaration that has no definition in the file declares a
        // different overload. Its definition, or the declaration, is a candidate.
        if !same_file.is_empty() && !declared.is_empty() {
            same_file.extend(self.definitions(declared.clone(), file, arity));
            same_file.sort();
            same_file.dedup();
        }
        if let Some(outcome) = Outcome::of(Basis::SameFile, same_file) {
            return outcome;
        }
        // A C file can declare a function of a different file itself, without
        // a header. The declaration in the file is the evidence, and the one
        // definition with a body is the callee.
        let linked: Vec<Id> = self
            .definitions(declared, file, arity)
            .into_iter()
            .filter(|id| self.symbol(*id).body_range.is_some())
            .collect();
        if let Some(outcome) = Outcome::of(Basis::SameFile, linked) {
            return outcome;
        }
        let binding = self.files[file].imports.iter().find(|binding| {
            binding.local_name.as_deref() == Some(name)
                && matches!(binding.kind, ImportKind::Named | ImportKind::ReExportNamed)
        });
        if let Some(binding) = binding {
            let imported = binding.imported_name.as_deref().unwrap_or(name);
            return self.imported(binding, imported, arity);
        }
        let package: Vec<usize> = self
            .package_files(file)
            .iter()
            .copied()
            .filter(|other| *other != file)
            .collect();
        let found: Vec<Id> = self
            .top_level(name, &package)
            .into_iter()
            .filter(callable)
            .collect();
        if let Some(outcome) = Outcome::of(Basis::SamePackage, self.preferred(found, arity)) {
            return outcome;
        }
        // A C++ `using namespace name;` is a local name `*` with the namespace as its type.
        let used: Vec<Id> = self.files[file]
            .locals
            .iter()
            .filter(|local| local.name.name == "*")
            .filter_map(|local| local.name.type_name.as_deref())
            .flat_map(|namespace| self.types(namespace, None, file))
            .flat_map(|namespace| self.members(namespace, name, arity, &mut BTreeSet::new()))
            .collect();
        if let Some(outcome) = Outcome::of(Basis::ImportedModule, used) {
            return outcome;
        }
        let visible = self.visible_files(file);
        let mut found: Vec<Id> = self
            .top_level(name, &visible)
            .into_iter()
            .filter(callable)
            .collect();
        // `using static Type` and `import static Type.*` give the members of a type.
        for binding in &self.files[file].imports {
            if binding.kind != ImportKind::Wildcard {
                continue;
            }
            let Some(owner) = segments(&binding.module_path).last().copied() else {
                continue;
            };
            for target in self.target_files(binding) {
                for owner in self.types(owner, None, target) {
                    if owner.0 == target {
                        found.extend(self.members(owner, name, arity, &mut BTreeSet::new()));
                    }
                }
            }
        }
        if !found.is_empty() {
            return Outcome::of(Basis::ImportedModule, self.definitions(found, file, arity))
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        let anywhere: Vec<Id> = self
            .named(name, file)
            .filter(|id| {
                // A definition that its file does not export is not a
                // candidate for a call from a different file.
                let visible = id.0 == file
                    || self.symbol(*id).exported != super::model::Exported::NotExported;
                callable(id)
                    && visible
                    && self.symbol(*id).container.is_none()
                    && self.facts(*id).parent.is_none()
            })
            .collect();
        let anywhere = self.definitions(anywhere, file, arity);
        // No rule with better evidence gave a definition. The definitions
        // with the name stay visible as candidates. A name is not sufficient
        // for an edge, also when only one definition has it: on real
        // repositories most edges of this kind were wrong.
        if !anywhere.is_empty() {
            return Outcome::offered(Basis::NameOnly, anywhere);
        }
        // A Ruby method calls a method of its object without `self`. A
        // module that other types include does not tell which type.
        let owner = self
            .enclosing_type(chain)
            .filter(|_| family == Family::Ruby);
        let Some(owner) = owner else {
            return Outcome::none(Status::Unresolved);
        };
        // The object is an object of a type that includes the module, or of
        // a subtype of such a type. One method for all these types is the
        // callee.
        let found: BTreeSet<Id> = self
            .including_types(owner)
            .into_iter()
            .flat_map(|includer| self.members(includer, name, arity, &mut BTreeSet::new()))
            .collect();
        match found.into_iter().collect::<Vec<Id>>().as_slice() {
            [only] => Outcome::of(Basis::EnclosingType, vec![*only])
                .unwrap_or_else(|| Outcome::none(Status::Unresolved)),
            _ => Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file)),
        }
    }

    /// The types that include the Ruby module `module`, and their subtypes.
    fn including_types(&self, module: Id) -> BTreeSet<Id> {
        let mut found = BTreeSet::new();
        if self.symbol(module).kind != SymbolKind::Module {
            return found;
        }
        let children = self.ruby_children.get_or_init(|| {
            let mut children: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
            for (file, facts) in self.files.iter().enumerate() {
                if self.family(file) != Family::Ruby {
                    continue;
                }
                for (position, symbol) in facts.symbols.iter().enumerate() {
                    if symbol.supertypes.is_empty() {
                        continue;
                    }
                    for parent in self.parents((file, position)) {
                        children.entry(parent).or_default().push((file, position));
                    }
                }
            }
            children
        });
        let mut pending = vec![module];
        while let Some(parent) = pending.pop() {
            for child in children.get(&parent).into_iter().flatten() {
                if found.insert(*child) {
                    pending.push(*child);
                }
            }
        }
        found
    }

    /// Replaces declarations by the definitions that they declare. A call
    /// binds to the definition, not to the declaration.
    fn definitions(&self, found: Vec<Id>, file: usize, arity: usize) -> Vec<Id> {
        let mut result = Vec::new();
        for id in found {
            let symbol = self.symbol(id);
            if symbol.body_range.is_some() {
                result.push(id);
                continue;
            }
            let path = &self.facts(id).container_path;
            // The language of the caller gives the linkage of the call. A
            // header that a C++ file includes is C++ in the index, also when
            // a C file includes it too.
            let c_call = self.files[file].language == Language::C;
            let bodies: Vec<Id> = self
                .named(&symbol.name, file)
                .filter(|other| {
                    let definition = self.symbol(*other);
                    definition.body_range.is_some()
                        && is_callable(definition)
                        && self.facts(*other).container_path == *path
                        && self.same_parameters(id, *other)
                        && (other.0 == file
                            || definition.exported != super::model::Exported::NotExported)
                        && (!c_call || self.has_c_linkage(*other))
                })
                .collect();
            if bodies.is_empty() {
                result.push(id);
            } else {
                result.extend(bodies);
            }
        }
        result.sort();
        result.dedup();
        self.preferred(result, arity)
    }

    /// Tells if a function has C linkage: it is C, it is in `extern "C"`, or
    /// an `extern "C"` declaration declares it. That declaration must be in
    /// the file of the definition or in a file that this file includes. A
    /// declaration in a different translation unit declares a different
    /// function.
    fn has_c_linkage(&self, id: Id) -> bool {
        if self.files[id.0].language == Language::C || self.facts(id).c_linkage {
            return true;
        }
        let included = self.visible_files(id.0);
        let name = &self.symbol(id).name;
        self.named(name, id.0).any(|other| {
            (other.0 == id.0 || included.contains(&other.0))
                && self.symbol(other).body_range.is_none()
                && self.facts(other).c_linkage
                && self.facts(other).container_path.is_empty()
        })
    }

    /// Selects one overload by the types of the arguments.
    ///
    /// The candidates must be overloads: they have one name, and the number
    /// of their parameters is the number of the arguments. An argument has a
    /// known type only when it is a literal or a local name with a declared
    /// type. The rule binds only when each argument at a position where the
    /// overloads differ has a known type, and exactly one overload has
    /// exactly those types there. The rule does not use a wider type, a boxed
    /// type, or a conversion. In each other case the call stays ambiguous.
    fn overload(&self, file: usize, position: usize, candidates: &[Id]) -> Option<Id> {
        let facts = &self.files[file].reference_facts[position];
        let arguments = &facts.arguments;
        let chain = self.chain(file, facts.owner);
        let mut overloads = Vec::new();
        for id in candidates {
            let types = self.facts(*id).parameter_types.as_ref()?;
            if types.len() != arguments.len()
                || self.symbol(*id).name != self.symbol(candidates[0]).name
            {
                return None;
            }
            overloads.push((*id, types));
        }
        let mut matching: Vec<Id> = candidates.to_vec();
        for (at, argument) in arguments.iter().enumerate() {
            let differs = overloads
                .iter()
                .any(|(_, types)| types[at] != overloads[0].1[at]);
            if !differs {
                continue;
            }
            let known = match argument {
                Argument::Literal(name) => name.clone(),
                Argument::Name(name) => {
                    let at = Some(facts.start_byte);
                    let declared = self
                        .local(&chain, file, name, at)?
                        .name
                        .type_name
                        .as_ref()?;
                    let declared: String = declared.split_whitespace().collect();
                    let declared = declared.trim_start_matches(':').to_string();
                    if ["var", "auto", "let"].contains(&declared.as_str()) {
                        return None;
                    }
                    declared
                }
                Argument::Other => return None,
            };
            matching.retain(|id| {
                overloads
                    .iter()
                    .any(|(overload, types)| overload == id && types[at] == known)
            });
        }
        match matching.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    fn resolve(&self, file: usize, position: usize, depth: usize) -> Outcome {
        let mut outcome = self.resolve_by_rules(file, position, depth);
        // A Python or C++ call of a class makes an object. It runs the constructor.
        if matches!(self.family(file), Family::Python | Family::C) {
            let arity = self.files[file].references[position].arity;
            outcome.candidates = outcome
                .candidates
                .iter()
                .flat_map(|id| {
                    if is_container(self.symbol(*id)) {
                        self.constructors(*id, arity)
                    } else {
                        vec![*id]
                    }
                })
                .collect();
            outcome.candidates.sort();
            outcome.candidates.dedup();
            if outcome.status == Status::UniqueCandidate && outcome.candidates.len() > 1 {
                outcome.status = Status::Ambiguous;
            }
        }
        // The argument types select one of the overloads that the rule found.
        // The basis stays the basis of that rule.
        if outcome.status == Status::Ambiguous && outcome.basis != Some(Basis::NameOnly) {
            if let Some(only) = self.overload(file, position, &outcome.candidates) {
                outcome.status = Status::UniqueCandidate;
                outcome.candidates = vec![only];
            }
        }
        outcome
    }

    fn resolve_by_rules(&self, file: usize, position: usize, depth: usize) -> Outcome {
        let facts = &self.files[file];
        let reference = &facts.references[position];
        let chain = self.chain(file, facts.reference_facts[position].owner);
        let name = reference.name.as_str();
        let arity = reference.arity;
        if reference.kind == ReferenceKind::Construct {
            // PHP `new self` and `new static` make an object of the enclosing class.
            let types = if ["self", "static"].contains(&name) {
                self.enclosing_type(&chain).into_iter().collect()
            } else {
                self.types(name, reference.qualifier.as_deref(), file)
            };
            let found: Vec<Id> = types
                .iter()
                .flat_map(|owner| self.constructors(*owner, arity))
                .collect();
            return Outcome::of(Basis::QualifiedPath, found)
                .unwrap_or_else(|| Outcome::none(Status::Unresolved));
        }
        match &reference.receiver {
            Some(Receiver::SelfValue) => {
                let Some(owner) = self.enclosing_type(&chain) else {
                    return Outcome::none(Status::Unresolved);
                };
                // C# `: this(...)` calls a different constructor of the class.
                if self.family(file) == Family::CSharp && name == "this" {
                    let found: Vec<Id> = self
                        .own_constructors(owner, arity)
                        .into_iter()
                        .filter(|id| Some(id) != chain.first())
                        .collect();
                    return Outcome::of(Basis::EnclosingType, found)
                        .unwrap_or_else(|| Outcome::none(Status::Unresolved));
                }
                // Ruby `self.new` in a class method makes an object of the class.
                if self.family(file) == Family::Ruby && name == "new" {
                    return Outcome::of(Basis::EnclosingType, self.constructors(owner, arity))
                        .unwrap_or_else(|| Outcome::none(Status::Unresolved));
                }
                let members = self.members(owner, name, arity, &mut BTreeSet::new());
                Outcome::of(Basis::EnclosingType, members).unwrap_or_else(|| {
                    Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file))
                })
            }
            Some(Receiver::Super) => {
                let Some(owner) = self.enclosing_type(&chain) else {
                    return Outcome::none(Status::Unresolved);
                };
                let parents = self.parents(owner);
                let method = chain.first().map(|id| self.symbol(*id));
                let found: Vec<Id> = if ["super", "base"].contains(&name) {
                    match method {
                        Some(method) if method.kind == SymbolKind::Constructor => parents
                            .iter()
                            .filter(|parent| {
                                !matches!(
                                    self.symbol(**parent).kind,
                                    SymbolKind::Interface | SymbolKind::Trait
                                )
                            })
                            .flat_map(|parent| self.constructors(*parent, arity))
                            .collect(),
                        Some(method) => parents
                            .iter()
                            .flat_map(|parent| {
                                self.members(*parent, &method.name, arity, &mut BTreeSet::new())
                            })
                            .collect(),
                        None => Vec::new(),
                    }
                } else {
                    parents
                        .iter()
                        .flat_map(|parent| self.members(*parent, name, arity, &mut BTreeSet::new()))
                        .collect()
                };
                Outcome::of(Basis::EnclosingType, found)
                    .unwrap_or_else(|| Outcome::none(Status::Unresolved))
            }
            Some(Receiver::Name(receiver)) => self.with_receiver(
                file,
                reference,
                &chain,
                receiver,
                facts.reference_facts[position].start_byte,
                depth,
            ),
            Some(Receiver::Expression) => {
                // A receiver that is a call has the type of the result of that call.
                let receiver = facts.reference_facts[position].receiver;
                let call = receiver.and_then(|(start, end)| {
                    self.references_at(file, start)
                        .iter()
                        .copied()
                        .find(|other| facts.reference_facts[*other].end_byte == end)
                });
                // A receiver that is a chain of names has the declared type
                // of its last field.
                let types = match call {
                    Some(call) => self.result_types(file, call, depth + 1),
                    // `values[i]` is one element of a declared slice, array,
                    // or list. A map gives no type.
                    None if facts.reference_facts[position].receiver_indexed => facts
                        .reference_facts[position]
                        .receiver_path
                        .as_ref()
                        .and_then(|path| {
                            let at = Some(facts.reference_facts[position].start_byte);
                            self.path_types(file, &chain, path, true, at, depth + 1)
                        }),
                    None => match facts.reference_facts[position].receiver_path.as_deref() {
                        // C++ `name.reset()` can call a smart pointer, and the
                        // extraction gives no name for the receiver. A member
                        // with a declared type that is not a pointer is an
                        // object, thus the call is a call of that type.
                        Some([name]) => self
                            .own_field(file, &chain, name)
                            .filter(|(declared, _)| !declared.contains('*'))
                            .map(|(declared, text_file)| {
                                self.types_of_text(declared, text_file, false)
                            }),
                        Some(path) => {
                            let at = Some(facts.reference_facts[position].start_byte);
                            // In C and C++ the member after `.` of a field that
                            // has a pointer type belongs to the pointer, which
                            // can be a smart pointer. It is not a member of the pointee.
                            let pointer = self.family(file) == Family::C
                                && !facts.reference_facts[position].arrow
                                && self.ends_in_pointer(file, &chain, path, at, depth);
                            if pointer {
                                None
                            } else {
                                self.path_types(file, &chain, path, false, at, depth + 1)
                            }
                        }
                        None => None,
                    },
                };
                match types {
                    Some(types) => {
                        self.on_types(&types, name, arity, Basis::ReceiverDeclaredType, file)
                    }
                    None => Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file)),
                }
            }
            None => match &reference.qualifier {
                Some(qualifier) => {
                    // A chain of names that starts at a local name or at the
                    // object is a value, not a path of a module.
                    let at = Some(facts.reference_facts[position].start_byte);
                    let path = name_path(qualifier).filter(|path| {
                        path.len() > 1
                            && (SELF_WORDS.contains(&path[0].as_str())
                                || self.local(&chain, file, &path[0], at).is_some()
                                || self.own_field(file, &chain, &path[0]).is_some())
                    });
                    match path {
                        Some(path) => {
                            match self.path_types(file, &chain, &path, false, at, depth + 1) {
                                Some(types) => self.on_types(
                                    &types,
                                    name,
                                    arity,
                                    Basis::ReceiverDeclaredType,
                                    file,
                                ),
                                None => Outcome::offered(
                                    Basis::NameOnly,
                                    self.methods_by_name(name, file),
                                ),
                            }
                        }
                        None => self.with_qualifier(
                            file,
                            reference,
                            qualifier,
                            &chain,
                            facts.reference_facts[position].start_byte,
                        ),
                    }
                }
                None => {
                    let start = facts.reference_facts[position].start_byte;
                    self.bare(file, reference, &chain, start)
                }
            },
        }
    }
}

/// Gives each reference its resolution. Call `link_files` and
/// `resolve_imports` first. The function puts `files` in path order, thus the
/// result does not depend on the order of the input.
pub fn resolve_references(files: &mut [FileSymbols]) {
    files.sort_by(|left, right| left.file.cmp(&right.file));
    let resolutions: Vec<Vec<Resolution>> = {
        let resolver = Resolver::new(files);
        (0..files.len())
            .into_par_iter()
            .map(|file| {
                (0..files[file].references.len())
                    .map(|position| {
                        let outcome = resolver.resolve(file, position, 0);
                        let mut candidates: Vec<String> = outcome
                            .candidates
                            .iter()
                            .map(|id| resolver.symbol(*id).key.clone())
                            .collect();
                        candidates.sort();
                        let mut superseded: Vec<String> = outcome
                            .superseded
                            .iter()
                            .map(|id| resolver.symbol(*id).key.clone())
                            .collect();
                        superseded.sort();
                        Resolution {
                            superseded,
                            status: outcome.status,
                            basis: outcome.basis,
                            candidates,
                        }
                    })
                    .collect()
            })
            .collect()
    };
    for (file, resolutions) in files.iter_mut().zip(resolutions) {
        for (reference, resolution) in file.references.iter_mut().zip(resolutions) {
            reference.resolution = resolution;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::rows;

    /// The rows of the calls of `name`.
    fn rows_of(files: &[(&str, &str)], file: &str, name: &str) -> Vec<String> {
        rows(files, file)
            .into_iter()
            .filter(|row| {
                let callee = row.split(' ').nth(1).unwrap_or_default();
                callee.ends_with(&format!(".{name}")) || callee.ends_with(&format!(":{name}"))
            })
            .map(|row| {
                let mut parts = row.splitn(3, ' ');
                let line = parts.next().unwrap_or_default();
                let outcome = parts.nth(1).unwrap_or_default();
                format!("{line} {outcome}")
            })
            .collect()
    }

    #[test]
    fn test_names_from_a_tuple_of_values_are_not_results_rust() {
        let source = "pub enum Query {\n    Nothing,\n}\n\nimpl Query {\n    fn or(left: u32) -> u32 {\n        left\n    }\n\n    fn union(&self) -> u32 {\n        use self::Query::*;\n        let (or, other) = (Query::or, 1);\n        or(other)\n    }\n}\n";
        assert_eq!(
            rows(&[("a.rs", source)], "a.rs"),
            ["13 Query.union>or UniqueCandidate ImportedModule [a.rs#Query.or]"]
        );
    }

    #[test]
    fn test_field_receiver_typescript() {
        let source = "class Item {\n  name(): string {\n    return \"\";\n  }\n}\n\ninterface Holder {\n  item: Item;\n}\n\nclass Cart {\n  private lead: Item;\n  private items: Item[];\n  private more: Array<Item>;\n  private maybe?: Item;\n  private opt: Item | undefined;\n  handler = (): number => 1;\n\n  constructor(private holder: Holder, plain: Holder) {}\n\n  run(h: Holder): void {\n    this.lead.name();\n    for (const item of this.items) {\n      item.name();\n    }\n    for (const other of this.more) {\n      other.name();\n    }\n    this.maybe.name();\n    this.opt.name();\n    h.item.name();\n    this.holder.item.name();\n    this.items.name();\n    this.plain.name();\n  }\n}\n";
        let found = "UniqueCandidate ReceiverDeclaredType [a.ts#Item.name]";
        assert_eq!(
            rows_of(&[("a.ts", source)], "a.ts", "name"),
            [
                format!("22 {found}"),
                format!("24 {found}"),
                format!("27 {found}"),
                format!("29 {found}"),
                format!("30 {found}"),
                format!("31 {found}"),
                format!("32 {found}"),
                "33 Unresolved - []".to_string(),
                "34 Ambiguous NameOnly [a.ts#Item.name]".to_string(),
            ]
        );
        // A field does not change the key of a function with the same name.
        let keys = crate::index::extract::testing::keys(crate::core::Language::TypeScript, source);
        assert!(keys.contains(&"file#Cart.handler".to_string()), "{keys:?}");
        assert!(!keys.iter().any(|key| key.contains('~')), "{keys:?}");
    }

    #[test]
    fn test_python_stub_is_the_module_when_no_source_file_exists() {
        let files = [
            ("pkg/__init__.py", ""),
            ("pkg/_native.pyi", "def fast() -> int: ...\n"),
            ("pkg/both.py", "def slow():\n    return 1\n"),
            ("pkg/both.pyi", "def slow() -> int: ...\n"),
            (
                "main.py",
                "from pkg._native import fast\nfrom pkg.both import slow\n\ndef run():\n    return fast() + slow()\n",
            ),
        ];
        assert_eq!(
            rows(&files, "main.py"),
            [
                "5 run>fast UniqueCandidate ImportedName [pkg/_native.pyi#fast]",
                "5 run>slow UniqueCandidate ImportedName [pkg/both.py#slow]",
            ]
        );
    }

    #[test]
    fn test_field_receiver_python() {
        let source = "class Item:\n    def name(self):\n        return \"\"\n\n\nclass Cart:\n    lead: Item\n    items: list[Item]\n    maybe: Optional[Item]\n    quoted: \"Item\"\n\n    def __init__(self):\n        self.made = Item()\n        self.typed: Item = build()\n        self.other = build()\n        self.count = 0\n\n    def run(self):\n        self.lead.name()\n        self.made.name()\n        self.typed.name()\n        self.other.name()\n        self.maybe.name()\n        self.quoted.name()\n        for item in self.items:\n            item.name()\n        self.items.name()\n        self.count.name()\n";
        let found = "UniqueCandidate ReceiverDeclaredType [a.py#Item.name]";
        assert_eq!(
            rows_of(&[("a.py", source)], "a.py", "name"),
            [
                format!("19 {found}"),
                format!("20 {found}"),
                format!("21 {found}"),
                "22 Unresolved - []".to_string(),
                format!("23 {found}"),
                format!("24 {found}"),
                format!("26 {found}"),
                "27 Unresolved - []".to_string(),
                "28 Ambiguous NameOnly [a.py#Item.name]".to_string(),
            ]
        );
    }

    #[test]
    fn test_index_of_a_declared_collection_gives_the_element() {
        let go = "package shop\n\ntype Item struct{}\n\nfunc (i *Item) Name() string { return \"\" }\n\ntype byName []*Item\n\nfunc (c byName) Less(i, j int) bool { return c[i].Name() < c[j].Name() }\n\ntype Cart struct {\n\titems []*Item\n\tnamed map[string]*Item\n}\n\nfunc (c *Cart) Run(list []*Item) {\n\tlist[0].Name()\n\tc.items[1].Name()\n\tc.named[\"a\"].Name()\n}\n";
        let found = "UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]";
        assert_eq!(
            rows_of(&[("a.go", go)], "a.go", "Name"),
            [
                format!("9 {found}"),
                format!("9 {found}"),
                format!("17 {found}"),
                format!("18 {found}"),
                "19 Unresolved - []".to_string(),
            ]
        );
        let typescript = "class Item {\n  name(): string {\n    return \"\";\n  }\n}\n\nfunction run(xs: Item[], m: Map<string, Item>): void {\n  xs[0].name();\n  m[\"a\"].name();\n}\n";
        assert_eq!(
            rows_of(&[("a.ts", typescript)], "a.ts", "name"),
            [
                "8 UniqueCandidate ReceiverDeclaredType [a.ts#Item.name]",
                "9 Unresolved - []",
            ]
        );
        let python = "class Item:\n    def name(self):\n        return \"\"\n\n\ndef run(xs: list[Item], d: dict[str, Item]):\n    xs[0].name()\n    d[\"a\"].name()\n";
        assert_eq!(
            rows_of(&[("a.py", python)], "a.py", "name"),
            [
                "7 UniqueCandidate ReceiverDeclaredType [a.py#Item.name]",
                "8 Unresolved - []",
            ]
        );
        let rust = "pub struct Item;\n\nimpl Item {\n    pub fn name(&self) -> u32 {\n        1\n    }\n}\n\nfn run(v: Vec<Item>, s: &[Item]) -> u32 {\n    v[0].name() + s[1].name()\n}\n";
        let found = "UniqueCandidate ReceiverDeclaredType [a.rs#Item.name]";
        assert_eq!(
            rows_of(&[("a.rs", rust)], "a.rs", "name"),
            [format!("10 {found}"), format!("10 {found}")]
        );
    }

    #[test]
    fn test_union_of_two_types_gives_no_edge() {
        let python = "class Item:\n    def name(self):\n        return \"\"\n\n\nclass Other:\n    pass\n\n\ndef run(\n    one: Item | Other,\n    many: list[\n        Other\n        | Item\n    ],\n    maybe: Item | None,\n):\n    one.name()\n    many[0].name()\n    maybe.name()\n";
        assert_eq!(
            rows_of(&[("a.py", python)], "a.py", "name"),
            [
                "18 Unresolved - []",
                "19 Unresolved - []",
                "20 UniqueCandidate ReceiverDeclaredType [a.py#Item.name]",
            ]
        );
        let typescript = "class Item {\n  name(): string {\n    return \"\";\n  }\n}\n\nclass Other {}\n\nfunction run(one: Item | Other, many: Array<Item | Other>): void {\n  one.name();\n  many[0].name();\n}\n";
        assert_eq!(
            rows_of(&[("a.ts", typescript)], "a.ts", "name"),
            ["10 Unresolved - []", "11 Unresolved - []"]
        );
    }

    #[test]
    fn test_field_receiver_rust() {
        let source = "pub struct Item;\n\nimpl Item {\n    pub fn name(&self) -> u32 {\n        1\n    }\n}\n\npub struct Cart {\n    lead: Item,\n    items: Vec<Item>,\n    boxed: Option<Item>,\n}\n\nimpl Cart {\n    fn find(&self) -> (Item, u32) {\n        (Item, 1)\n    }\n\n    fn load(&self) -> Result<Item, String> {\n        Ok(Item)\n    }\n\n    fn run(&self) -> Result<u32, String> {\n        self.lead.name();\n        for item in &self.items {\n            item.name();\n        }\n        let (found, _) = self.find();\n        found.name();\n        let loaded = self.load()?;\n        loaded.name();\n        let kept = self.load();\n        kept.name();\n        self.boxed.name();\n        self.items.name();\n        Ok(1)\n    }\n}\n";
        assert_eq!(
            rows_of(&[("a.rs", source)], "a.rs", "name"),
            [
                "25 UniqueCandidate ReceiverDeclaredType [a.rs#Item.name]",
                "27 UniqueCandidate ReceiverDeclaredType [a.rs#Item.name]",
                "30 UniqueCandidate ReceiverDeclaredType [a.rs#Item.name]",
                "32 UniqueCandidate ReceiverDeclaredType [a.rs#Item.name]",
                "34 Ambiguous NameOnly [a.rs#Item.name]",
                "35 Unresolved - []",
                "36 Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_rust() {
        let files = [
            ("Cargo.toml", "[package]\nname = \"shop\"\n"),
            ("src/lib.rs", "pub mod other;\npub mod deep;\npub mod caller;\n"),
            ("src/other.rs", "pub fn thing() {}\n\npub struct Cart;\n\nimpl Cart {\n    pub fn open() {}\n}\n"),
            ("src/deep/mod.rs", "pub mod inner;\n\npub fn thing() {}\n"),
            ("src/deep/inner.rs", "pub fn leaf() {\n    super::thing();\n    super::super::other::thing();\n}\n\npub mod nested {\n    pub fn twig() {}\n}\n"),
            (
                "src/caller.rs",
                "fn run() {\n    crate::other::thing();\n    crate::deep::inner::leaf();\n    crate::deep::inner::nested::twig();\n    crate::other::Cart::open();\n    crate::other::missing();\n    crate::other::Gone::open();\n    serde::other::thing();\n}\n",
            ),
            ("tests/it.rs", "fn check() {\n    shop::other::thing();\n    shop::deep::inner::leaf();\n}\n"),
        ];
        assert_eq!(
            rows(&files, "src/caller.rs"),
            [
                "2 run>crate::other::thing UniqueCandidate QualifiedPath [src/other.rs#thing]",
                "3 run>crate::deep::inner::leaf UniqueCandidate QualifiedPath [src/deep/inner.rs#leaf]",
                "4 run>crate::deep::inner::nested::twig UniqueCandidate QualifiedPath [src/deep/inner.rs#nested.twig]",
                "5 run>crate::other::Cart::open UniqueCandidate QualifiedPath [src/other.rs#Cart.open]",
                "6 run>crate::other::missing Unresolved - []",
                "7 run>crate::other::Gone::open Unresolved - []",
                "8 run>serde::other::thing Unresolved - []",
            ]
        );
        assert_eq!(
            rows(&files, "tests/it.rs"),
            [
                "2 check>shop::other::thing UniqueCandidate QualifiedPath [src/other.rs#thing]",
                "3 check>shop::deep::inner::leaf UniqueCandidate QualifiedPath [src/deep/inner.rs#leaf]",
            ]
        );
        assert_eq!(
            rows(&files, "src/deep/inner.rs"),
            [
                "2 leaf>super::thing UniqueCandidate QualifiedPath [src/deep/mod.rs#thing]",
                "3 leaf>super::super::other::thing UniqueCandidate QualifiedPath [src/other.rs#thing]",
            ]
        );
    }

    #[test]
    fn test_qualified_path_java() {
        let files = [
            ("a/Tool.java", "package shop.a;\n\npublic class Tool {\n    public static int run() {\n        return 1;\n    }\n}\n"),
            ("b/Tool.java", "package shop.b;\n\npublic class Tool {\n    public static int run() {\n        return 2;\n    }\n}\n"),
            ("c/Main.java", "package shop.c;\n\npublic class Main {\n    int go() {\n        return shop.b.Tool.run() + shop.gone.Tool.run();\n    }\n}\n"),
        ];
        assert_eq!(
            rows(&files, "c/Main.java"),
            [
                "5 Main.go>shop.b.Tool::run UniqueCandidate QualifiedPath [b/Tool.java#Tool.run]",
                "5 Main.go>shop.gone.Tool::run Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_csharp() {
        let files = [
            ("a/Tool.cs", "namespace Shop.A\n{\n    public static class Tool\n    {\n        public static int Run() { return 1; }\n    }\n}\n"),
            ("b/Tool.cs", "namespace Shop.B\n{\n    public static class Tool\n    {\n        public static int Run() { return 2; }\n    }\n}\n"),
            ("c/Main.cs", "namespace Shop.C\n{\n    public class Main\n    {\n        int Go() { return Shop.B.Tool.Run() + Shop.Gone.Tool.Run(); }\n    }\n}\n"),
        ];
        assert_eq!(
            rows(&files, "c/Main.cs"),
            [
                "5 Shop.C.Main.Go>Shop.B.Tool::Run UniqueCandidate QualifiedPath [b/Tool.cs#Shop.B.Tool.Run]",
                "5 Shop.C.Main.Go>Shop.Gone.Tool::Run Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_php() {
        let files = [
            ("a/tool.php", "<?php\nnamespace Shop\\A;\n\nfunction run() { return 1; }\n"),
            ("b/tool.php", "<?php\nnamespace Shop\\B;\n\nfunction run() { return 2; }\n"),
            ("c/main.php", "<?php\nnamespace Shop\\C;\n\nfunction go() {\n    return \\Shop\\B\\run() + \\Shop\\Gone\\run();\n}\n"),
        ];
        assert_eq!(
            rows(&files, "c/main.php"),
            [
                "5 go>\\Shop\\B::run UniqueCandidate QualifiedPath [b/tool.php#run]",
                "5 go>\\Shop\\Gone::run Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_cpp() {
        let files = [
            ("a.cpp", "namespace shop {\nnamespace a {\nint run() { return 1; }\n}\n}\n"),
            ("a.hpp", "namespace shop {\nnamespace a {\nint run();\n}\n}\n"),
            ("b.cpp", "namespace shop {\nnamespace b {\nint run() { return 2; }\n}\n}\n"),
            ("b.hpp", "namespace shop {\nnamespace b {\nint run();\n}\n}\n"),
            (
                "main.cpp",
                "#include \"a.hpp\"\n#include \"b.hpp\"\n\nint go() {\n    return shop::b::run() + shop::gone::run();\n}\n",
            ),
        ];
        assert_eq!(
            rows(&files, "main.cpp"),
            [
                "5 go>shop::b::run UniqueCandidate QualifiedPath [b.cpp#shop.b.run]",
                "5 go>shop::gone::run Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_python() {
        let files = [
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "def run():\n    return 1\n"),
            ("pkg/b.py", "def run():\n    return 2\n"),
            (
                "main.py",
                "import pkg.b\n\ndef go():\n    return pkg.b.run() + pkg.gone.run()\n",
            ),
        ];
        assert_eq!(
            rows(&files, "main.py"),
            [
                "4 go>pkg.b::run UniqueCandidate QualifiedPath [pkg/b.py#run]",
                "4 go>pkg.gone::run Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_qualified_path_go() {
        let files = [
            ("go.mod", "module example.com/shop\n"),
            ("a/tool.go", "package tool\n\nfunc Run() int { return 1 }\n"),
            ("b/tool.go", "package tool\n\nfunc Run() int { return 2 }\n"),
            ("main.go", "package main\n\nimport \"example.com/shop/b\"\n\nfunc run() int {\n\treturn tool.Run() + gone.Run()\n}\n"),
        ];
        assert_eq!(
            rows(&files, "main.go"),
            [
                "6 run>tool.Run UniqueCandidate ImportedModule [b/tool.go#Run]",
                "6 run>gone.Run Unresolved - []",
            ]
        );
    }

    /// The constructor call `Widget(1)` first finds the class. The overloads
    /// of the constructor then select one, or the call is ambiguous.
    #[test]
    fn test_constructor_overloads_select_one_or_are_ambiguous_cpp() {
        let source = "struct Widget {\n    Widget(int value) {}\n    Widget(long value) {}\n};\n\nvoid run(auto unknown) {\n    auto first = Widget(1);\n    auto second = Widget(unknown);\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            [
                "7 run>Widget UniqueCandidate SameFile [a.cpp#Widget.Widget]",
                "8 run>Widget Ambiguous SameFile [a.cpp#Widget.Widget, a.cpp#Widget.Widget~2]",
            ]
        );
    }

    /// The selection of an overload by the argument types keeps the basis of
    /// the rule that found the overloads.
    #[test]
    fn test_overload_selection_keeps_the_basis_java() {
        let source = "class Util {\n    static int parse(int value) { return value; }\n    static int parse(double value) { return 0; }\n\n    static int run() {\n        return parse(1);\n    }\n}\n";
        assert_eq!(
            rows(&[("Util.java", source)], "Util.java"),
            ["6 Util.run>parse UniqueCandidate EnclosingType [Util.java#Util.parse]"]
        );
    }

    #[test]
    fn test_overload_selection_keeps_the_basis_cpp() {
        let source = "int parse(int value) { return value; }\nint parse(double value) { return 0; }\n\nint run() {\n    return parse(1);\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            ["5 run>parse UniqueCandidate SameFile [a.cpp#parse]"]
        );
    }

    /// A qualifier that names a different namespace is not the only type with
    /// the name. The type of the repository is not the callee.
    #[test]
    fn test_qualifier_of_a_different_namespace_gives_no_edge_cpp() {
        let source = "namespace local {\nstruct Widget {\n    static int run() { return 1; }\n};\n}\n\nint go() {\n    return vendor::Widget::run() + local::Widget::run();\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            [
                "8 go>vendor::Widget::run Unresolved - []",
                "8 go>local::Widget::run UniqueCandidate QualifiedPath [a.cpp#local.Widget.run]",
            ]
        );
    }

    #[test]
    fn test_qualifier_of_a_different_package_gives_no_edge_java() {
        let widget = "package local;\n\npublic class Widget {\n    public static int run() { return 1; }\n}\n";
        let main = "package app;\n\nclass Main {\n    int go() {\n        return vendor.Widget.run() + local.Widget.run();\n    }\n}\n";
        assert_eq!(
            rows(&[("local/Widget.java", widget), ("app/Main.java", main)], "app/Main.java"),
            [
                "5 Main.go>vendor.Widget::run Unresolved - []",
                "5 Main.go>local.Widget::run UniqueCandidate QualifiedPath [local/Widget.java#Widget.run]",
            ]
        );
    }

    /// A C declaration has C linkage. A C++ definition without `extern "C"`
    /// is a different function, also with the same name.
    #[test]
    fn test_c_declaration_does_not_link_to_a_cpp_definition() {
        let header =
            "void shared(void);\nvoid exported(void);\nvoid blocked(void);\nvoid declared(void);\n";
        let caller = "#include \"api.h\"\n\nint main(void) {\n    shared();\n    exported();\n    blocked();\n    declared();\n    return 0;\n}\n";
        let cpp = "void shared() {}\n\nextern \"C\" void exported() {}\n\nextern \"C\" {\nvoid blocked() {}\n}\n\nextern \"C\" void declared();\n\nvoid declared() {}\n";
        let files = [("api.h", header), ("main.c", caller), ("impl.cpp", cpp)];
        assert_eq!(
            rows(&files, "main.c"),
            [
                "4 main>shared UniqueCandidate ImportedModule [api.h#shared]",
                "5 main>exported UniqueCandidate ImportedModule [impl.cpp#exported]",
                "6 main>blocked UniqueCandidate ImportedModule [impl.cpp#blocked]",
                "7 main>declared UniqueCandidate ImportedModule [impl.cpp#declared~2]",
            ]
        );
    }

    /// A namespace alias at the start of a qualifier names its namespace.
    #[test]
    fn test_namespace_alias_before_a_nested_type_cpp() {
        let source = "namespace vendor {\nstruct Widget {\n    static int run() { return 1; }\n};\n}\n\nnamespace v = vendor;\n\nint go() {\n    return v::Widget::run();\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            ["10 go>v::Widget::run UniqueCandidate QualifiedPath [a.cpp#vendor.Widget.run]"]
        );
    }

    /// The two namespaces of the alias tests, and the file alias `v` of `one`.
    const ALIAS_TYPES: &str = "namespace one {\nstruct Widget {\n    static int run() { return 1; }\n};\n}\n\nnamespace two {\nstruct Widget {\n    static int run() { return 2; }\n};\n}\n\nnamespace v = one;\n";

    /// The resolver does not model the C++ name lookup fully. When more
    /// than one alias with the name is visible at the call, the call has no
    /// edge. An alias in a scope that does not contain the call is not visible.
    #[test]
    fn test_namespace_alias_of_the_nearest_scope_cpp() {
        let source = format!("{ALIAS_TYPES}\nnamespace scope {{\nnamespace v = two;\n\nint go() {{\n    return v::Widget::run();\n}}\n}}\n\nint outer() {{\n    return v::Widget::run();\n}}\n\nint local() {{\n    namespace v = two;\n    return v::Widget::run();\n}}\n");
        assert_eq!(
            rows(&[("a.cpp", &source)], "a.cpp"),
            [
                "19 scope.go>v::Widget::run Unresolved - []",
                "24 outer>v::Widget::run UniqueCandidate QualifiedPath [a.cpp#one.Widget.run]",
                "29 local>v::Widget::run Unresolved - []",
            ]
        );
    }

    /// An alias in one block of a namespace is visible only in that block.
    /// The file alias is then the only visible alias.
    #[test]
    fn test_namespace_alias_in_a_second_block_of_the_namespace_cpp() {
        let reopened = format!("{ALIAS_TYPES}\nnamespace scope {{\nnamespace v = two;\n}}\n\nnamespace scope {{\nint go() {{\n    return v::Widget::run();\n}}\n}}\n");
        assert_eq!(
            rows(&[("a.cpp", &reopened)], "a.cpp"),
            ["21 scope.go>v::Widget::run UniqueCandidate QualifiedPath [a.cpp#one.Widget.run]"]
        );
    }

    /// A parameter is not a namespace alias. A parameter with the name of a
    /// visible alias makes the alias uncertain.
    #[test]
    fn test_parameter_is_not_a_namespace_alias_cpp() {
        let source = "namespace two {\nint run() { return 2; }\n}\n\nint go(two A) {\n    return A::run();\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            ["6 go>A::run Unresolved - []"]
        );
        let shadowed =
            format!("{ALIAS_TYPES}\nint go(two v) {{\n    return v::Widget::run();\n}}\n");
        assert_eq!(
            rows(&[("a.cpp", &shadowed)], "a.cpp"),
            ["16 go>v::Widget::run Unresolved - []"]
        );
    }

    /// An alias is not visible before its declaration.
    #[test]
    fn test_namespace_alias_after_the_call_cpp() {
        let source = format!("{ALIAS_TYPES}\nnamespace scope {{\nint go() {{\n    return v::Widget::run();\n}}\n\nnamespace v = two;\n}}\n");
        assert_eq!(
            rows(&[("a.cpp", &source)], "a.cpp"),
            ["17 scope.go>v::Widget::run UniqueCandidate QualifiedPath [a.cpp#one.Widget.run]"]
        );
    }

    /// An alias in a block of a function is visible only in that block.
    #[test]
    fn test_namespace_alias_in_a_block_cpp() {
        let source = format!("{ALIAS_TYPES}\nint go() {{\n    {{\n        namespace v = two;\n        v::Widget::run();\n    }}\n    return v::Widget::run();\n}}\n");
        assert_eq!(
            rows(&[("a.cpp", &source)], "a.cpp"),
            [
                "18 go>v::Widget::run Unresolved - []",
                "20 go>v::Widget::run UniqueCandidate QualifiedPath [a.cpp#one.Widget.run]",
            ]
        );
    }

    /// An `extern "C"` declaration gives C linkage only to the definition
    /// that it declares: a definition in its file, or in a file that
    /// includes it. A C++ definition in a different translation unit keeps
    /// C++ linkage.
    #[test]
    fn test_extern_c_declaration_marks_only_its_own_definition() {
        let header = "void shared(void);\nvoid linked(void);\n";
        let caller = "#include \"api.h\"\n\nint main(void) {\n    shared();\n    linked();\n    return 0;\n}\n";
        let user = "extern \"C\" void shared();\n\nvoid use() {\n    shared();\n}\n";
        let unrelated = "void shared() {}\n";
        let library_header = "extern \"C\" void linked();\n";
        let library = "#include \"lib.hpp\"\n\nvoid linked() {}\n";
        let files = [
            ("api.h", header),
            ("main.c", caller),
            ("user.cpp", user),
            ("unrelated.cpp", unrelated),
            ("lib.hpp", library_header),
            ("lib.cpp", library),
        ];
        assert_eq!(
            rows(&files, "main.c"),
            [
                "4 main>shared UniqueCandidate ImportedModule [api.h#shared]",
                "5 main>linked UniqueCandidate ImportedModule [lib.cpp#linked]",
            ]
        );
    }

    /// Two local classes with one name in one file are different classes.
    /// The constructor of one is not the constructor of the other.
    #[test]
    fn test_constructors_of_same_named_local_classes_python() {
        let source = "def first():\n    class Mock:\n        def __init__(self):\n            pass\n\n    return Mock()\n\n\ndef second():\n    class Mock:\n        def __init__(self):\n            pass\n\n    return Mock()\n";
        assert_eq!(
            rows(&[("a.py", source)], "a.py"),
            [
                "6 first>Mock UniqueCandidate LexicalScope [a.py#first.Mock.__init__]",
                "14 second>Mock UniqueCandidate LexicalScope [a.py#second.Mock.__init__]",
            ]
        );
    }

    /// Two types with one name in one file are different types. A member of
    /// one is not a member of the other.
    #[test]
    fn test_members_of_same_named_types_in_one_file_cpp() {
        let source = "#include <base>\n\nnamespace a {\nstruct X {\n    void ping() {}\n};\n}\n\nnamespace b {\nstruct X : Base {\n    void run() {\n        this->ping();\n    }\n};\n}\n";
        assert_eq!(
            rows(&[("a.cpp", source)], "a.cpp"),
            ["12 b.X.run>self.ping Ambiguous NameOnly [a.cpp#a.X.ping]"]
        );
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::index::imports::testing::index;

    /// One line for each call in `file`:
    /// `line caller>callee status basis [candidate keys]`.
    pub(crate) fn rows(files: &[(&str, &str)], file: &str) -> Vec<String> {
        let mut facts = index(files);
        resolve_references(&mut facts);
        let facts = facts.iter().find(|facts| facts.file == file).unwrap();
        facts
            .references
            .iter()
            .map(|reference| {
                let caller = reference
                    .enclosing
                    .as_deref()
                    .and_then(|key| key.split_once('#'))
                    .map_or("-", |(_, name)| name);
                let callee = match (&reference.receiver, &reference.qualifier) {
                    (Some(Receiver::SelfValue), _) => format!("self.{}", reference.name),
                    (Some(Receiver::Super), _) => format!("super.{}", reference.name),
                    (Some(Receiver::Name(name)), _) => format!("{name}.{}", reference.name),
                    (Some(Receiver::Expression), _) => format!("(..).{}", reference.name),
                    (None, Some(qualifier)) => format!("{qualifier}::{}", reference.name),
                    (None, None) => reference.name.clone(),
                };
                let basis = reference
                    .resolution
                    .basis
                    .map_or("-".to_string(), |basis| format!("{basis:?}"));
                format!(
                    "{} {caller}>{callee} {:?} {basis} [{}]",
                    reference.name_range.start_line,
                    reference.resolution.status,
                    reference.resolution.candidates.join(", ")
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod false_edge_tests {
    use super::testing::rows;

    /// The rows of the calls of `name`.
    fn rows_of(files: &[(&str, &str)], file: &str, name: &str) -> Vec<String> {
        rows(files, file)
            .into_iter()
            .filter(|row| {
                let callee = row.split(' ').nth(1).unwrap_or_default();
                callee.ends_with(&format!(".{name}"))
                    || callee.ends_with(&format!(":{name}"))
                    || callee.ends_with(&format!(">{name}"))
            })
            .collect()
    }

    #[test]
    fn test_var_names_pair_with_their_values_go() {
        let source = "package a\n\ntype First struct{}\n\nfunc (f First) run() {}\n\ntype Second struct{}\n\nfunc (s Second) run() {}\n\nfunc makeFirst() First { return First{} }\n\nfunc makeSecond() Second { return Second{} }\n\nfunc main() {\n\tvar first, second = makeFirst(), makeSecond()\n\tfirst.run()\n\tsecond.run()\n}\n";
        assert_eq!(
            rows_of(&[("a.go", source)], "a.go", "run"),
            [
                "17 main>first.run UniqueCandidate ReceiverDeclaredType [a.go#First.run]",
                "18 main>second.run UniqueCandidate ReceiverDeclaredType [a.go#Second.run]",
            ]
        );
    }

    #[test]
    fn test_export_alias_of_the_same_file_resolves_the_public_name() {
        for (language, library, user) in [
            ("ts", "function original(): void {}\n\nexport { original as publicName };\n", "import { publicName } from './lib';\n\nfunction run(): void {\n  publicName();\n}\n"),
            ("js", "function original() {}\n\nexport { original as publicName };\n", "import { publicName } from './lib';\n\nfunction run() {\n  publicName();\n}\n"),
        ] {
            let lib = format!("lib.{language}");
            let main = format!("main.{language}");
            let rows = rows(&[(lib.as_str(), library), (main.as_str(), user)], &main);
            assert_eq!(
                rows,
                [format!(
                    "4 run>publicName UniqueCandidate ImportedName [{lib}#original]"
                )]
            );
        }
    }

    #[test]
    fn test_grouped_go_result_names_share_the_type() {
        let source = "package a\n\ntype Item struct{}\n\nfunc (i *Item) Name() {}\n\nfunc pair() (first, second *Item) { return nil, nil }\n\nfunc run() {\n\ta, b := pair()\n\ta.Name()\n\tb.Name()\n}\n";
        let found = "UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]";
        assert_eq!(
            rows_of(&[("a.go", source)], "a.go", "Name"),
            [
                format!("11 run>a.Name {found}"),
                format!("12 run>b.Name {found}")
            ]
        );
    }

    #[test]
    fn test_defined_go_type_owns_methods() {
        let source = "package a\n\ntype Count int\n\nfunc (c Count) Inc() {}\n\nfunc run() {\n\tvar c Count\n\tc.Inc()\n}\n";
        assert_eq!(
            rows_of(&[("a.go", source)], "a.go", "Inc"),
            ["9 run>c.Inc UniqueCandidate ReceiverDeclaredType [a.go#Count.Inc]"]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_go() {
        let source = "package a\n\ntype A struct{}\n\nfunc (a A) Method() {}\n\ntype B struct{}\n\nfunc (b B) Method() {}\n\nfunc run() {\n\tx := A{}\n\t{\n\t\tx := B{}\n\t\tx.Method()\n\t}\n\tx.Method()\n}\n";
        assert_eq!(
            rows_of(&[("a.go", source)], "a.go", "Method"),
            [
                "15 run>x.Method UniqueCandidate ReceiverDeclaredType [a.go#B.Method]",
                "17 run>x.Method UniqueCandidate ReceiverDeclaredType [a.go#A.Method]",
            ]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_rust() {
        let source = "struct A;\nimpl A {\n    fn method(&self) {}\n}\nstruct B;\nimpl B {\n    fn method(&self) {}\n}\n\nfn run() {\n    let x: A = A;\n    {\n        let x: B = B;\n        x.method();\n    }\n    x.method();\n}\n";
        assert_eq!(
            rows_of(&[("a.rs", source)], "a.rs", "method"),
            [
                "14 run>x.method UniqueCandidate ReceiverDeclaredType [a.rs#B.method]",
                "16 run>x.method UniqueCandidate ReceiverDeclaredType [a.rs#A.method]",
            ]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_java() {
        let source = "class A {\n    void method() {}\n}\n\nclass B {\n    void method() {}\n}\n\nclass Main {\n    void run() {\n        A x = new A();\n        {\n            B x = new B();\n            x.method();\n        }\n        x.method();\n    }\n}\n";
        assert_eq!(
            rows_of(&[("Main.java", source)], "Main.java", "method"),
            [
                "14 Main.run>x.method UniqueCandidate ReceiverDeclaredType [Main.java#B.method]",
                "16 Main.run>x.method UniqueCandidate ReceiverDeclaredType [Main.java#A.method]",
            ]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_csharp() {
        let source = "class A {\n    public void Method() {}\n}\n\nclass B {\n    public void Method() {}\n}\n\nclass Main {\n    void Run() {\n        A x = new A();\n        {\n            B x = new B();\n            x.Method();\n        }\n        x.Method();\n    }\n}\n";
        assert_eq!(
            rows_of(&[("Main.cs", source)], "Main.cs", "Method"),
            [
                "14 Main.Run>x.Method UniqueCandidate ReceiverDeclaredType [Main.cs#B.Method]",
                "16 Main.Run>x.Method UniqueCandidate ReceiverDeclaredType [Main.cs#A.Method]",
            ]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_cpp() {
        let source = "struct A {\n    void method() {}\n};\n\nstruct B {\n    void method() {}\n};\n\nvoid run() {\n    A x;\n    {\n        B x;\n        x.method();\n    }\n    x.method();\n}\n";
        assert_eq!(
            rows_of(&[("a.cpp", source)], "a.cpp", "method"),
            [
                "13 run>x.method UniqueCandidate ReceiverDeclaredType [a.cpp#B.method]",
                "15 run>x.method UniqueCandidate ReceiverDeclaredType [a.cpp#A.method]",
            ]
        );
    }

    #[test]
    fn test_a_local_in_an_inner_block_hides_the_outer_local_typescript() {
        let source = "class A {\n  method(): void {}\n}\n\nclass B {\n  method(): void {}\n}\n\nfunction run(): void {\n  const x: A = new A();\n  {\n    const x: B = new B();\n    x.method();\n  }\n  x.method();\n}\n";
        assert_eq!(
            rows_of(&[("a.ts", source)], "a.ts", "method"),
            [
                "13 run>x.method UniqueCandidate ReceiverDeclaredType [a.ts#B.method]",
                "15 run>x.method UniqueCandidate ReceiverDeclaredType [a.ts#A.method]",
            ]
        );
    }

    #[test]
    fn test_python_write_of_an_unknown_value_removes_the_field_type() {
        let source = "class Item:\n    def name(self):\n        return \"\"\n\n\nclass Holder:\n    def __init__(self, replacement):\n        self.value = Item()\n        self.value = replacement\n\n    def run(self):\n        self.value.name()\n";
        let rows = rows_of(&[("a.py", source)], "a.py", "name");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].contains("UniqueCandidate"), "{rows:?}");
    }

    #[test]
    fn test_php_qualified_new_does_not_use_an_unrelated_import() {
        let files = [
            ("a.php", "<?php\nnamespace A;\n\nclass Item {}\n"),
            ("b.php", "<?php\nnamespace B;\n\nclass Item {}\n"),
            (
                "c.php",
                "<?php\nnamespace App;\n\nuse B\\Item;\n\nfunction run() {\n    return new \\A\\Item();\n}\n",
            ),
        ];
        let rows = rows(&files, "c.php");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].contains("[a.php#Item]"), "{rows:?}");
    }

    #[test]
    fn test_qualified_new_does_not_use_an_unrelated_import_java() {
        let files = [
            ("a/Item.java", "package a;\n\npublic class Item {}\n"),
            ("b/Item.java", "package b;\n\npublic class Item {}\n"),
            (
                "c/Main.java",
                "package c;\n\nimport b.Item;\n\nclass Main {\n    void run() {\n        new a.Item();\n    }\n}\n",
            ),
        ];
        let rows = rows(&files, "c/Main.java");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[a/Item.java#Item]"), "{rows:?}");
    }

    #[test]
    fn test_ruby_prepended_module_comes_before_the_class() {
        let source = "module Loud\n  def name\n    \"loud\"\n  end\nend\n\nclass Base\n  prepend Loud\n\n  def name\n    \"base\"\n  end\n\n  def run\n    self.name\n  end\nend\n";
        let rows = rows_of(&[("a.rb", source)], "a.rb", "name");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[a.rb#Loud.name]"), "{rows:?}");
    }

    #[test]
    fn test_ruby_included_module_comes_after_the_class() {
        let source = "module Quiet\n  def name\n    \"quiet\"\n  end\nend\n\nclass Base\n  include Quiet\n\n  def name\n    \"base\"\n  end\n\n  def run\n    self.name\n  end\nend\n";
        let rows = rows_of(&[("a.rb", source)], "a.rb", "name");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[a.rb#Base.name]"), "{rows:?}");
    }

    #[test]
    fn test_ruby_operator_assignment_binds_a_local() {
        let source = "def helper\n  1\nend\n\ndef run\n  helper ||= 0\n  helper\nend\n\ndef count\n  total = 1\n  total += 1\n  total\nend\n\ndef total\n  2\nend\n\ndef bump\n  total += 1\n  total\nend\n";
        assert_eq!(
            rows(&[("a.rb", source)], "a.rb"),
            [
                "7 run>helper Unresolved - []",
                "13 count>total Unresolved - []",
                "22 bump>total Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_rust_macro_that_gives_text_does_not_call() {
        let source = "fn helper() -> u32 {\n    1\n}\n\nfn main() {\n    let text = stringify!(helper());\n    println!(\"{}\", helper());\n}\n";
        assert_eq!(
            rows(&[("a.rs", source)], "a.rs"),
            ["7 main>helper UniqueCandidate SameFile [a.rs#helper]"]
        );
    }

    #[test]
    fn test_rust_super_in_an_inline_module_keeps_the_module() {
        let source = "fn target() {}\n\nmod outer {\n    pub fn target() {}\n\n    mod inner {\n        use super::target as chosen;\n\n        pub fn run() {\n            chosen();\n        }\n    }\n}\n";
        let rows = rows(&[("a.rs", source)], "a.rs");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[a.rs#outer.target]"), "{rows:?}");
    }

    #[test]
    fn test_rust_turbofish_call_is_a_call() {
        let source = "fn helper<T>() {}\n\nstruct Item;\n\nimpl Item {\n    fn run<T>(&self) {}\n\n    fn make<T>() {}\n}\n\nfn main(item: Item) {\n    helper::<u8>();\n    item.run::<u8>();\n    Item::make::<u8>();\n}\n";
        assert_eq!(
            rows(&[("a.rs", source)], "a.rs"),
            [
                "12 main>helper UniqueCandidate SameFile [a.rs#helper]",
                "13 main>item.run UniqueCandidate ReceiverDeclaredType [a.rs#Item.run]",
                "14 main>Item::make UniqueCandidate QualifiedPath [a.rs#Item.make]",
            ]
        );
    }

    #[test]
    fn test_rust_impl_of_a_qualified_type_keeps_the_path() {
        let source = "mod a {\n    pub struct Node;\n}\n\nmod b {\n    pub struct Node;\n}\n\nimpl a::Node {\n    fn run(&self) {}\n}\n\nimpl b::Node {\n    fn run(&self) {}\n}\n\nfn go(node: b::Node) {\n    node.run();\n}\n";
        let keys = crate::index::extract::testing::keys(crate::core::Language::Rust, source);
        assert!(keys.contains(&"file#a.Node.run".to_string()), "{keys:?}");
        assert!(keys.contains(&"file#b.Node.run".to_string()), "{keys:?}");
        let rows = rows(&[("a.rs", source)], "a.rs");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[a.rs#b.Node.run]"), "{rows:?}");
    }

    #[test]
    fn test_rust_declared_type_with_a_module_path_resolves() {
        let files = [
            ("Cargo.toml", "[package]\nname = \"shop\"\n"),
            ("src/lib.rs", "pub mod shapes;\npub mod other;\n"),
            (
                "src/shapes.rs",
                "pub struct Circle;\n\nimpl Circle {\n    pub fn area(&self) {}\n}\n",
            ),
            (
                "src/other.rs",
                "pub struct Pair {\n    lead: crate::shapes::Circle,\n    list: Vec<shapes::Circle>,\n}\n\nimpl Pair {\n    fn run(&self, given: crate::shapes::Circle) {\n        self.lead.area();\n        given.area();\n    }\n}\n",
            ),
        ];
        let found = "UniqueCandidate ReceiverDeclaredType [src/shapes.rs#Circle.area]";
        assert_eq!(
            rows_of(&files, "src/other.rs", "area"),
            [
                format!("8 Pair.run>(..).area {found}"),
                format!("9 Pair.run>given.area {found}")
            ]
        );
    }

    #[test]
    fn test_rust_qualifier_of_a_missing_module_gives_no_edge() {
        let source = "struct Widget;\n\nimpl Widget {\n    fn run() {}\n}\n\nfn main() {\n    vendor::Widget::run();\n}\n";
        let rows = rows(&[("a.rs", source)], "a.rs");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].contains("UniqueCandidate"), "{rows:?}");
    }

    #[test]
    fn test_explicit_import_of_an_outside_type_does_not_use_a_local_type_java() {
        let files = [
            (
                "Main.java",
                "package app;\n\nimport vendor.Widget;\n\nclass Main {\n    void run() {\n        new Widget();\n    }\n}\n",
            ),
            ("Widget.java", "package local;\n\npublic class Widget {}\n"),
        ];
        let rows = rows(&files, "Main.java");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].contains("UniqueCandidate"), "{rows:?}");
    }

    #[test]
    fn test_a_declaration_of_another_overload_stays_cpp() {
        let source = "int parse(int);\n\nint parse(double value) {\n    return 1;\n}\n\nint run() {\n    return parse(1);\n}\n";
        let rows = rows(&[("a.cpp", source)], "a.cpp");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].contains("parse~2"), "{rows:?}");
    }

    #[test]
    fn test_static_import_selects_an_overload_by_arity_java() {
        let files = [
            (
                "Util.java",
                "package u;\n\npublic class Util {\n    public static void parse(int a) {}\n\n    public static void parse(int a, int b) {}\n}\n",
            ),
            (
                "Main.java",
                "package m;\n\nimport static u.Util.parse;\n\nclass Main {\n    void run() {\n        parse(1);\n    }\n}\n",
            ),
        ];
        let rows = rows(&files, "Main.java");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].contains("UniqueCandidate"), "{rows:?}");
        assert!(rows[0].ends_with("[Util.java#Util.parse]"), "{rows:?}");
    }

    #[test]
    fn test_member_of_a_function_value_is_not_an_invocation() {
        let source = "function run() {\n  const handler = () => 1;\n  handler.call(null);\n  handler.toString();\n}\n";
        let rows = rows(&[("a.js", source)], "a.js");
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].ends_with("[a.js#run.handler]"), "{rows:?}");
        assert!(!rows[1].contains("run.handler"), "{rows:?}");
    }

    #[test]
    fn test_java_array_dimensions_after_the_name_select_the_overload() {
        let source = "class Item {}\n\nclass Main {\n    void choose(Item a) {}\n\n    void choose(Item[] a) {}\n\n    void run() {\n        Item values[] = {};\n        choose(values);\n    }\n}\n";
        let rows = rows(&[("Main.java", source)], "Main.java");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[Main.java#Main.choose~2]"), "{rows:?}");
    }

    #[test]
    fn test_java_varargs_parameter_has_an_array_type() {
        let source = "class Item {\n    void name() {}\n}\n\nclass Main {\n    void run(Item... items) {\n        for (var item : items) {\n            item.name();\n        }\n    }\n}\n";
        assert_eq!(
            rows_of(&[("Main.java", source)], "Main.java", "name"),
            ["8 Main.run>item.name UniqueCandidate ReceiverDeclaredType [Main.java#Item.name]"]
        );
    }

    #[test]
    fn test_cpp_call_on_a_value_member_chain_is_kept() {
        let source = "struct Item {\n    void reset() {}\n};\n\nstruct Holder {\n    Item plain_;\n    std::unique_ptr<Item> smart_;\n\n    void run() {\n        this->plain_.reset();\n        this->smart_.reset();\n    }\n};\n";
        let rows = rows(&[("a.cpp", source)], "a.cpp");
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].contains("UniqueCandidate"), "{rows:?}");
        assert!(rows[0].ends_with("[a.cpp#Item.reset]"), "{rows:?}");
        assert!(!rows[1].contains("UniqueCandidate"), "{rows:?}");
    }

    #[test]
    fn test_csharp_this_initializer_calls_the_other_constructor() {
        let source =
            "class Item {\n    public Item(int a) {}\n\n    public Item() : this(1) {}\n}\n";
        let rows = rows(&[("Item.cs", source)], "Item.cs");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].ends_with("[Item.cs#Item.Item]"), "{rows:?}");
    }
}
