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
    find_sinks_with(parsed, files, &[])
}

/// Like [`find_sinks`], with more rows for the sinks of the user, for example the
/// wrappers of a company. The rows apply to the languages that they name.
pub fn find_sinks_with(
    parsed: &[ParseResult],
    files: &[FileSymbols],
    extra: &'static [SinkSpec],
) -> Vec<Sink> {
    assert_eq!(parsed.len(), files.len(), "each file needs its syntax tree");
    let repository = Repository::new(files);
    let per_file: Vec<Vec<Sink>> = (0..files.len())
        .into_par_iter()
        .map(|position| file_sinks(&parsed[position], &files[position], &repository, extra))
        .collect();
    per_file.into_iter().flatten().collect()
}

/// The names that the repository defines. A field and a variable are not names of a module or a type.
pub(super) struct Repository<'a> {
    by_file: HashMap<&'a str, HashSet<&'a str>>,
    by_directory: HashMap<&'a str, HashSet<&'a str>>,
    all: HashSet<&'a str>,
}

fn directory(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

impl<'a> Repository<'a> {
    pub(super) fn new(files: &'a [FileSymbols]) -> Self {
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

/// The text without the parentheses that enclose all of it.
fn without_outer_parentheses(text: &str) -> &str {
    let mut text = text.trim();
    while let Some(inner) = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let mut depth = 0i32;
        let balanced = inner.chars().all(|character| {
            match character {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth >= 0
        }) && depth == 0;
        if !balanced {
            break;
        }
        text = inner.trim();
    }
    text
}

/// The type that `new Type(arguments)` makes, when the text is only that expression.
fn constructed_type(text: &str) -> Option<Vec<String>> {
    let rest = without_outer_parentheses(text)
        .strip_prefix("new ")?
        .trim_start();
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

/// The one expression of a list, as the sides of the Go assignment `a.b = c` are. A list
/// with more expressions gives `None`. A node that is not a list is the expression.
fn single_expression(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() != "expression_list" {
        return Some(node);
    }
    (node.named_child_count() == 1)
        .then(|| node.named_child(0))
        .flatten()
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
    generated: bool,
    call_kinds: Vec<&'static str>,
}

pub(super) fn file_sinks(
    parsed: &ParseResult,
    file: &FileSymbols,
    repository: &Repository<'_>,
    extra: &'static [SinkSpec],
) -> Vec<Sink> {
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
        specs: specs_for(language)
            .chain(
                extra
                    .iter()
                    .filter(|spec| spec.languages.contains(&language)),
            )
            .collect(),
        source,
        test_ranges,
        generated: language == Language::Go
            && syntax::has_generated_header(parsed.root_node(), source),
        call_kinds: language_spec(language).call_syntax().call_kinds(),
    };
    let mut sinks = Vec::new();
    let covered = context.reference_sinks(&mut sinks);
    context.tree_sinks(parsed.root_node(), &mut sinks, &covered);
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

    /// The local name that the byte `at` sees. A name is not in scope before
    /// its first value, and the name in the nearest block that contains `at`
    /// hides the others. This is the rule of the call resolver.
    fn find_local(&self, name: &str, chain: &[usize], at: usize) -> Option<&Local> {
        let visible = |local: &&Local| {
            local.name.name == name
                && local.name.value_start.is_none_or(|value| value < at)
                && local.block.as_ref().is_none_or(|block| block.contains(&at))
        };
        let nearest = |owner: Option<usize>| {
            self.file
                .locals
                .iter()
                .filter(visible)
                .filter(|local| local.owner == owner)
                .min_by_key(|local| local.block.as_ref().map_or(usize::MAX, |block| block.len()))
        };
        chain
            .iter()
            .find_map(|owner| nearest(Some(*owner)))
            .or_else(|| nearest(None))
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

    fn resolve(&self, written: &[String], chain: &[usize], at: usize) -> Owner {
        let mut written = written.to_vec();
        for _ in 0..2 {
            let Some(local) = self.find_local(&written[0], chain, at) else {
                break;
            };
            if written.len() < 2 {
                return Owner::Unknown;
            }
            match local
                .name
                .type_name
                .as_deref()
                .map(crate::index::calls::required_type)
                .and_then(type_segments)
            {
                Some(declared) => {
                    written.splice(0..1, declared);
                }
                None => return Owner::Unknown,
            }
        }
        let head = written[0].clone();
        let rest = &written[1..];
        let imports = &self.file.imports;
        let in_effect = imports
            .iter()
            .rfind(|binding| Self::binds(binding, &head) && binding.range.start_byte <= at);
        let binding =
            in_effect.or_else(|| imports.iter().find(|binding| Self::binds(binding, &head)));
        if let Some(binding) = binding {
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

    /// The target of a sink: the path without the module that the language gives without
    /// an import. `builtins.eval` and `eval` are one target.
    fn target_of(&self, path: &[String]) -> String {
        for module in self.implicit_modules() {
            if path.len() > module.len() && path.starts_with(&module) {
                return path[module.len()..].join(".");
            }
        }
        path.join(".")
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

    /// Pushes the sinks of the calls. The result has the start bytes of the object literals
    /// that are arguments of a call that a row names. The matcher does not read their
    /// properties again.
    fn reference_sinks(&self, sinks: &mut Vec<Sink>) -> HashSet<usize> {
        let mut covered = HashSet::new();
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
            let chain = self.owner_chain(facts.owner);
            // `const run = promisify(exec)` makes `run(command)` a call of `exec`.
            let promisified = if aliased {
                None
            } else {
                self.promisified(reference, &chain, facts.start_byte)
            };
            let callee_name = promisified
                .as_ref()
                .and_then(|(path, _)| path.last())
                .map_or(reference.name.as_str(), String::as_str);
            let named: Vec<&SinkSpec> = rows
                .iter()
                .copied()
                .filter(|spec| match spec.target {
                    Target::Path { callee, .. } | Target::Method { callee } => {
                        aliased
                            || callee == "*"
                            || callee.split('|').any(|name| name == callee_name)
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
            let (written, owner) = match &promisified {
                Some((path, at)) => (Some(path.clone()), self.resolve(path, &chain, *at)),
                None => {
                    let Some(written) = Self::written_path(reference, receiver_text) else {
                        continue;
                    };
                    let owner = match &written {
                        Some(path) => self.resolve(path, &chain, facts.start_byte),
                        None => Owner::Unknown,
                    };
                    (written, owner)
                }
            };
            let written_length = written.as_ref().map_or(0, Vec::len);
            let call = syntax::call_node(root, facts.start_byte, facts.end_byte, &self.call_kinds);
            let call_arguments = call
                .map(|call| syntax::arguments(call, self.file.language))
                .unwrap_or_default();
            let receiver: Vec<Node<'_>> = facts
                .receiver
                .and_then(|(start, end)| root.descendant_for_byte_range(start, end))
                .into_iter()
                .collect();
            let cover = |covered: &mut HashSet<usize>| {
                covered.extend(
                    call_arguments
                        .iter()
                        .filter(|argument| argument.kind() == "object")
                        .map(|argument| argument.start_byte()),
                );
            };
            if matches!(owner, Owner::Repository) || self.resolver_selected_repository(reference) {
                cover(&mut covered);
            }
            for spec in named {
                let arguments = if spec.predicate == Predicate::Receiver {
                    &receiver
                } else {
                    &call_arguments
                };
                let matched: Option<(SinkBasis, String)> = match (&spec.target, &owner) {
                    (_, Owner::Repository) => None,
                    (Target::Method { .. }, _) => (reference.receiver.is_some()
                        && !self.resolver_selected_repository(reference))
                    .then(|| (SinkBasis::NameOnly, format!("*.{}", reference.name))),
                    (Target::Path { module, callee }, Owner::Paths { paths, imported }) => paths
                        .iter()
                        .find_map(|path| {
                            self.path_match(module, callee, path, *imported)
                                .map(|basis| (basis, self.target_of(path)))
                        })
                        .filter(|_| {
                            *imported
                                || (written_length > 1 && !self.syntax.global_namespace)
                                || !self.resolver_selected_repository(reference)
                        }),
                    _ => None,
                };
                let Some((basis, target)) = matched else {
                    continue;
                };
                let held = call.is_some()
                    && syntax::holds(&spec.predicate, arguments, self.source, &self.syntax)
                    || (call.is_none()
                        && matches!(spec.predicate, Predicate::Always | Predicate::Receiver));
                if !held {
                    continue;
                }
                if spec.predicate == Predicate::NotSafeLoader
                    && !self.loader_is_unsafe(arguments, &chain, facts.start_byte)
                {
                    continue;
                }
                // A receiver that is an expression can hold literals, and a literal can be a
                // secret. Thus the text of the callee has only the names, never the arguments.
                let callee_text = if matches!(reference.receiver, Some(Receiver::Expression)) {
                    written
                        .as_ref()
                        .map_or_else(|| reference.name.clone(), |path| path.join("."))
                } else {
                    let callee_start = facts.start_byte.min(reference.name_range.start_byte);
                    self.source
                        .get(callee_start..reference.name_range.end_byte)
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .unwrap_or(reference.name.as_str())
                        .to_string()
                };
                let mut sink = self.sink(
                    spec.category,
                    reference.name_range,
                    reference.enclosing.as_ref().map(|key| key.to_string()),
                    &callee_text,
                    basis,
                    self.is_constant(&spec.predicate, arguments),
                );
                sink.target = target;
                sinks.push(sink);
                cover(&mut covered);
            }
        }
        covered
    }

    /// The function that a bare name holds when a `const` declaration binds the name to
    /// `promisify(function)` of `util`. The result is the written path of the function and the
    /// byte at which the imports resolve it.
    fn promisified(
        &self,
        reference: &Reference,
        chain: &[usize],
        at: usize,
    ) -> Option<(Vec<String>, usize)> {
        if reference.receiver.is_some() || reference.qualifier.is_some() {
            return None;
        }
        let local = self.find_local(&reference.name, chain, at)?;
        let start = local.name.value_start?;
        let mut node = self
            .parsed
            .root_node()
            .descendant_for_byte_range(start, start)?;
        while node.kind() != "call_expression" {
            node = node
                .parent()
                .filter(|parent| parent.start_byte() == start)?;
        }
        let declarator = node
            .parent()
            .filter(|parent| parent.kind() == "variable_declarator")?;
        let declaration = declarator
            .parent()
            .filter(|parent| parent.kind() == "lexical_declaration")?;
        if declaration.child(0).map(|keyword| keyword.kind()) != Some("const") {
            return None;
        }
        let function_text: String = node
            .child_by_field_name("function")
            .map(|function| syntax::text(function, self.source))?
            .split_whitespace()
            .collect();
        let function = chain_segments(&function_text)?;
        let is_promisify = match self.resolve(&function, chain, start) {
            Owner::Paths { paths, imported } => paths.iter().any(|path| {
                self.path_match("util|node:util", "promisify", path, imported)
                    .is_some()
            }),
            Owner::Repository | Owner::Unknown => false,
        };
        let arguments = syntax::arguments(node, self.file.language);
        let [argument] = arguments.as_slice() else {
            return None;
        };
        let argument_text: String = syntax::text(*argument, self.source)
            .split_whitespace()
            .collect();
        let path = chain_segments(&argument_text)?;
        (is_promisify && path.iter().all(|segment| !segment.ends_with(')')))
            .then_some((path, start))
    }

    /// The arguments that make the call a sink are all literals. A call without such
    /// arguments is not constant.
    fn is_constant(&self, predicate: &Predicate, arguments: &[Node<'_>]) -> bool {
        let named = syntax::subject(predicate, arguments, self.source, &self.syntax)
            .unwrap_or_else(|| arguments.to_vec());
        !named.is_empty() && named.into_iter().all(syntax::is_literal)
    }

    /// A missing loader is not safe. A loader is not safe when the imports resolve it to
    /// a PyYAML loader that builds arbitrary objects. A loader that the imports cannot
    /// resolve, such as a variable, is not a sink.
    fn loader_is_unsafe(&self, arguments: &[Node<'_>], chain: &[usize], at: usize) -> bool {
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
        match self.resolve(&written, chain, at) {
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
            target: callee_text.clone(),
            callee_text,
            basis,
            flags: SinkFlags {
                constant_argument,
                in_test: self.in_test(range.start_byte),
                in_generated: self.generated,
            },
        }
    }

    fn enclosing_key(&self, byte: usize) -> Option<String> {
        self.enclosing_position(byte)
            .map(|position| self.file.symbols[position].key.to_string())
    }

    /// Walks the tree for the sinks that are not calls.
    fn tree_sinks(&self, node: Node<'_>, sinks: &mut Vec<Sink>, covered: &HashSet<usize>) {
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
            if self.has_target(|target| matches!(target, Target::MemberAssignment { .. })) {
                self.member_assignment(node, sinks);
            }
            if self.has_target(|target| matches!(target, Target::Assignment { .. })) {
                self.assignment(node, sinks);
            }
        }
        if self.syntax.attribute_kinds.contains(&kind) {
            self.attribute(node, sinks);
        }
        if self.syntax.property_kinds.contains(&kind) {
            self.object_property(node, sinks, covered);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.tree_sinks(child, sinks, covered);
        }
    }

    fn has_target(&self, test: impl Fn(&Target) -> bool) -> bool {
        self.specs.iter().any(|spec| test(&spec.target))
    }

    fn keyed_field(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let Some((key, _)) = syntax::key_value(node, self.source) else {
            return;
        };
        let count = node.named_child_count();
        let Some(value) = count
            .checked_sub(1)
            .and_then(|last| node.named_child(last as u32))
        else {
            return;
        };
        let value = syntax::literal_value(value);
        for spec in &self.specs {
            if let Target::KeyedField {
                key: wanted_key,
                module,
                owner,
            } = spec.target
            {
                if key == wanted_key.to_lowercase()
                    && syntax::holds(&spec.predicate, &[value], self.source, &self.syntax)
                    && self.literal_has_type(node, module, owner)
                {
                    // A value can hold a literal, and a literal can be a secret. Thus the text
                    // of the sink has the value only when the value is a chain of names.
                    let value_text: String = syntax::text(value, self.source)
                        .split_whitespace()
                        .collect();
                    let shown = if chain_segments(&value_text).is_some() {
                        value_text
                    } else {
                        "<expression>".to_string()
                    };
                    sinks.push(self.sink(
                        spec.category,
                        Span::of(&node),
                        self.enclosing_key(node.start_byte()),
                        &format!("{wanted_key}: {shown}"),
                        SinkBasis::NameOnly,
                        self.is_constant(&spec.predicate, &[value]),
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
                return match self.resolve(&written, &chain, node.start_byte()) {
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

    /// The assigned value is one that a row names. The value `=>true` means a lambda whose
    /// whole body is `true`, or a block whose only statement is `return true;`. Any other
    /// value is a name, and the last segment of the assigned path must be that name.
    fn value_matches(&self, right: Node<'_>, values: &[&str]) -> bool {
        let right = syntax::unwrap_parentheses(right);
        values.is_empty()
            || values.iter().any(|wanted| {
                if *wanted == "=>true" {
                    return syntax::is_constant_true_lambda(right, self.source);
                }
                let compact: String = syntax::text(right, self.source)
                    .split_whitespace()
                    .collect::<String>()
                    .to_lowercase();
                split_path(&compact).last().map(String::as_str) == Some(*wanted)
            })
    }

    /// Reports an assignment to a member of any receiver when a row names the member.
    fn member_assignment(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        // `a.b = c` and `a["b"] = c`.
        let (object, member) = match left.kind() {
            "member_expression" => (
                left.child_by_field_name("object"),
                left.child_by_field_name("property")
                    .map(|property| syntax::text(property, self.source)),
            ),
            "subscript_expression" => (
                left.child_by_field_name("object"),
                left.child_by_field_name("index")
                    .filter(|index| index.kind() == "string" && syntax::is_literal(*index))
                    .map(|index| syntax::unquote(syntax::text(index, self.source))),
            ),
            _ => return,
        };
        let (Some(object), Some(member)) = (object, member) else {
            return;
        };
        let object_text: String = syntax::text(object, self.source)
            .split_whitespace()
            .collect();
        for spec in &self.specs {
            let Target::MemberAssignment { names, receiver } = spec.target else {
                continue;
            };
            if !names.split('|').any(|name| name == member)
                || (!receiver.is_empty() && object_text != receiver)
                || !syntax::holds(&spec.predicate, &[right], self.source, &self.syntax)
            {
                continue;
            }
            // A receiver can hold literals, and a literal can be a secret. Thus the text of
            // the sink is the chain of names, or only the member.
            let compact: String = syntax::text(left, self.source).split_whitespace().collect();
            let callee_text = if chain_segments(&compact).is_some() {
                compact
            } else {
                format!("*.{member}")
            };
            sinks.push(self.sink(
                spec.category,
                Span::of(&node),
                self.enclosing_key(node.start_byte()),
                &callee_text,
                SinkBasis::NameOnly,
                self.is_constant(&spec.predicate, &[right]),
            ));
        }
    }

    /// Reports a property of an object literal that a row names, unless the object is an
    /// argument of a call that the call rows read.
    fn object_property(&self, node: Node<'_>, sinks: &mut Vec<Sink>, covered: &HashSet<usize>) {
        let Some(object) = node.parent().filter(|parent| parent.kind() == "object") else {
            return;
        };
        if covered.contains(&object.start_byte()) {
            return;
        }
        let (Some(key), Some(value)) = (
            node.child_by_field_name("key"),
            node.child_by_field_name("value"),
        ) else {
            return;
        };
        let written = match key.kind() {
            "property_identifier" => syntax::text(key, self.source),
            "string" if syntax::is_literal(key) => syntax::unquote(syntax::text(key, self.source)),
            _ => return,
        };
        for spec in &self.specs {
            let Target::ObjectProperty { keys } = spec.target else {
                continue;
            };
            if !keys.split('|').any(|wanted| wanted == written)
                || !syntax::holds(&spec.predicate, &[value], self.source, &self.syntax)
            {
                continue;
            }
            // A value can hold a literal, and a literal can be a secret.
            let value_text: String = syntax::text(value, self.source)
                .split_whitespace()
                .collect();
            let is_name = matches!(
                value.kind(),
                "identifier" | "member_expression" | "true" | "false"
            );
            let shown = if is_name && chain_segments(&value_text).is_some() {
                value_text
            } else {
                "<expression>".to_string()
            };
            sinks.push(self.sink(
                spec.category,
                Span::of(&node),
                self.enclosing_key(node.start_byte()),
                &format!("{written}: {shown}"),
                SinkBasis::NameOnly,
                self.is_constant(&spec.predicate, &[value]),
            ));
        }
    }

    /// Reports a JSX attribute that a row names. The value is the property `key` of an
    /// object literal in the attribute, or the expression of the attribute.
    fn attribute(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        let Some((name, rest)) = children.split_first() else {
            return;
        };
        let written = syntax::text(*name, self.source);
        for spec in &self.specs {
            let Target::Attribute { name, key } = spec.target else {
                continue;
            };
            if written != name {
                continue;
            }
            let expression = rest
                .first()
                .map(|value| {
                    if value.kind() == "jsx_expression" {
                        value.named_child(0).unwrap_or(*value)
                    } else {
                        *value
                    }
                })
                .map(syntax::unwrap_parentheses);
            let value = expression.map(|expression| {
                if expression.kind() != "object" {
                    return expression;
                }
                let mut members = expression.walk();
                let found = expression.named_children(&mut members).find(|member| {
                    syntax::key_value(*member, self.source)
                        .is_some_and(|(found, _)| found == key.to_lowercase())
                });
                found
                    .and_then(|pair| pair.child_by_field_name("value"))
                    .unwrap_or(expression)
            });
            let values: Vec<Node<'_>> = value.into_iter().collect();
            if !syntax::holds(&spec.predicate, &values, self.source, &self.syntax) {
                continue;
            }
            sinks.push(self.sink(
                spec.category,
                Span::of(&node),
                self.enclosing_key(node.start_byte()),
                written,
                SinkBasis::NameOnly,
                self.is_constant(&spec.predicate, &values),
            ));
        }
    }

    /// Reports an assignment to a member that a row names.
    fn assignment(&self, node: Node<'_>, sinks: &mut Vec<Sink>) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left").and_then(single_expression),
            node.child_by_field_name("right")
                .and_then(single_expression),
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
        let chain = self.owner_chain(self.enclosing_position(node.start_byte()));
        let Owner::Paths { paths, imported } = self.resolve(&written, &chain, node.start_byte())
        else {
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
                || !self.value_matches(right, values)
                || !syntax::holds(&spec.predicate, &[right], self.source, &self.syntax)
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
                    self.is_constant(&spec.predicate, &[right]),
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
        let owner = self.resolve(&written, &chain, node.start_byte());
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
