//! Symbol extraction. One walk of the syntax tree gives the symbols of a file.
//!
//! A [`LanguageSpec`] supplies a table of definition node kinds. The hook
//! methods of the specification are only for syntax that the table cannot
//! describe.

use std::collections::{BTreeMap, HashMap};

use tree_sitter::Node;

use super::calls::{split_type, Argument, Call, CallSyntax, LocalName};
use super::lang;
use super::model::{
    symbol_key, Exported, ImportBinding, ImportKind, ImportTarget, Reference, ReferenceKind,
    Resolution, ResolutionStatus, Span, Symbol, SymbolKind,
};
use crate::core::Language;
use crate::parser::ParseResult;

/// Tells where the name of a definition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRule {
    /// The name is the child with this field name.
    Field(&'static str),
    /// [`LanguageSpec::name_node`] finds the name.
    Hook,
}

/// One row of the definition table of a language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionRule {
    pub node_kind: &'static str,
    pub kind: SymbolKind,
    /// The kind when the nearest container is a type.
    pub kind_in_type: SymbolKind,
    pub name: NameRule,
}

impl DefinitionRule {
    pub const fn new(node_kind: &'static str, kind: SymbolKind, name: NameRule) -> Self {
        Self {
            node_kind,
            kind,
            kind_in_type: kind,
            name,
        }
    }

    /// A function that is a method when a type contains it.
    pub const fn function(node_kind: &'static str, name: NameRule) -> Self {
        Self {
            node_kind,
            kind: SymbolKind::Function,
            kind_in_type: SymbolKind::Method,
            name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRole {
    Type,
    Function,
    /// A module is a container, but a function in it is not a method.
    Module,
    /// A namespace is a part of the key of a symbol, but it is not a container.
    Namespace,
}

/// A syntax node that contains definitions.
#[derive(Debug, Clone)]
pub struct Scope<'tree> {
    pub node: Node<'tree>,
    pub name: String,
    pub role: ScopeRole,
    /// The kind of the symbol that opened this scope, if a symbol opened it.
    pub kind: Option<SymbolKind>,
    pub exported: Exported,
    /// The names of the scopes that contain this scope, and then the name of this scope.
    pub path: Vec<String>,
}

/// A symbol before the extraction gives it a key.
#[derive(Debug, Clone)]
pub struct Draft<'tree> {
    pub node: Node<'tree>,
    pub name_node: Node<'tree>,
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub container_path: Vec<String>,
    pub receiver_type: Option<String>,
    pub supertypes: Vec<String>,
    pub span: Span,
    pub body: Option<Node<'tree>>,
    /// The role of the scope that this definition opens for its children.
    pub role: ScopeRole,
    /// The number of declared parameters, if the language links definitions
    /// to declarations.
    pub parameter_count: Option<usize>,
    /// The declared type of each parameter, without white space. A parameter
    /// without a declared type has an empty text.
    pub parameter_types: Option<Vec<String>>,
    /// The node ID of the nearest definition that contains this definition.
    pub owner: Option<usize>,
    /// The declared type of the result, as the source has it.
    pub return_type: Option<String>,
    /// The definition implements a member of a trait or an interface in a
    /// block that is not the body of its type, as a Rust `impl Trait for Type` does.
    pub trait_implementation: bool,
    /// The definition is the default export of its module.
    pub default_export: bool,
    /// The definition is a function value that a variable holds, in a
    /// language where a variable and a function do not share names.
    pub function_value: bool,
    /// A C++ function in `extern "C"`. It has the linkage of a C function.
    pub c_linkage: bool,
    /// A named type that owns methods although its kind is `TypeAlias`, as a
    /// Go defined type `type Count int` does.
    pub defined_type: bool,
    /// The modules that a Ruby `prepend` adds. They come before the type in
    /// the search for a method.
    pub prepends: Vec<String>,
}

/// Facts that only `link_files` and the resolver use. They are not a part of
/// the symbol model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkFacts {
    /// The names of the scopes that contain the symbol. A qualifier in the
    /// name of a definition is a part of this path.
    pub container_path: Vec<String>,
    pub parameter_count: Option<usize>,
    pub parameter_types: Option<Vec<String>>,
    /// The position in `FileSymbols::symbols` of the nearest definition that
    /// contains the symbol.
    pub parent: Option<usize>,
    pub return_type: Option<String>,
    pub trait_implementation: bool,
    pub default_export: bool,
    pub function_value: bool,
    pub c_linkage: bool,
    pub defined_type: bool,
    pub prepends: Vec<String>,
    /// The names, different from its own, under which the module exports
    /// this top-level symbol, as `export { a as b }` gives `b` to `a`.
    pub export_aliases: Vec<String>,
}

