//! The matcher. It reads the references of the index after import resolution,
//! and the syntax tree for the sinks that are not calls.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use rayon::prelude::*;
use tree_sitter::Node;

use super::spec::{specs_for, syntax_of, LanguageSyntax, Predicate, SinkSpec, Target};
use super::syntax;
use super::{Sink, SinkBasis, SinkCategory, SinkFlags};
use crate::core::{is_test_file, Language};
use crate::index::extract::{language_spec, FileSymbols, Local};
use crate::index::model::{
    ImportBinding, ImportKind, ImportTarget, Receiver, Reference, ResolutionBasis,
    ResolutionStatus, Span, SymbolKind,
};
use crate::parser::{cfg_test_module_ranges, ParseResult};

/// The sinks of the files, in file order and then in source order.
///
/// `parsed[i]` is the syntax tree of `files[i]`. Give the files after
/// `resolve_imports` and `resolve_references`.
pub fn find_sinks(parsed: &[ParseResult], files: &[FileSymbols]) -> Vec<Sink> {
    assert_eq!(parsed.len(), files.len(), "each file needs its syntax tree");
    let repository = Repository::new(files);
    let per_file: Vec<Vec<Sink>> = (0..files.len())
        .into_par_iter()
        .map(|position| file_sinks(&parsed[position], &files[position], &repository))
        .collect();
    per_file.into_iter().flatten().collect()
}

/// The names that the repository defines. A field and a variable are not names of a module or a type.
struct Repository<'a> {
    by_file: HashMap<&'a str, HashSet<&'a str>>,
    by_directory: HashMap<&'a str, HashSet<&'a str>>,
    all: HashSet<&'a str>,
}

