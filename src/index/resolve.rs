//! Call resolution. The rules run in a fixed order, and the first rule that
//! applies gives the basis. A rule that needs a guess gives no edge.
//!
//! The order is: a lexical name, a member of the object itself or of its
//! parent type, a qualifier, a receiver with a known type, and then a bare
//! name. A qualifier or a receiver that fails does not continue to the rules
//! for a bare name.

use std::collections::{BTreeSet, HashMap};

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
}

impl Outcome {
    fn none(status: Status) -> Self {
        Self {
            status,
            basis: None,
            candidates: Vec::new(),
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
                    && (namespace
                        || id.0 == owner.0
                        || member.receiver_type.is_some()
                        || self.same_package(id.0, owner.0))
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
        keep(&mut found, &|id| {
            self.facts(*id).parameter_count == Some(arity)
        });
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
        let facts = &self.files[file];
        let position = facts
            .reference_facts
            .iter()
            .position(|reference| reference.start_byte == start)?;
        let reference = &facts.references[position];
        if reference.kind == ReferenceKind::Construct {
            return Some(self.types(&reference.name, reference.qualifier.as_deref(), file));
        }
        if depth > 4 {
            return None;
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
        Outcome::of(basis, members).unwrap_or_else(|| Outcome::none(Status::Unresolved))
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
        }
        let types = self.types(x, None, file);
        if types.is_empty() {
            return Outcome::none(Status::Unresolved);
        }
        self.on_types(&types, name, arity, Basis::QualifiedPath, file)
    }

    fn with_qualifier(&self, file: usize, reference: &Reference, qualifier: &str) -> Outcome {
        let name = reference.name.as_str();
        let path = segments(qualifier);
        let joined = |separator: &str| path.join(separator);
        let facts = &self.files[file];
        let binding = facts.imports.iter().find(|binding| {
            binding.local_name.as_deref() == Some(qualifier)
                || segments(&binding.module_path) == path
                || (path.len() == 1 && binding.local_name.as_deref() == Some(path[0]))
        });
        if let Some(binding) = binding {
            let mut outcome = self.imported(binding, name);
            if outcome.status != Status::Unresolved || self.target_files(binding).is_empty() {
                outcome.basis = outcome.basis.map(|_| Basis::QualifiedPath);
                return outcome;
            }
        }
        if let Some(last) = path.last() {
            let before = (path.len() > 1).then(|| joined("."));
            let types = self.types(last, before.as_deref(), file);
            if !types.is_empty() {
                return self.on_types(&types, name, reference.arity, Basis::QualifiedPath, file);
            }
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

    fn bare(&self, file: usize, reference: &Reference, chain: &[Id]) -> Outcome {
        let name = reference.name.as_str();
        let arity = reference.arity;
        let family = self.family(file);
        let callable = |id: &Id| {
            let symbol = self.symbol(*id);
            is_callable(symbol) || (family == Family::Python && is_container(symbol))
        };
        // Java cannot call a local variable with the syntax of a function call.
        if family != Family::Java {
            let lexical: Vec<Id> = self
                .named(name, file)
                .filter(|id| {
                    id.0 == file
                        && callable(id)
                        && self.facts(*id).parent.is_some_and(|parent| {
                            chain.contains(&(file, parent))
                                && !is_container(self.symbol((file, parent)))
                        })
                })
                .collect();
            if let Some(outcome) = Outcome::of(Basis::LexicalScope, self.preferred(lexical, arity))
            {
                return outcome;
            }
            let value = self.files[file].locals.iter().any(|local| {
                local.name.name == name
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
        Outcome::of(Basis::NameOnly, self.definitions(anywhere, file, arity))
            .unwrap_or_else(|| Outcome::none(Status::Unresolved))
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
                };
            }
        }
        // A Python call of a class makes an object. It runs the constructor.
        if self.family(file) == Family::Python {
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
            let types = self.types(name, reference.qualifier.as_deref(), file);
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
            Some(Receiver::Expression) => Outcome::none(Status::Unresolved),
            None => match &reference.qualifier {
                Some(qualifier) => self.with_qualifier(file, reference, qualifier),
                None => self.bare(file, reference, &chain),
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
                        Resolution {
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