/// Facts of a reference that only the resolver uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceFacts {
    /// The start byte of the call expression. A local name whose first value
    /// starts at this byte has the result of the call.
    pub start_byte: usize,
    pub end_byte: usize,
    /// The position in `FileSymbols::symbols` of the enclosing definition.
    pub owner: Option<usize>,
    pub arguments: Vec<Argument>,
    /// The byte range of the receiver expression. When the receiver is a
    /// call, the call with this range gives the type of the receiver.
    pub receiver: Option<(usize, usize)>,
    /// The names of the receiver expression when it is one name or a chain
    /// of member accesses, as `c.parent` is.
    pub receiver_path: Option<Vec<String>>,
    /// `?` follows the call, as Rust `call()?` has. `?.` is not this mark.
    pub tried: bool,
    /// The receiver is one element of the value that `receiver_path` names,
    /// as `c.items[i]` is.
    pub receiver_indexed: bool,
    /// `->` follows the receiver. In C and C++ this is a member of the
    /// object that a pointer names.
    pub arrow: bool,
}

/// The names of the value that an index expression reads: `c.items[i]`
/// gives `c`, `items`. `None` means that the text is a different expression.
fn indexed_path(text: &str) -> Option<Vec<String>> {
    let text = text.trim().strip_suffix(']')?;
    let mut depth = 1usize;
    let open = text
        .char_indices()
        .rev()
        .find_map(|(position, character)| {
            match character {
                ']' => depth += 1,
                '[' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(position)
        })?;
    name_path(&text[..open])
}

/// Makes a field that an assignment or a parameter in a method declares a
/// member of the nearest type. `false` means that no type contains the node.
pub fn field_of_type(draft: &mut Draft<'_>, context: &Context<'_, '_>) -> bool {
    let Some(owner) = context
        .scopes
        .iter()
        .rev()
        .find(|scope| scope.role == ScopeRole::Type)
    else {
        return false;
    };
    draft.kind = SymbolKind::Field;
    draft.container = Some(owner.name.clone());
    draft.container_path = owner.path.clone();
    true
}

/// The names of a chain of member accesses: `a.b.c` gives `a`, `b`, `c`.
/// `None` means that the text is a different expression.
pub fn name_path(text: &str) -> Option<Vec<String>> {
    let text = text.trim().replace("?.", ".").replace("->", ".");
    let is_name = |name: &str| {
        let mut characters = name.chars();
        characters
            .next()
            .is_some_and(|first| first.is_alphabetic() || "_$@".contains(first))
            && characters.all(|character| character.is_alphanumeric() || character == '_')
    };
    let names: Vec<&str> = text.split('.').collect();
    names
        .iter()
        .all(|name| is_name(name))
        .then(|| names.into_iter().map(str::to_string).collect())
}

/// The names of the chain of member accesses that starts at the byte `start`
/// and is the full value of a statement. A call, an operator, or an index
/// after the chain gives `None`.
fn value_path(source: &[u8], start: usize) -> Option<Vec<String>> {
    let rest = source.get(start..)?;
    let length = rest
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || b"_$@.".contains(byte)))
        .unwrap_or(rest.len());
    let after = rest[length..]
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t'));
    if !after.is_none_or(|byte| b"\n\r;,){}:".contains(byte)) {
        return None;
    }
    name_path(std::str::from_utf8(&rest[..length]).ok()?)
}

/// The declared types of the parameters in a parameter list, without white
/// space and without the `:` of a type annotation.
pub fn parameter_types(parameters: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .map(|parameter| {
            let text = parameter
                .child_by_field_name("type")
                .and_then(|type_node| type_node.utf8_text(source).ok())
                .unwrap_or("");
            let text: String = text.split_whitespace().collect();
            text.trim_start_matches(':').to_string()
        })
        .collect()
}

/// A local name and the definition that binds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub name: LocalName,
    /// The position in `FileSymbols::symbols` of the function that binds the name.
    pub owner: Option<usize>,
    /// The names of the first value when it is a chain of member accesses.
    pub value_path: Option<Vec<String>>,
    /// The bytes of the block that contains the declaration, when the name
    /// is visible in that block only.
    pub block: Option<std::ops::Range<usize>>,
}

/// One symbol and its link facts.
pub struct Linked<'a> {
    pub symbol: &'a mut Symbol,
    pub facts: &'a LinkFacts,
}

pub struct Context<'a, 'tree> {
    /// The path of the file, as the keys have it.
    pub file: &'a str,
    pub source: &'a [u8],
    pub scopes: &'a [Scope<'tree>],
    /// The ID of the node at which the walk is, and the parent of that node.
    pub current: Option<(usize, Option<Node<'tree>>)>,
}

impl<'tree> Context<'_, 'tree> {
    /// The parent of `node`. The walk knows the parent of its current node.
    /// For a different node, the tree searches from the root, which is slow.
    pub fn parent(&self, node: Node<'tree>) -> Option<Node<'tree>> {
        match self.current {
            Some((current, parent)) if current == node.id() => parent,
            _ => node.parent(),
        }
    }

    pub fn text(&self, node: Node<'_>) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// The nearest scope that is a type or a function.
    pub fn container(&self) -> Option<&Scope<'tree>> {
        self.scopes
            .iter()
            .rev()
            .find(|scope| scope.role != ScopeRole::Namespace)
    }

    /// The names of the scopes that contain the current node.
    pub fn path(&self) -> Vec<String> {
        self.scopes
            .last()
            .map(|scope| scope.path.clone())
            .unwrap_or_default()
    }

    /// Makes an import binding that has no target. `statement` gives the range.
    pub fn import(
        &self,
        statement: Node<'_>,
        module_path: impl Into<String>,
        kind: ImportKind,
        imported_name: Option<String>,
        local_name: Option<String>,
    ) -> ImportBinding {
        ImportBinding {
            file: self.file.to_string(),
            range: Span::of(&statement),
            module_path: module_path.into(),
            kind,
            imported_name,
            local_name,
            target_file: ImportTarget::Unresolved,
            target_symbol: None,
        }
    }

    pub fn in_function(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.role == ScopeRole::Function)
    }
}