fn directory(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

impl<'a> Repository<'a> {
    fn new(files: &'a [FileSymbols]) -> Self {
        let mut repository = Self {
            by_file: HashMap::new(),
            by_directory: HashMap::new(),
            all: HashSet::new(),
        };
        for file in files {
            for symbol in &file.symbols {
                if matches!(symbol.kind, SymbolKind::Field | SymbolKind::Variable) {
                    continue;
                }
                let name = symbol.name.as_str();
                repository
                    .by_file
                    .entry(&file.file)
                    .or_default()
                    .insert(name);
                repository
                    .by_directory
                    .entry(directory(&file.file))
                    .or_default()
                    .insert(name);
                repository.all.insert(name);
            }
        }
        repository
    }

    fn defines_in_files(&self, files: &[String], name: &str) -> bool {
        files.iter().any(|file| {
            self.by_file
                .get(file.as_str())
                .is_some_and(|names| names.contains(name))
        })
    }
}

/// The segments of a module path or a type path, whatever the separators are.
fn split_path(text: &str) -> Vec<String> {
    text.split(['.', ':', '/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// The path of a declared type: `&mut sql.DB` gives `sql`, `DB`.
fn type_segments(text: &str) -> Option<Vec<String>> {
    let mut rest = text.trim();
    loop {
        let stripped = rest
            .strip_prefix(['&', '*'])
            .or_else(|| rest.strip_prefix("mut "))
            .or_else(|| rest.strip_prefix("const "))
            .or_else(|| rest.strip_prefix("dyn "));
        match stripped {
            Some(inner) => rest = inner.trim_start(),
            None => break,
        }
    }
    let rest = rest.split('<').next().unwrap_or("");
    let rest = rest.trim_end_matches(['?', ']', '[']).trim();
    if rest.is_empty()
        || rest.contains(char::is_whitespace)
        || matches!(rest, "var" | "auto" | "let")
        || !rest
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '$'))
    {
        return None;
    }
    Some(split_path(rest))
}

/// The segments of a receiver that is a chain of names and calls without arguments.
fn chain_segments(text: &str) -> Option<Vec<String>> {
    if text.trim_start().starts_with("new ") {
        return None;
    }
    let compact: String = text.split_whitespace().collect();
    let mut segments = Vec::new();
    for part in compact.split(['.', ':']).filter(|part| !part.is_empty()) {
        let name = part.strip_suffix("()").unwrap_or(part);
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
        {
            return None;
        }
        segments.push(part.to_string());
    }
    Some(segments)
}

/// The type that `new Type(arguments)` makes, when the text is only that expression.
fn constructed_type(text: &str) -> Option<Vec<String>> {
    let rest = text.trim().strip_prefix("new ")?.trim_start();
    let open = rest.find(['(', '<', '{'])?;
    if !rest[open..].starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    let mut close = None;
    for (offset, character) in rest[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    if !rest[close? + 1..].trim().is_empty() {
        return None;
    }
    chain_segments(rest[..open].trim())
}

/// The loaders of PyYAML that build arbitrary objects.
const UNSAFE_LOADERS: [&str; 6] = [
    "Loader",
    "UnsafeLoader",
    "FullLoader",
    "CLoader",
    "CUnsafeLoader",
    "CFullLoader",
];

/// The owner of a written path after the imports of the file.
enum Owner {
    /// A repository definition has the name.
    Repository,
    /// The head is a variable that has no known type.
    Unknown,
    /// The paths that the written path can mean. `imported` tells that an import gave them.
    Paths {
        paths: Vec<Vec<String>>,
        imported: bool,
    },
}

struct FileContext<'a> {
    parsed: &'a ParseResult,
    file: &'a FileSymbols,
    repository: &'a Repository<'a>,
    syntax: LanguageSyntax,
    specs: Vec<&'static SinkSpec>,
    source: &'a [u8],
    test_ranges: Vec<Range<usize>>,
    call_kinds: Vec<&'static str>,
}

fn file_sinks(parsed: &ParseResult, file: &FileSymbols, repository: &Repository<'_>) -> Vec<Sink> {
    let language = file.language;
    let source = parsed.source.as_slice();
    let test_ranges = if language == Language::Rust {
        let text = std::str::from_utf8(source).unwrap_or("");
        cfg_test_module_ranges(&parsed.root_node(), text)
    } else {
        Vec::new()
    };
    let context = FileContext {
        parsed,
        file,
        repository,
        syntax: syntax_of(language),
        specs: specs_for(language).collect(),
        source,
        test_ranges,
        call_kinds: language_spec(language).call_syntax().call_kinds(),
    };
    let mut sinks = Vec::new();
    context.reference_sinks(&mut sinks);
    context.tree_sinks(parsed.root_node(), &mut sinks);
    sinks.sort_by_key(|sink| (sink.range.start_byte, sink.category));
    sinks.dedup_by_key(|sink| (sink.range.start_byte, sink.category));
    sinks
}

impl FileContext<'_> {
    fn in_test(&self, start_byte: usize) -> bool {
        is_test_file(&self.file.file)
            || self
                .test_ranges
                .iter()
                .any(|range| range.contains(&start_byte))
    }

    /// The symbol positions that contain a function, from the inner one to the outer one.
    fn owner_chain(&self, owner: Option<usize>) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut next = owner;
        while let Some(position) = next {
            if chain.contains(&position) {
                break;
            }
            chain.push(position);
            next = self
                .file
                .link_facts
                .get(position)
                .and_then(|facts| facts.parent);
        }
        chain
    }

    /// The innermost function that contains the byte.
    fn enclosing_position(&self, byte: usize) -> Option<usize> {
        self.file
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                ) && symbol.range.start_byte <= byte
                    && byte < symbol.range.end_byte
            })
            .min_by_key(|(_, symbol)| symbol.range.end_byte - symbol.range.start_byte)
            .map(|(position, _)| position)
    }

    fn find_local(&self, name: &str, chain: &[usize]) -> Option<&Local> {
        let named = |local: &&Local| local.name.name == name;
        chain
            .iter()
            .find_map(|owner| {
                self.file
                    .locals
                    .iter()
                    .rev()
                    .filter(named)
                    .find(|local| local.owner == Some(*owner))
            })
            .or_else(|| {
                self.file
                    .locals
                    .iter()
                    .rev()
                    .filter(named)
                    .find(|local| local.owner.is_none())
            })
    }

    fn binds(binding: &ImportBinding, name: &str) -> bool {
        matches!(
            binding.kind,
            ImportKind::Module
                | ImportKind::Named
                | ImportKind::ReExportNamed
                | ImportKind::ModuleDeclaration
        ) && binding.local_name.as_deref() == Some(name)
    }

    fn is_repository_target(target: &ImportTarget) -> bool {
        matches!(
            target,
            ImportTarget::Resolved(_) | ImportTarget::Package(_) | ImportTarget::Ambiguous(_)
        )
    }

    /// The path that an import gives to the name that it binds.
    fn binding_prefix(binding: &ImportBinding, local: &str) -> Vec<String> {
        let segments = split_path(&binding.module_path);
        match (binding.kind, binding.imported_name.as_deref()) {
            (ImportKind::Module, Some(imported))
                if imported == local && segments.first().map(String::as_str) == Some(local) =>
            {
                vec![local.to_string()]
            }
            (ImportKind::Named | ImportKind::ReExportNamed, Some(imported))
                if !matches!(imported, "default" | "*") =>
            {
                if segments.last().map(String::as_str) == Some(imported) {
                    segments
                } else {
                    let mut with_name = segments;
                    with_name.extend(split_path(imported));
                    with_name
                }
            }
            _ => segments,
        }
    }

    fn defined_near(&self, name: &str) -> bool {
        let here = self
            .repository
            .by_file
            .get(self.file.file.as_str())
            .is_some_and(|names| names.contains(name));
        let near = self.syntax.same_directory_names
            && self
                .repository
                .by_directory
                .get(directory(&self.file.file))
                .is_some_and(|names| names.contains(name));
        here || near
    }

    fn is_root(&self, head: &str) -> bool {
        self.syntax.roots.contains(&head)
            || (self.syntax.roots.contains(&"*") && !matches!(head, "crate" | "self" | "super"))
    }

    fn resolve(&self, written: &[String], chain: &[usize]) -> Owner {
        let mut written = written.to_vec();
        for _ in 0..2 {
            let Some(local) = self.find_local(&written[0], chain) else {
                break;
            };
            if written.len() < 2 {
                return Owner::Unknown;
            }
            match local.name.type_name.as_deref().and_then(type_segments) {
                Some(declared) => {
                    written.splice(0..1, declared);
                }
                None => return Owner::Unknown,
            }
        }
        let head = written[0].clone();
        let rest = &written[1..];
        let imports = &self.file.imports;
        if let Some(binding) = imports.iter().find(|binding| Self::binds(binding, &head)) {
            if binding.kind == ImportKind::ModuleDeclaration
                || Self::is_repository_target(&binding.target_file)
            {
                return Owner::Repository;
            }
            let mut path = Self::binding_prefix(binding, &head);
            path.extend(rest.iter().cloned());
            return Owner::Paths {
                paths: vec![path],
                imported: true,
            };
        }
        let wildcards: Vec<&ImportBinding> = imports
            .iter()
            .filter(|binding| binding.kind == ImportKind::Wildcard)
            .collect();
        let repository_wildcard = wildcards.iter().any(|binding| match &binding.target_file {
            ImportTarget::Resolved(file) => self
                .repository
                .defines_in_files(std::slice::from_ref(file), &head),
            ImportTarget::Package(files) | ImportTarget::Ambiguous(files) => {
                self.repository.defines_in_files(files, &head)
            }
            ImportTarget::External | ImportTarget::Unresolved => false,
        });
        if repository_wildcard || self.defined_near(&head) {
            return Owner::Repository;
        }
        let external: Vec<Vec<String>> = wildcards
            .iter()
            .filter(|binding| !Self::is_repository_target(&binding.target_file))
            .map(|binding| {
                let mut path = split_path(&binding.module_path);
                path.extend(written.iter().cloned());
                path
            })
            .collect();
        if !external.is_empty() {
            return Owner::Paths {
                paths: external,
                imported: true,
            };
        }
        if self.is_root(&head) && self.repository.all.contains(head.as_str()) {
            return Owner::Repository;
        }
        Owner::Paths {
            paths: vec![written],
            imported: false,
        }
    }

    /// The resolver of the index selected a repository definition for the call.
    fn resolver_selected_repository(&self, reference: &Reference) -> bool {
        if !matches!(
            reference.resolution.status,
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
        ) {
            return false;
        }
        self.syntax.global_namespace
            || !matches!(
                reference.resolution.basis,
                None | Some(ResolutionBasis::NameOnly | ResolutionBasis::QualifiedPath)
            )
    }

    /// The names before the callee, as the source writes them. The outer
    /// `None` means that the call has no sink: its receiver is the object itself.
    /// The inner `None` means that the receiver is an expression that is not a chain of names.
    fn written_path(
        reference: &Reference,
        receiver_text: Option<&str>,
    ) -> Option<Option<Vec<String>>> {
        let mut path = match &reference.receiver {
            Some(Receiver::SelfValue | Receiver::Super) => return None,
            Some(Receiver::Name(name)) => vec![name.clone()],
            Some(Receiver::Expression) => match receiver_text
                .and_then(|text| chain_segments(text).or_else(|| constructed_type(text)))
            {
                Some(chain) => chain,
                None => return Some(None),
            },
            None => reference
                .qualifier
                .as_deref()
                .map(split_path)
                .unwrap_or_default(),
        };
        path.push(reference.name.clone());
        Some(Some(path))
    }

    /// The modules that the language gives without an import, each as segments.
    fn implicit_modules(&self) -> Vec<Vec<String>> {
        self.syntax
            .implicit_modules
            .iter()
            .map(|module| split_path(module))
            .collect()
    }

    /// Tells how a path matches a row, if it does.
    fn path_match(
        &self,
        module: &str,
        callee: &str,
        path: &[String],
        imported: bool,
    ) -> Option<SinkBasis> {
        let implicit = self.implicit_modules();
        let mut forms: Vec<(Vec<String>, bool)> = vec![(path.to_vec(), false)];
        for module in &implicit {
            let mut prefixed = module.clone();
            prefixed.extend(path.iter().cloned());
            forms.push((prefixed, true));
            if path.len() > module.len() && path.starts_with(module) {
                forms.push((path[module.len()..].to_vec(), true));
            }
        }
        let modules: Vec<Vec<String>> = if module.is_empty() {
            vec![Vec::new()]
        } else {
            module.split('|').map(split_path).collect()
        };
        for (form, through_implicit) in &forms {
            let Some((last, owner)) = form.split_last() else {
                continue;
            };
            let callee_found = callee == "*" || callee.split('|').any(|name| name == last);
            if !callee_found || !modules.iter().any(|module| module == owner) {
                continue;
            }
            return Some(if imported {
                SinkBasis::ImportedSymbol
            } else if *through_implicit || form.len() == 1 {
                SinkBasis::Builtin
            } else if self.is_root(&form[0]) {
                SinkBasis::ImportedSymbol
            } else {
                SinkBasis::NameOnly
            });
        }
        None
    }

    fn reference_sinks(&self, sinks: &mut Vec<Sink>) {
        let root = self.parsed.root_node();
        let rows: Vec<&SinkSpec> = self
            .specs
            .iter()
            .copied()
            .filter(|spec| matches!(spec.target, Target::Path { .. } | Target::Method { .. }))
            .collect();
        for (reference, facts) in self.file.references.iter().zip(&self.file.reference_facts) {
            // A bare name that an import binds can be an alias of any callee.
            let aliased = reference.receiver.is_none()
                && reference.qualifier.is_none()
                && self
                    .file
                    .imports
                    .iter()
                    .any(|binding| Self::binds(binding, &reference.name));
            let named: Vec<&SinkSpec> = rows
                .iter()
                .copied()
                .filter(|spec| match spec.target {
                    Target::Path { callee, .. } | Target::Method { callee } => {
                        aliased
                            || callee == "*"
                            || callee.split('|').any(|name| name == reference.name)
                    }
                    _ => false,
                })
                .collect();
            if named.is_empty() {
                continue;
            }
            let receiver_text = facts
                .receiver
                .and_then(|(start, end)| self.source.get(start..end))
                .and_then(|bytes| std::str::from_utf8(bytes).ok());
            let Some(written) = Self::written_path(reference, receiver_text) else {
                continue;
            };
            let chain = self.owner_chain(facts.owner);
            let owner = match &written {
                Some(path) => self.resolve(path, &chain),
                None => Owner::Unknown,
            };
            let written_length = written.as_ref().map_or(0, Vec::len);
            let call = syntax::call_node(root, facts.start_byte, facts.end_byte, &self.call_kinds);
            let arguments = call
                .map(|call| syntax::arguments(call, self.file.language))
                .unwrap_or_default();
            for spec in named {
                let basis = match (&spec.target, &owner) {
                    (_, Owner::Repository) => None,
                    (Target::Method { .. }, _) => (reference.receiver.is_some()
                        && !self.resolver_selected_repository(reference))
                    .then_some(SinkBasis::NameOnly),
                    (Target::Path { module, callee }, Owner::Paths { paths, imported }) => paths
                        .iter()
                        .find_map(|path| self.path_match(module, callee, path, *imported))
                        .filter(|_| {
                            *imported
                                || (written_length > 1 && !self.syntax.global_namespace)
                                || !self.resolver_selected_repository(reference)
                        }),
                    _ => None,
                };
                let Some(basis) = basis else {
                    continue;
                };
                let held = call.is_some()
                    && syntax::holds(&spec.predicate, &arguments, self.source, &self.syntax)
                    || (call.is_none() && spec.predicate == super::spec::Predicate::Always);
                if !held {
                    continue;
                }
                if spec.predicate == Predicate::NotSafeLoader
                    && !self.loader_is_unsafe(&arguments, &chain)
                {
                    continue;
                }
                let callee_start = facts.start_byte.min(reference.name_range.start_byte);
                let callee_text = self
                    .source
                    .get(callee_start..reference.name_range.end_byte)
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .unwrap_or(reference.name.as_str());
                sinks.push(self.sink(
                    spec.category,
                    reference.name_range,
                    reference.enclosing.clone(),
                    callee_text,
                    basis,
                    !arguments.is_empty() && arguments.iter().all(|node| syntax::is_literal(*node)),
                ));
            }
        }
    }

    /// A missing loader is not safe. A loader is not safe when the imports resolve it to
    /// a PyYAML loader that builds arbitrary objects. A loader that the imports cannot
    /// resolve, such as a variable, is not a sink.
    fn loader_is_unsafe(&self, arguments: &[Node<'_>], chain: &[usize]) -> bool {
        let keyword = arguments.iter().find_map(|argument| {
            let named = argument.kind() == "keyword_argument"
                && argument.child_by_field_name("name").is_some_and(|name| {
                    syntax::text(name, self.source).eq_ignore_ascii_case("loader")
                });
            named
                .then(|| argument.child_by_field_name("value"))
                .flatten()
        });
        let positional = arguments
            .get(1)
            .filter(|argument| argument.kind() != "keyword_argument");
        let Some(loader) = keyword.or(positional.copied()) else {
            return true;
        };
        let compact: String = syntax::text(loader, self.source)
            .split_whitespace()
            .collect();
        let written = split_path(&compact);
        if written.is_empty()
            || !written
                .iter()
                .all(|part| part.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            return false;
        }
        match self.resolve(&written, chain) {
            Owner::Paths { paths, .. } => paths.iter().any(|path| {
                path.len() >= 2
                    && path[0] == "yaml"
                    && path
                        .last()
                        .is_some_and(|last| UNSAFE_LOADERS.contains(&last.as_str()))
            }),
            Owner::Repository | Owner::Unknown => false,
        }
    }

    fn sink(
        &self,
        category: SinkCategory,
        range: Span,
        enclosing: Option<String>,
        callee_text: &str,
        basis: SinkBasis,
        constant_argument: bool,
    ) -> Sink {
        let callee_text: String = callee_text.chars().take(200).collect();
        Sink {
            key: format!(
                "{}:{}:{}:{}",
                self.file.file,
                range.start_line,
                range.start_column,
                category.name()
            ),
            file: self.file.file.clone(),
            category,
            cwe: category.cwe(),
            range,
            enclosing,
            callee_text,
            basis,
            flags: SinkFlags {
                constant_argument,
                in_test: self.in_test(range.start_byte),
            },
        }
    }

    fn enclosing_key(&self, byte: usize) -> Option<String> {
        self.enclosing_position(byte)
            .map(|position| self.file.symbols[position].key.clone())
    }

    /// Walks the tree for the sinks that are not calls.
    fn tree_sinks(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let kind = node.kind();
        if self.syntax.string_kinds.contains(&kind) {
            let literal = syntax::text(node, self.source);
            if let Some(prefix) = syntax::token_prefix(literal) {
                sinks.push(self.sink(
                    SinkCategory::HardcodedCredential,
                    Span::of(&node),
                    self.enclosing_key(node.start_byte()),
                    prefix,
                    SinkBasis::Builtin,
                    false,
                ));
            }
            return;
        }
        if self.syntax.unsafe_kinds.contains(&kind)
            && self.has_target(|target| matches!(target, Target::UnsafeBlock))
        {
            sinks.push(self.sink(
                SinkCategory::MemoryUnsafe,
                Span::of(&node),
                self.enclosing_key(node.start_byte()),
                "unsafe",
                SinkBasis::Builtin,
                false,
            ));
        }
        if self.syntax.mention_kinds.contains(&kind) && self.mention(node, sinks) {
            return;
        }
        if self.syntax.keyed_kinds.contains(&kind) {
            self.keyed_field(node, sinks);
        }
        if self.syntax.assignment_kinds.contains(&kind) {
            self.assignment(node, sinks);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.tree_sinks(child, sinks);
        }
    }

    fn has_target(&self, test: impl Fn(&Target) -> bool) -> bool {
        self.specs.iter().any(|spec| test(&spec.target))
    }

    fn keyed_field(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let Some((key, value)) = syntax::key_value(node, self.source) else {
            return;
        };
        for spec in &self.specs {
            if let Target::KeyedField {
                key: wanted_key,
                value: wanted_value,
                module,
                owner,
            } = spec.target
            {
                if key == wanted_key.to_lowercase()
                    && value == wanted_value
                    && self.literal_has_type(node, module, owner)
                {
                    sinks.push(self.sink(
                        spec.category,
                        Span::of(&node),
                        self.enclosing_key(node.start_byte()),
                        syntax::text(node, self.source),
                        SinkBasis::NameOnly,
                        false,
                    ));
                }
            }
        }
    }

    /// The nearest composite literal around the node has the type `owner` of the module,
    /// and the imports resolve the type. A map literal has no such type.
    fn literal_has_type(&self, node: Node<'_>, module: &str, owner: &str) -> bool {
        let mut parent = node.parent();
        while let Some(candidate) = parent {
            if candidate.kind() == "composite_literal" {
                let Some(declared) = candidate.child_by_field_name("type") else {
                    return false;
                };
                let compact: String = syntax::text(declared, self.source)
                    .split_whitespace()
                    .collect();
                let written = split_path(&compact);
                if written.is_empty()
                    || !written
                        .iter()
                        .all(|part| part.chars().all(|c| c.is_alphanumeric() || c == '_'))
                {
                    return false;
                }
                let chain = self.owner_chain(self.enclosing_position(node.start_byte()));
                return match self.resolve(&written, &chain) {
                    Owner::Paths { paths, imported } => paths
                        .iter()
                        .any(|path| self.path_match(module, owner, path, imported).is_some()),
                    Owner::Repository | Owner::Unknown => false,
                };
            }
            parent = candidate.parent();
        }
        false
    }

    /// Reports an assignment to a member that a row names.
    fn assignment(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        let compact: String = syntax::text(left, self.source).split_whitespace().collect();
        let written = split_path(&compact);
        if written.len() < 2
            || !written
                .iter()
                .all(|part| part.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            return;
        }
        let value: String = syntax::text(right, self.source)
            .split_whitespace()
            .collect::<String>()
            .to_lowercase();
        let chain = self.owner_chain(self.enclosing_position(node.start_byte()));
        let Owner::Paths { paths, imported } = self.resolve(&written, &chain) else {
            return;
        };
        for spec in &self.specs {
            let Target::Assignment {
                module,
                name,
                values,
            } = spec.target
            else {
                continue;
            };
            if written.last().map(String::as_str) != Some(name)
                || !values.iter().any(|wanted| value.contains(wanted))
            {
                continue;
            }
            let basis = paths
                .iter()
                .find_map(|path| self.path_match(module, name, path, imported));
            if let Some(basis) = basis {
                sinks.push(self.sink(
                    spec.category,
                    Span::of(&node),
                    self.enclosing_key(node.start_byte()),
                    &compact,
                    basis,
                    false,
                ));
            }
        }
    }

    /// Reports a member access that a row names. `true` means that the node matched.
    fn mention(&self, node: Node<'_>, sinks: &mut Vec<Sink>) -> bool {
        let compact: String = syntax::text(node, self.source).split_whitespace().collect();
        let written = split_path(&compact);
        if written.len() < 2
            || !written
                .iter()
                .all(|part| part.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            return false;
        }
        let chain = self.owner_chain(self.enclosing_position(node.start_byte()));
        let owner = self.resolve(&written, &chain);
        let Owner::Paths { paths, imported } = owner else {
            return false;
        };
        for spec in &self.specs {
            let Target::Mention { module, name } = spec.target else {
                continue;
            };
            let last = written.last().map(String::as_str).unwrap_or("");
            if last != name || (self.syntax.global_namespace && self.repository.all.contains(last))
            {
                continue;
            }
            let basis = paths
                .iter()
                .find_map(|path| self.path_match(module, name, path, imported));
            if let Some(basis) = basis {
                sinks.push(self.sink(
                    spec.category,
                    Span::of(&node),
                    self.enclosing_key(node.start_byte()),
                    &compact,
                    basis,
                    false,
                ));
                return true;
            }
        }
        false
    }
}
