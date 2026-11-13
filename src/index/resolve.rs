//! Call resolution. The rules run in a fixed order, and the first rule that
//! applies gives the basis. A rule that needs a guess gives no edge.
//!
//! The order is: a lexical name, a member of the object itself or of its
//! parent type, a qualifier, a receiver with a known type, and then a bare
//! name. A qualifier or a receiver that fails does not continue to the rules
//! for a bare name.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

use super::calls::{split_type, Argument};
use super::extract::{FileSymbols, LinkFacts, Local};
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
    /// For each Ruby type, the types that have it as a direct parent type.
    ruby_children: OnceLock<BTreeMap<Id, Vec<Id>>>,
}

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
        Self {
            files,
            by_name,
            by_file,
            ruby_children: OnceLock::new(),
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
    fn visible_files(&self, file: usize) -> Vec<usize> {
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
        // A name in a module in the file, as a Rust `mod testing { ... }` has it.
        let in_module: Vec<Id> = self
            .named(name, file)
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
        let mut found = self.types_by_name(name, qualifier, file);
        // More than one type can have the name. The qualifier selects the
        // types whose containers have the names of the qualifier, or whose
        // file declares the package that the qualifier names.
        if let Some(path) = qualifier.map(segments).filter(|_| found.len() > 1) {
            let within = |id: &Id| {
                let containers: Vec<&str> = self
                    .facts(*id)
                    .container_path
                    .iter()
                    .flat_map(|container| segments(container))
                    .collect();
                let declared = self.files[id.0]
                    .modules
                    .iter()
                    .any(|module| segments(module) == path);
                declared || containers.ends_with(&path)
            };
            if found.iter().any(within) {
                found.retain(within);
            }
        }
        found
    }

    fn types_by_name(&self, name: &str, qualifier: Option<&str>, file: usize) -> Vec<Id> {
        let is_type = |id: &Id| is_container(self.symbol(*id));
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
        for binding in &facts.imports {
            if binding.local_name.as_deref() != Some(name) {
                continue;
            }
            let imported = binding.imported_name.as_deref().unwrap_or(name);
            for target in self.target_files(binding) {
                found.extend(
                    self.exported_name(target, imported, &mut BTreeSet::new())
                        .into_iter()
                        .filter(is_type),
                );
            }
        }
        if !found.is_empty() {
            return found;
        }
        let package: Vec<usize> = (0..self.files.len())
            .filter(|other| self.same_package(file, *other))
            .collect();
        let found = in_files(&package);
        if !found.is_empty() {
            return found;
        }
        let found = in_files(&self.visible_files(file));
        if !found.is_empty() {
            return found;
        }
        self.named(name, file).filter(is_type).collect()
    }

    /// The callable members `name` of a type. The search goes to the parent
    /// types when the type has no such member.
    fn members(&self, owner: Id, name: &str, arity: usize, seen: &mut BTreeSet<Id>) -> Vec<Id> {
        if !seen.insert(owner) {
            return Vec::new();
        }
        let type_symbol = self.symbol(owner);
        let namespace = type_symbol.kind == SymbolKind::Module;
        let mut found: Vec<Id> = self
            .named(name, owner.0)
            .filter(|id| {
                let member = self.symbol(*id);
                is_callable(member)
                    && member.container.as_deref() == Some(type_symbol.name.as_str())
                    // Two types can have one name. A member in a different
                    // file belongs to this type only when that file gives
                    // this type for the name.
                    && (namespace
                        || id.0 == owner.0
                        || self.types(&type_symbol.name, None, id.0) == [owner])
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
        keep(&mut found, &|id| self.symbol(*id).body_range.is_some());
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
        let found: Vec<Id> = (0..self.files[owner.0].symbols.len())
            .map(|position| (owner.0, position))
            .chain(
                self.named(&type_symbol.name, owner.0)
                    .filter(|id| id.0 != owner.0 && self.symbol(*id).receiver_type.is_some()),
            )
            .filter(|id| {
                let member = self.symbol(*id);
                member.kind == SymbolKind::Constructor
                    && member.container.as_deref() == Some(type_symbol.name.as_str())
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

    fn local(&self, chain: &[Id], file: usize, name: &str) -> Option<&'a Local> {
        let owners = chain.iter().map(|id| Some(id.1)).chain([None]);
        for owner in owners {
            let found = self.files[file]
                .locals
                .iter()
                .find(|local| local.owner == owner && local.name.name == name);
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
        if let Some(type_name) = &local.name.type_name {
            let (qualifier, name) = split_type(type_name);
            if !name.is_empty() && !["var", "auto", "let", "_"].contains(&name.as_str()) {
                return Some(self.types(&name, qualifier.as_deref(), file));
            }
        }
        let start = local.name.value_start?;
        let position = self.files[file]
            .reference_facts
            .iter()
            .position(|reference| reference.start_byte == start)?;
        self.result_types(file, position, depth)
    }

    /// The types of the value that a call gives. `None` means that the syntax
    /// does not give a type: the call has no one callee, or the callee has no
    /// declared result type.
    fn result_types(&self, file: usize, position: usize, depth: usize) -> Option<Vec<Id>> {
        let facts = &self.files[file];
        let reference = &facts.references[position];
        if reference.kind == ReferenceKind::Construct {
            return Some(self.types(&reference.name, reference.qualifier.as_deref(), file));
        }
        if depth > 4 {
            return None;
        }
        // Ruby `self.class` gives the class of the object.
        if self.family(file) == Family::Ruby
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
        if symbol.kind == SymbolKind::Constructor {
            return own_type();
        }
        if is_container(symbol) {
            return Some(vec![*callee]);
        }
        let (qualifier, name) = split_type(self.facts(*callee).return_type.as_deref()?);
        if ["Self", "self", "static"].contains(&name.as_str()) {
            return own_type();
        }
        Some(self.types(&name, qualifier.as_deref(), callee.0))
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
        let is_abstract = |id: &Id| self.symbol(*id).body_range.is_none();
        if !members.is_empty() && members.iter().all(is_abstract) {
            return Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file));
        }
        Outcome::of(basis, members).unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }

    /// The namespace for which a C++ `namespace alias = target;` gives a name.
    /// The extraction records the alias as a local name with the target as its type.
    fn namespace_alias(&self, file: usize, alias: &str) -> Option<&'a str> {
        self.files[file]
            .locals
            .iter()
            .find(|local| {
                local.owner.is_none()
                    && local.name.name == alias
                    && local.name.value_start.is_none()
            })
            .and_then(|local| local.name.type_name.as_deref())
    }

    fn imported(&self, binding: &ImportBinding, name: &str) -> Outcome {
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
        let mut found: Vec<Id> = targets
            .iter()
            .flat_map(|target| self.exported_name(*target, name, &mut BTreeSet::new()))
            .filter(|id| is_callable(self.symbol(*id)) || is_container(self.symbol(*id)))
            .collect();
        if found.is_empty() {
            // A static member, as Java `import static a.Type.member` gives.
            let path = segments(&binding.module_path);
            if let [.., owner, _] = path.as_slice() {
                found = targets
                    .iter()
                    .flat_map(|target| self.types(owner, None, *target))
                    .flat_map(|owner| self.members(owner, name, usize::MAX, &mut BTreeSet::new()))
                    .collect();
            }
        }
        Outcome::of(basis, self.preferred(found, usize::MAX))
            .unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }

    fn with_receiver(
        &self,
        file: usize,
        reference: &Reference,
        chain: &[Id],
        x: &str,
        depth: usize,
    ) -> Outcome {
        let name = reference.name.as_str();
        let arity = reference.arity;
        // A function value that a local name holds.
        let closure = self.named(x, file).find(|id| {
            id.0 == file
                && is_callable(self.symbol(*id))
                && self.symbol(*id).container.is_some()
                && self
                    .facts(*id)
                    .parent
                    .is_some_and(|parent| chain.contains(&(file, parent)))
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
            .local(chain, file, x)
            .filter(|local| binding.is_none() || local.owner.is_some());
        if let Some(local) = local {
            // The members of an object literal have the name of the variable in their key.
            let literal: Vec<Id> = self
                .named(name, file)
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
                return self.imported(binding, name);
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
            return Outcome::none(Status::Unresolved);
        }
        self.on_types(&types, name, arity, Basis::QualifiedPath, file)
    }

    fn with_qualifier(&self, file: usize, reference: &Reference, qualifier: &str) -> Outcome {
        let name = reference.name.as_str();
        let qualifier = match self.family(file) {
            Family::C => self.namespace_alias(file, qualifier).unwrap_or(qualifier),
            _ => qualifier,
        };
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
            let mut outcome = self.imported(binding, name);
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
        let in_scope = |local: &Local| !local.name.value_start.is_some_and(|value| value >= start);
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
                .named(variable, file)
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
            let bound = self.files[file].locals.iter().any(|local| {
                local.name.name == name
                    && in_scope(local)
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
                .named(name, file)
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
            let value = self.files[file].locals.iter().any(|local| {
                local.name.name == name
                    && in_scope(local)
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
        same_file.retain(|id| self.symbol(*id).body_range.is_some() || family != Family::C);
        if let Some(outcome) = Outcome::of(Basis::SameFile, same_file) {
            return outcome;
        }
        let binding = self.files[file].imports.iter().find(|binding| {
            binding.local_name.as_deref() == Some(name)
                && matches!(binding.kind, ImportKind::Named | ImportKind::ReExportNamed)
        });
        if let Some(binding) = binding {
            let imported = binding.imported_name.as_deref().unwrap_or(name);
            return self.imported(binding, imported);
        }
        let package: Vec<usize> = (0..self.files.len())
            .filter(|other| *other != file && self.same_package(file, *other))
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
        Outcome::of(Basis::NameOnly, self.definitions(anywhere, file, arity)).unwrap_or_else(|| {
            // A Ruby method calls a method of its object without `self`. A
            // module that other types include does not tell which type.
            let owner = self
                .enclosing_type(chain)
                .filter(|_| family == Family::Ruby);
            let Some(owner) = owner else {
                return Outcome::none(Status::Unresolved);
            };
            // The object is an object of a type that includes the module, or
            // of a subtype of such a type. One method for all these types is
            // the callee.
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
        })
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
            let bodies: Vec<Id> = self
                .named(&symbol.name, file)
                .filter(|other| {
                    let definition = self.symbol(*other);
                    definition.body_range.is_some()
                        && is_callable(definition)
                        && self.facts(*other).container_path == *path
                        && (other.0 == file
                            || definition.exported != super::model::Exported::NotExported)
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
                    let declared = self.local(&chain, file, name)?.name.type_name.as_ref()?;
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
        if outcome.status == Status::Ambiguous && outcome.basis != Some(Basis::NameOnly) {
            if let Some(only) = self.overload(file, position, &outcome.candidates) {
                outcome = Outcome {
                    status: Status::UniqueCandidate,
                    basis: Some(Basis::ArgumentType),
                    candidates: vec![only],
                    superseded: Vec::new(),
                };
            }
        }
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
            Some(Receiver::Name(receiver)) => {
                self.with_receiver(file, reference, &chain, receiver, depth)
            }
            Some(Receiver::Expression) => {
                // A receiver that is a call has the type of the result of that call.
                let receiver = facts.reference_facts[position].receiver;
                let call = receiver.and_then(|(start, end)| {
                    facts
                        .reference_facts
                        .iter()
                        .position(|other| other.start_byte == start && other.end_byte == end)
                });
                match call.and_then(|call| self.result_types(file, call, depth + 1)) {
                    Some(types) => {
                        self.on_types(&types, name, arity, Basis::ReceiverDeclaredType, file)
                    }
                    None => Outcome::offered(Basis::NameOnly, self.methods_by_name(name, file)),
                }
            }
            None => match &reference.qualifier {
                Some(qualifier) => self.with_qualifier(file, reference, qualifier),
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
                "5 Main.go>shop.gone.Tool::run Ambiguous NameOnly [a/Tool.java#Tool.run, b/Tool.java#Tool.run]",
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
                "5 Shop.C.Main.Go>Shop.Gone.Tool::Run Ambiguous NameOnly [a/Tool.cs#Shop.A.Tool.Run, b/Tool.cs#Shop.B.Tool.Run]",
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