pub trait LanguageSpec: Sync {
    fn definitions(&self) -> Vec<DefinitionRule>;

    /// Each node kind that a hook method of this specification compares with.
    /// The grammar lint examines these kinds.
    fn hook_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn comment_kinds(&self) -> &'static [&'static str];

    /// Finds the name of a definition whose rule has [`NameRule::Hook`].
    /// If the result is `None`, the node is not a definition.
    fn name_node<'tree>(
        &self,
        _node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        None
    }

    /// Corrects the draft that the table gave. If the result is `false`, the
    /// node is not a definition.
    fn refine<'tree>(&self, _draft: &mut Draft<'tree>, _context: &Context<'_, 'tree>) -> bool {
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported;

    /// Each node kind for which the walk calls `scope`.
    fn scope_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Gives the scope that a node opens when the node is not a definition.
    fn scope<'tree>(
        &self,
        _node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        None
    }

    /// Each node kind for which the walk calls `imports`.
    fn import_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Each node kind that `imports` compares with. The grammar lint examines these kinds.
    fn import_hook_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Adds the import bindings of one node to `out`.
    fn imports(&self, _node: Node<'_>, _context: &Context<'_, '_>, _out: &mut Vec<ImportBinding>) {}

    /// Each node kind for which the walk calls `module_name`.
    fn module_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// The package or the namespace that a node declares for the file.
    fn module_name(&self, _node: Node<'_>, _context: &Context<'_, '_>) -> Option<String> {
        None
    }

    /// Completes the facts that the syntax of one file does not give.
    /// `symbols` has the symbols of each file of this language. The result
    /// must not depend on the order of `symbols`.
    fn link(&self, _symbols: &mut [Linked<'_>]) {}

    /// The table that tells how the language writes calls and local names.
    fn call_syntax(&self) -> CallSyntax {
        CallSyntax::default()
    }

    /// Each node kind for which the walk calls `call`.
    fn call_kinds(&self) -> Vec<&'static str> {
        self.call_syntax().call_kinds()
    }

    /// Describes the call at a node. The default reads the table.
    fn call<'tree>(&self, node: Node<'tree>, context: &Context<'_, 'tree>) -> Option<Call<'tree>> {
        self.call_syntax().call(node, context)
    }

    /// The node kinds of a block that limits the local names that it
    /// declares. A language where a function limits its names gives none.
    fn block_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Tells if the names that the declaration at `node` binds end with the
    /// nearest block.
    fn is_block_local(&self, _node: Node<'_>) -> bool {
        true
    }

    /// The names that the node exports under a different name, as the local
    /// name and the exported name. For example, `export { a as b }` gives `a` and `b`.
    fn export_aliases(&self, _node: Node<'_>, _context: &Context<'_, '_>) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Each node kind for which the walk calls `locals`.
    fn local_kinds(&self) -> Vec<&'static str> {
        self.call_syntax().local_kinds()
    }

    /// Adds the local names that a node binds. The default reads the table.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        self.call_syntax().locals(node, context, out);
    }

    /// Each node kind that `call` or `locals` compares with, and that is not
    /// in the table. The grammar lint examines these kinds.
    fn call_hook_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

/// Gives the specification of a language.
pub fn language_spec(language: Language) -> &'static dyn LanguageSpec {
    match language {
        Language::Go => &lang::go::Go,
        Language::Rust => &lang::rust::Rust,
        Language::Python => &lang::python::Python,
        Language::JavaScript | Language::Jsx => &lang::ecma::JAVASCRIPT,
        Language::TypeScript | Language::Tsx => &lang::ecma::TYPESCRIPT,
        Language::Java => &lang::java::Java,
        Language::CSharp => &lang::csharp::CSharp,
        Language::C => &lang::c::C,
        Language::Cpp => &lang::cpp::Cpp,
        Language::Ruby => &lang::ruby::Ruby,
        Language::Php => &lang::php::Php,
        Language::Bash => &lang::bash::Bash,
    }
}

/// The first descendant of `node`, or `node` itself, that has one of the kinds.
pub fn find_descendant<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'tree>> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find_map(|child| find_descendant(child, kinds))
}

/// The nearest nodes below `node` that have one of the kinds. The search does
/// not go into a node that it collects.
pub fn collect_outermost<'tree>(node: Node<'tree>, kinds: &[&str], out: &mut Vec<Node<'tree>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            out.push(child);
        } else {
            collect_outermost(child, kinds, out);
        }
    }
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of the symbol range before the body. Without a body, the text is
/// the first line of the range.
fn signature(draft: &Draft<'_>, source: &[u8]) -> String {
    let start = draft.span.start_byte;
    let end = match draft.body {
        Some(body) if body.start_byte() > start => body.start_byte(),
        _ => draft.span.end_byte,
    };
    let text = String::from_utf8_lossy(&source[start..end]);
    match draft.body {
        Some(_) => collapse_whitespace(&text),
        None => collapse_whitespace(text.lines().next().unwrap_or("")),
    }
}

/// The comments that end on the line before the definition, with no line between them.
fn doc(node: Node<'_>, comment_kinds: &[&str], source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut next_start_row = node.start_position().row;
    let mut previous = node.prev_sibling();
    while let Some(comment) = previous {
        if !comment_kinds.contains(&comment.kind())
            || comment.end_position().row + 1 != next_start_row
        {
            break;
        }
        lines.push(comment.utf8_text(source).unwrap_or("").to_string());
        next_start_row = comment.start_position().row;
        previous = comment.prev_sibling();
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

/// A call with its byte range, its owner, its closure flag, and the byte
/// range of its receiver.
type RawCall<'tree> = (
    Call<'tree>,
    std::ops::Range<usize>,
    Option<usize>,
    bool,
    Option<(usize, usize)>,
);

struct Extractor<'a, 'tree> {
    spec: &'a dyn LanguageSpec,
    rules: HashMap<&'static str, DefinitionRule>,
    file: &'a str,
    source: &'a [u8],
    scopes: Vec<Scope<'tree>>,
    scope_depths: Vec<usize>,
    found: Vec<(Draft<'tree>, Exported)>,
    import_kinds: Vec<&'static str>,
    module_kinds: Vec<&'static str>,
    imports: Vec<ImportBinding>,
    modules: Vec<String>,
    /// For each scope, the node ID of the definition that opened it.
    scope_owners: Vec<Option<usize>>,
    /// The depths of the function values that have no name and contain the current node.
    closure_depths: Vec<usize>,
    call_kinds: Vec<&'static str>,
    local_kinds: Vec<&'static str>,
    /// One entry for each kind ID that the walk found.
    facts_of_kinds: Vec<Option<KindFacts>>,
    /// The nodes that contain the current node, from the root.
    ancestors: Vec<Node<'tree>>,
    return_type_field: &'static str,
    /// Each call with its start byte, its owner, and its closure flag.
    calls: Vec<RawCall<'tree>>,
    locals: Vec<(LocalName, Option<usize>, Option<std::ops::Range<usize>>)>,
    block_kinds: Vec<&'static str>,
    /// The local name and the exported name of each export of a name of this
    /// file under a different name.
    export_aliases: Vec<(String, String)>,
}

/// What the walk does for the nodes of one kind.
#[derive(Clone, Copy)]
struct KindFacts {
    imports: bool,
    module: bool,
    call: bool,
    local: bool,
    scope: bool,
    rule: Option<DefinitionRule>,
}

impl<'tree> Extractor<'_, 'tree> {
    /// The facts of the kind of `node`. The kind ID is the key, because the
    /// text of a kind needs a string comparison for each rule and each node.
    fn kind_facts(&mut self, node: Node<'tree>) -> KindFacts {
        let id = usize::from(node.kind_id());
        if self.facts_of_kinds.len() <= id {
            self.facts_of_kinds.resize(id + 1, None);
        }
        if let Some(facts) = self.facts_of_kinds[id] {
            return facts;
        }
        let kind = node.kind();
        let facts = KindFacts {
            imports: self.import_kinds.contains(&kind),
            module: self.module_kinds.contains(&kind),
            call: self.call_kinds.contains(&kind),
            local: self.local_kinds.contains(&kind),
            scope: self.spec.scope_kinds().contains(&kind),
            rule: self.rules.get(kind).copied(),
        };
        self.facts_of_kinds[id] = Some(facts);
        facts
    }

    /// The node ID of the nearest definition that contains the current node.
    fn owner(&self) -> Option<usize> {
        self.scope_owners.iter().rev().flatten().next().copied()
    }

    fn draft(&self, node: Node<'tree>, rule: &DefinitionRule) -> Option<(Draft<'tree>, Exported)> {
        let context = Context {
            file: self.file,
            source: self.source,
            scopes: &self.scopes,
            current: Some((node.id(), self.ancestors.last().copied())),
        };
        let name_node = match rule.name {
            NameRule::Field(field) => node.child_by_field_name(field),
            NameRule::Hook => self.spec.name_node(node, &context),
        }?;
        let container = context.container();
        let in_type = container.is_some_and(|scope| scope.role == ScopeRole::Type);
        let kind = if in_type {
            rule.kind_in_type
        } else {
            rule.kind
        };
        let parameters = node.child_by_field_name("parameters");
        let mut draft = Draft {
            node,
            name_node,
            name: context.text(name_node),
            kind,
            container: container.map(|scope| scope.name.clone()),
            container_path: context.path(),
            receiver_type: None,
            supertypes: Vec::new(),
            span: Span::of(&node),
            body: node.child_by_field_name("body"),
            role: ScopeRole::Function,
            parameter_count: parameters.map(|parameters| parameters.named_child_count()),
            parameter_types: parameters.map(|parameters| parameter_types(parameters, self.source)),
            owner: self.owner(),
            return_type: (!self.return_type_field.is_empty())
                .then(|| node.child_by_field_name(self.return_type_field))
                .flatten()
                .map(|result| context.text(result)),
            trait_implementation: false,
            default_export: false,
            function_value: false,
            c_linkage: false,
            defined_type: false,
            prepends: Vec::new(),
        };
        if draft.kind == SymbolKind::Module {
            draft.role = ScopeRole::Module;
        } else if draft.kind.is_type() {
            draft.role = ScopeRole::Type;
        }
        if !self.spec.refine(&mut draft, &context) || draft.name.is_empty() {
            return None;
        }
        // The declared type of a field is in the place of a result type.
        if draft.kind == SymbolKind::Field && draft.return_type.is_none() {
            draft.return_type = node
                .child_by_field_name("type")
                .map(|declared| context.text(declared));
        }
        let exported = self.spec.exported(&draft, &context);
        Some((draft, exported))
    }

    fn enter(&mut self, node: Node<'tree>, depth: usize) {
        // A keyword token can have the same kind string as a named node, for example `class`.
        if !node.is_named() {
            return;
        }
        let KindFacts {
            imports,
            module,
            call,
            local,
            scope: opens_scope,
            rule,
        } = self.kind_facts(node);
        if imports || module {
            let context = Context {
                file: self.file,
                source: self.source,
                scopes: &self.scopes,
                current: Some((node.id(), self.ancestors.last().copied())),
            };
            if imports {
                self.spec.imports(node, &context, &mut self.imports);
                self.export_aliases
                    .extend(self.spec.export_aliases(node, &context));
            }
            if module {
                self.modules.extend(self.spec.module_name(node, &context));
            }
        }
        if call || local {
            let context = Context {
                file: self.file,
                source: self.source,
                scopes: &self.scopes,
                current: Some((node.id(), self.ancestors.last().copied())),
            };
            let owner = self.owner();
            if let Some(call) = call.then(|| self.spec.call(node, &context)).flatten() {
                let via_closure = !self.closure_depths.is_empty();
                let receiver = self.spec.call_syntax().receiver_range(node);
                self.calls
                    .push((call, node.byte_range(), owner, via_closure, receiver));
            }
            if local {
                let mut names = Vec::new();
                self.spec.locals(node, &context, &mut names);
                let block = self.block_of(node);
                self.locals.extend(names.into_iter().map(|name| {
                    let block = (!name.is_parameter && name.visible.is_none())
                        .then(|| block.clone())
                        .flatten();
                    (name, owner, block)
                }));
            }
        }
        let definition = rule.and_then(|rule| self.draft(node, &rule));
        let function_value =
            rule.is_some_and(|rule| rule.name == NameRule::Hook && rule.kind != SymbolKind::Field);
        if definition.is_none() && function_value {
            self.closure_depths.push(depth);
        }
        let owner = definition.as_ref().map(|_| node.id());
        let scope = match definition {
            // A field opens no scope. A function value in the first value of
            // a field is in the scope of the type.
            Some((draft, exported)) if draft.kind == SymbolKind::Field => {
                self.found.push((draft, exported));
                None
            }
            Some((draft, exported)) => {
                let mut path = draft.container_path.clone();
                path.push(draft.name.clone());
                let scope = Scope {
                    node,
                    name: draft.name.clone(),
                    role: draft.role,
                    kind: Some(draft.kind),
                    exported: exported.clone(),
                    path,
                };
                self.found.push((draft, exported));
                Some(scope)
            }
            None if !opens_scope => None,
            None => self.spec.scope(
                node,
                &Context {
                    file: self.file,
                    source: self.source,
                    scopes: &self.scopes,
                    current: Some((node.id(), self.ancestors.last().copied())),
                },
            ),
        };
        if let Some(scope) = scope {
            self.scopes.push(scope);
            self.scope_depths.push(depth);
            self.scope_owners.push(owner);
        }
    }

    /// The bytes of the nearest block that contains the declaration at
    /// `node`, in a language where a block limits its names.
    fn block_of(&self, node: Node<'tree>) -> Option<std::ops::Range<usize>> {
        if self.block_kinds.is_empty() || !self.spec.is_block_local(node) {
            return None;
        }
        self.ancestors
            .iter()
            .rev()
            .find(|ancestor| self.block_kinds.contains(&ancestor.kind()))
            .map(|block| block.byte_range())
    }

    fn leave(&mut self, depth: usize) {
        if self.scope_depths.last() == Some(&depth) {
            self.scope_depths.pop();
            self.scopes.pop();
            self.scope_owners.pop();
        }
        if self.closure_depths.last() == Some(&depth) {
            self.closure_depths.pop();
        }
    }

    /// The walk does not use recursion, because generated source can have a very deep tree.
    fn walk(&mut self, root: Node<'tree>) {
        let mut cursor = root.walk();
        let mut depth = 0;
        loop {
            let node = cursor.node();
            self.enter(node, depth);
            if cursor.goto_first_child() {
                self.ancestors.push(node);
                depth += 1;
                continue;
            }
            loop {
                self.leave(depth);
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
                self.ancestors.pop();
                depth -= 1;
            }
        }
    }
}

/// The symbols of one file.
#[derive(Debug, Clone)]
pub struct FileSymbols {
    pub file: String,
    pub language: Language,
    pub symbols: Vec<Symbol>,
    /// One entry for each symbol, in the order of `symbols`.
    pub link_facts: Vec<LinkFacts>,
    /// The import bindings in source order. `resolve_imports` gives the targets.
    pub imports: Vec<ImportBinding>,
    /// The packages or the namespaces that the file declares.
    pub modules: Vec<String>,
    /// The call sites in source order. `resolve_references` gives the resolutions.
    pub references: Vec<Reference>,
    /// One entry for each reference, in the order of `references`.
    pub reference_facts: Vec<ReferenceFacts>,
    /// The parameters and the local variables of each function.
    pub locals: Vec<Local>,
    /// The module file that each qualifier of a call names, by the text of
    /// the qualifier. `resolve_imports` gives the entries.
    pub path_targets: BTreeMap<String, String>,
}

/// Completes the facts that need the symbols of a different file.
///
/// Give all the files of the index in one call. For example, a C++ definition
/// `int Scope::name() {}` gets its kind and its visibility from the
/// declaration in a header. The result does not depend on the order of `files`.
///
/// This function is the only step of the symbol extraction that reads more
/// than one file. When the module map of the shared index exists (migration
/// step 4), the module map must own this step.
pub fn link_files(files: &mut [FileSymbols]) {
    let mut sorted: Vec<&mut FileSymbols> = files.iter_mut().collect();
    sorted.sort_by(|left, right| left.file.cmp(&right.file));
    let mut languages: Vec<Language> = Vec::new();
    for file in &sorted {
        if !languages.contains(&file.language) {
            languages.push(file.language);
        }
    }
    for language in languages {
        let mut symbols: Vec<Linked<'_>> = sorted
            .iter_mut()
            .filter(|file| file.language == language)
            .flat_map(|file| {
                let FileSymbols {
                    symbols,
                    link_facts,
                    ..
                } = &mut **file;
                symbols
                    .iter_mut()
                    .zip(link_facts.iter())
                    .map(|(symbol, facts)| Linked { symbol, facts })
            })
            .collect();
        language_spec(language).link(&mut symbols);
    }
}

/// Extracts the symbols of one file in source order.
///
/// `file` is the path that the keys use.
pub fn extract_symbols(parsed: &ParseResult, file: &str) -> Vec<Symbol> {
    extract_file(parsed, file).symbols
}

/// Extracts the symbols of one file in source order, with the facts that
/// `link_files` uses.
pub fn extract_file(parsed: &ParseResult, file: &str) -> FileSymbols {
    let spec = language_spec(parsed.language);
    let mut extractor = Extractor {
        spec,
        rules: spec
            .definitions()
            .into_iter()
            .map(|rule| (rule.node_kind, rule))
            .collect(),
        file,
        source: &parsed.source,
        scopes: Vec::new(),
        scope_depths: Vec::new(),
        found: Vec::new(),
        import_kinds: spec.import_kinds(),
        module_kinds: spec.module_kinds(),
        imports: Vec::new(),
        modules: Vec::new(),
        scope_owners: Vec::new(),
        closure_depths: Vec::new(),
        call_kinds: spec.call_kinds(),
        local_kinds: spec.local_kinds(),
        facts_of_kinds: Vec::new(),
        ancestors: Vec::new(),
        return_type_field: spec.call_syntax().return_type_field,
        calls: Vec::new(),
        locals: Vec::new(),
        block_kinds: spec.block_kinds(),
        export_aliases: Vec::new(),
    };
    extractor.walk(parsed.root_node());

    let imports = extractor.imports;
    let modules = extractor.modules;
    let export_aliases = extractor.export_aliases;
    let mut found = extractor.found;
    found.sort_by_key(|(draft, _)| (draft.span.start_byte, draft.span.end_byte));
    let positions: HashMap<usize, usize> = found
        .iter()
        .enumerate()
        .map(|(position, (draft, _))| (draft.node.id(), position))
        .collect();
    let position_of = |owner: Option<usize>| owner.and_then(|id| positions.get(&id).copied());
    let locals: Vec<Local> = extractor
        .locals
        .into_iter()
        .map(|(name, owner, block)| Local {
            value_path: name
                .value_start
                .and_then(|start| value_path(&parsed.source, start)),
            name,
            owner: position_of(owner),
            block,
        })
        .collect();
    let parents: Vec<Option<usize>> = found
        .iter()
        .map(|(draft, _)| position_of(draft.owner))
        .collect();
    // The fields get their ordinals after the other symbols. Thus a field
    // does not change the key of a method with the same name.
    // The count is for each rendered key. Two different paths can render one
    // key, for example a Bash function `a.b` and a function `b` in `a`.
    // A name that ends in `~2` can also render the key of a second definition.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut ordinals = vec![0; found.len()];
    for fields in [false, true] {
        for (position, (draft, _)) in found.iter().enumerate() {
            if (draft.kind == SymbolKind::Field) == fields {
                let base = symbol_key(file, &draft.container_path, &draft.name, 1);
                let count = counts.entry(base).or_insert(0);
                *count += 1;
                while !used.insert(symbol_key(file, &draft.container_path, &draft.name, *count)) {
                    *count += 1;
                }
                ordinals[position] = *count;
            }
        }
    }
    let mut symbols = Vec::with_capacity(found.len());
    let mut link_facts = Vec::with_capacity(found.len());
    for (((draft, exported), parent), ordinal) in found.into_iter().zip(parents).zip(ordinals) {
        let export_aliases: Vec<String> = if parent.is_none() && draft.container_path.is_empty() {
            export_aliases
                .iter()
                .filter(|(local, _)| *local == draft.name)
                .map(|(_, alias)| alias.clone())
                .collect()
        } else {
            Vec::new()
        };
        symbols.push(Symbol {
            key: symbol_key(file, &draft.container_path, &draft.name, ordinal),
            signature: signature(&draft, &parsed.source),
            doc: doc(draft.node, spec.comment_kinds(), &parsed.source),
            name: draft.name,
            kind: draft.kind,
            container: draft.container,
            receiver_type: draft.receiver_type,
            supertypes: draft.supertypes,
            range: draft.span,
            name_range: Span::of(&draft.name_node),
            body_range: draft.body.as_ref().map(Span::of),
            exported,
            scip_symbol: None,
        });
        link_facts.push(LinkFacts {
            container_path: draft.container_path,
            parameter_count: draft.parameter_count,
            parameter_types: draft.parameter_types,
            parent,
            return_type: draft.return_type,
            trait_implementation: draft.trait_implementation,
            default_export: draft.default_export,
            function_value: draft.function_value,
            c_linkage: draft.c_linkage,
            defined_type: draft.defined_type,
            prepends: draft.prepends,
            export_aliases,
        });
    }
    let mut calls = extractor.calls;
    calls.sort_by_key(|(_, range, _, _, _)| range.start);
    let mut references = Vec::with_capacity(calls.len());
    let mut reference_facts = Vec::with_capacity(calls.len());
    for (call, range, owner, via_closure, receiver) in calls {
        let (start_byte, end_byte) = (range.start, range.end);
        let owner = position_of(owner);
        let text = String::from_utf8_lossy(&parsed.source[call.callee.name_node.byte_range()]);
        let name = match call.kind {
            ReferenceKind::Construct => split_type(&text).1,
            ReferenceKind::Call => text.to_string(),
        };
        references.push(Reference {
            file: file.to_string(),
            name_range: Span::of(&call.callee.name_node),
            name,
            kind: call.kind,
            enclosing: owner.map(|owner: usize| symbols[owner].key.clone()),
            via_closure,
            receiver: call.callee.receiver,
            qualifier: call.callee.qualifier,
            arity: call.arguments.len(),
            resolution: Resolution {
                status: ResolutionStatus::Unresolved,
                basis: None,
                candidates: Vec::new(),
                superseded: Vec::new(),
            },
        });
        let receiver_text = receiver
            .and_then(|(start, end)| parsed.source.get(start..end))
            .and_then(|text| std::str::from_utf8(text).ok());
        reference_facts.push(ReferenceFacts {
            start_byte,
            end_byte,
            owner,
            arguments: call.arguments,
            receiver,
            receiver_path: receiver_text
                .and_then(|text| name_path(text).or_else(|| indexed_path(text))),
            receiver_indexed: receiver_text.is_some_and(|text| indexed_path(text).is_some()),
            tried: parsed.source.get(end_byte) == Some(&b'?')
                && parsed.source.get(end_byte + 1) != Some(&b'.'),
            arrow: receiver.is_some_and(|(_, end)| {
                parsed
                    .source
                    .get(end..)
                    .is_some_and(|rest| rest.trim_ascii_start().starts_with(b"->"))
            }),
        });
    }
    FileSymbols {
        file: file.to_string(),
        language: parsed.language,
        symbols,
        link_facts,
        imports,
        modules,
        references,
        reference_facts,
        locals,
        path_targets: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::testing::rows;
    use crate::core::Language;

    /// The start line of a symbol is the line of its declaration. The lines
    /// of annotations, attributes, decorators, and comments before the
    /// declaration are not in the range.
    #[test]
    fn test_range_starts_at_the_declaration_in_each_language() {
        let cases: &[(Language, &str, &[&str])] = &[
            (
                Language::Python,
                "@cache\n@other(1)\ndef run():\n    pass\n\nclass Shop:\n    @staticmethod\n    def open():\n        pass\n",
                &[
                    "Function run - 3-4 exported",
                    "Class Shop - 6-9 exported",
                    "Method open Shop 8-9 exported",
                ],
            ),
            (
                Language::Rust,
                "/// Doc.\n#[test]\n#[ignore]\nfn run() {}\n\n#[derive(Debug)]\npub struct Shop;\n",
                &["Function run - 4-4 not_exported", "Struct Shop - 7-7 exported"],
            ),
            (
                Language::Java,
                "@Entity\npublic class Shop {\n    @Override\n    @Deprecated\n    public void run() {\n    }\n}\n",
                &["Class Shop - 2-7 exported", "Method run Shop 5-6 exported"],
            ),
            (
                Language::CSharp,
                "[Serializable]\npublic class Shop\n{\n    [Obsolete]\n    [Other(1)]\n    public void Run()\n    {\n    }\n}\n",
                &["Class Shop - 2-9 exported", "Method Run Shop 6-8 exported"],
            ),
            (
                Language::TypeScript,
                "@entity\n@other()\nexport class Shop {\n  @log\n  @bound\n  run(): void {}\n}\n",
                &["Class Shop - 3-7 exported", "Method run Shop 6-6 exported"],
            ),
            (
                Language::Php,
                "<?php\n#[Route('/a')]\nfunction run() {\n}\n\n#[Entity]\nclass Shop {\n    #[Get]\n    #[Other]\n    public function open() {\n    }\n}\n",
                &[
                    "Function run - 3-4 exported",
                    "Class Shop - 7-12 exported",
                    "Method open Shop 10-11 exported",
                ],
            ),
            (
                Language::Cpp,
                "[[nodiscard]]\nint run() {\n    return 1;\n}\n\ntemplate <typename T>\nT largest(T a, T b) {\n    return a;\n}\n",
                &["Function run - 2-4 exported", "Function largest - 7-9 exported"],
            ),
            (
                Language::Go,
                "package shop\n\n// Run runs.\n//go:noinline\nfunc Run() {\n}\n",
                &["Function Run - 5-6 exported"],
            ),
            (
                Language::Ruby,
                "# Runs.\nprivate def run\nend\n",
                &["Function run - 2-3 exported"],
            ),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .map(|(language, source, expected)| (language, rows(*language, source), expected))
            .filter(|(_, found, expected)| found != *expected)
            .map(|(language, found, _)| format!("{language:?}: {found:?}"))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use super::*;
    use crate::parser::Parser;

    pub(crate) fn symbols_in(language: Language, file: &str, source: &str) -> Vec<Symbol> {
        let parsed = Parser::new()
            .parse(source.as_bytes(), language, Path::new(file))
            .unwrap();
        let mut symbols = extract_symbols(&parsed, file);
        symbols.retain(|symbol| symbol.kind != SymbolKind::Field);
        symbols
    }

    /// The rows of the fields. The other helpers give the symbols that are not fields.
    pub(crate) fn fields(language: Language, source: &str) -> Vec<String> {
        let parsed = Parser::new()
            .parse(source.as_bytes(), language, Path::new("file"))
            .unwrap();
        let mut symbols = extract_symbols(&parsed, "file");
        symbols.retain(|symbol| symbol.kind == SymbolKind::Field);
        format_rows(&symbols)
    }

    pub(crate) fn symbols(language: Language, source: &str) -> Vec<Symbol> {
        symbols_in(language, "file", source)
    }

    pub(crate) fn rows_in(language: Language, file: &str, source: &str) -> Vec<String> {
        format_rows(&symbols_in(language, file, source))
    }

    pub(crate) fn rows(language: Language, source: &str) -> Vec<String> {
        rows_in(language, "file", source)
    }

    /// The rows of each file after `link_files`.
    pub(crate) fn linked_files(files: &[(Language, &str, &str)]) -> BTreeMap<String, Vec<String>> {
        let mut files: Vec<FileSymbols> = files
            .iter()
            .map(|(language, file, source)| {
                let parsed = Parser::new()
                    .parse(source.as_bytes(), *language, Path::new(file))
                    .unwrap();
                extract_file(&parsed, file)
            })
            .collect();
        link_files(&mut files);
        files
            .into_iter()
            .map(|mut file| {
                file.symbols
                    .retain(|symbol| symbol.kind != SymbolKind::Field);
                (file.file, format_rows(&file.symbols))
            })
            .collect()
    }

    /// One line for each symbol: `kind name container start-end exported`.
    pub(crate) fn format_rows(symbols: &[Symbol]) -> Vec<String> {
        symbols
            .iter()
            .map(|symbol| {
                let exported = match &symbol.exported {
                    Exported::Exported => "exported",
                    Exported::NotExported => "not_exported",
                    Exported::Unknown(_) => "unknown",
                };
                format!(
                    "{:?} {} {} {}-{} {exported}",
                    symbol.kind,
                    symbol.name,
                    symbol.container.as_deref().unwrap_or("-"),
                    symbol.range.start_line,
                    symbol.range.end_line,
                )
            })
            .collect()
    }

    pub(crate) fn keys(language: Language, source: &str) -> Vec<String> {
        symbols(language, source)
            .into_iter()
            .map(|symbol| symbol.key)
            .collect()
    }
}
