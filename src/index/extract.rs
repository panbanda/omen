//! Symbol extraction. One walk of the syntax tree gives the symbols of a file.
//!
//! A [`LanguageSpec`] supplies a table of definition node kinds. The hook
//! methods of the specification are only for syntax that the table cannot
//! describe.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use tree_sitter::Node;

use crate::parser::Walk;

use super::calls::{split_type, Argument, Call, CallSyntax, LocalName, ValueSyntax, ValueUse};
use super::lang;
use super::members::{self, MemberFacts, PendingBlock, Recorder, SymbolRow};
use super::model::{
    symbol_key, Exported, ImportBinding, ImportKind, ImportTarget, Receiver, Reference,
    ReferenceKind, Resolution, ResolutionStatus, SharedStr, Span, Symbol, SymbolKind, TestRole,
    NO_ARITY,
};
use super::test_code::{self, MarkContext, MarkTable, TestMark};
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
    /// The names of the type parameters of a generic definition, in order.
    pub type_parameters: Vec<String>,
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
    /// The name of the class that the type extends.
    pub bases: Vec<String>,
    /// The function has the name of the variable or the field that holds it.
    pub named_by_binding: bool,
    /// The `cfg` attributes that control the definition.
    pub condition: Option<Condition>,
    /// A method that a call defines without a body of its own, as Ruby
    /// `attr_reader :name` does. One node can define several of them.
    pub generated: Option<GeneratedFor>,
    /// The wrapper type of the `self` parameter of a Rust method, as `Arc`
    /// for `self: &Arc<Self>`.
    pub self_wrapper: Option<&'static str>,
    /// A Ruby method of the class object, as `def self.name` and a method in
    /// `class << self` are. Its instance variables are those of the class
    /// object.
    pub singleton: bool,
}

/// The objects that have a method that a call generates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedFor {
    /// The objects of the type, as Ruby `attr_reader :name` in a class body gives.
    Instances,
    /// The type itself, as Ruby `attr_reader :name` in `class << self` gives.
    Type,
}

/// The `cfg` attributes that control a Rust definition. Two definitions of
/// one name in one scope are legal only when no build has the two.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Condition {
    /// For the definition and for each item that contains it and has `cfg`
    /// attributes, from the outer item, the ID of the text of these
    /// attributes without white space. The IDs are of one file.
    pub attributes: Vec<usize>,
    /// The ID of the text of the nearest `impl` block before its body.
    pub implementation: Option<usize>,
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
    /// The names of the type parameters of a generic definition, in order.
    pub type_parameters: Vec<String>,
    pub trait_implementation: bool,
    pub default_export: bool,
    pub function_value: bool,
    pub c_linkage: bool,
    pub defined_type: bool,
    pub prepends: Vec<String>,
    pub bases: Vec<String>,
    pub named_by_binding: bool,
    /// A test runner calls the definition when the file of the definition is
    /// test code. See `test_code::MarkKind::EntryInTestCode`.
    pub test_entry_in_test_code: bool,
    pub condition: Option<Condition>,
    pub self_wrapper: Option<&'static str>,
    /// See `Draft::generated`.
    pub generated: Option<GeneratedFor>,
    /// The names, different from its own, under which the module exports
    /// this top-level symbol, as `export { a as b }` gives `b` to `a`.
    pub export_aliases: Vec<String>,
    /// See `Draft::singleton`.
    pub singleton: bool,
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
    /// A construction whose declared type is optional. The resolver gives it
    /// a candidate only when each type of the repository with the name is a class.
    pub nullable_target: bool,
    /// A function reference whose name is a variable. Only a function value
    /// that the variable holds is its candidate.
    pub variable: bool,
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
    let blank = rest[length..]
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let after = &rest[length + blank..];
    let comment = ["//", "/*", "#"]
        .iter()
        .any(|start| after.starts_with(start.as_bytes()));
    if !comment
        && !after
            .first()
            .is_none_or(|byte| b"\n\r;,){}:".contains(byte))
    {
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
    /// The first value is the object of the method in a wrapper, as Rust
    /// `Arc::new(self)` is. The text is the name of the wrapper type. It is
    /// empty for `self.clone()`.
    pub self_wrapper: Option<String>,
}

/// One symbol and its link facts.
pub struct Linked<'a> {
    pub symbol: &'a mut Symbol,
    pub facts: &'a LinkFacts,
}

/// A definition or a function value that a returned value belongs to. The
/// walk reads its facts once, when it enters the node.
#[derive(Debug, Clone, Copy)]
pub struct ReturnOwner<'tree> {
    pub node: Node<'tree>,
    /// The owner is asynchronous, so that a returned value is not the result of the owner.
    pub is_asynchronous: bool,
    /// The node of the declared type of the result.
    pub return_type: Option<Node<'tree>>,
}

/// The facts of a node that a language computes once, by node ID and a number
/// that the language chooses. A language reads the cost of a node that has
/// many children once, and not for each child.
pub type Memo = RefCell<HashMap<(usize, u8), Rc<dyn Any>>>;

pub struct Context<'a, 'tree> {
    /// The path of the file, as the keys have it.
    pub file: &'a str,
    pub source: &'a [u8],
    pub scopes: &'a [Scope<'tree>],
    /// The ancestors of the node at which the walk is, the nearest last.
    pub ancestors: &'a [Node<'tree>],
    /// The nearest ancestor whose kind is one of `LanguageSpec::return_owner_kinds`.
    pub return_owner: Option<ReturnOwner<'tree>>,
    /// The members recorder of the file, which has the type parameters read so far.
    pub members: Option<&'a Recorder>,
    /// The ID of the node at which the walk is, and the parent of that node.
    pub current: Option<(usize, Option<Node<'tree>>)>,
    /// The facts that a language reads once for a node and uses for many nodes.
    pub memo: &'a Memo,
}

impl<'tree> Context<'_, 'tree> {
    /// The parent of `node`. The walk knows the parent of its current node
    /// and of each ancestor. For a different node, the tree searches from the
    /// root, which costs the depth of the node.
    pub fn parent(&self, node: Node<'tree>) -> Option<Node<'tree>> {
        if let Some((current, parent)) = self.current {
            if current == node.id() {
                return parent;
            }
        }
        match self.ancestor_position(node) {
            Some(0) => None,
            Some(position) => Some(self.ancestors[position - 1]),
            None => node.parent(),
        }
    }

    /// The position of `node` in `ancestors`. Each ancestor contains the next
    /// one, so the ancestors are in the order of their start, and then in the
    /// reverse order of their end. A binary search finds the ancestors with
    /// the range of the node. Only a few nodes can have the same range,
    /// because a grammar cannot nest nodes without text without limit.
    fn ancestor_position(&self, node: Node<'tree>) -> Option<usize> {
        let range = |ancestor: &Node<'_>| {
            (
                ancestor.start_byte(),
                std::cmp::Reverse(ancestor.end_byte()),
            )
        };
        let wanted = range(&node);
        let first = self
            .ancestors
            .partition_point(|ancestor| range(ancestor) < wanted);
        self.ancestors[first..]
            .iter()
            .take_while(|ancestor| range(ancestor) == wanted)
            .position(|ancestor| ancestor.id() == node.id())
            .map(|offset| first + offset)
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
            type_only: false,
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

    /// Each node kind for which the walk calls `generated_methods`.
    fn generated_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// The methods that a node defines without a body of its own, as Ruby
    /// `attr_accessor :name` defines `name` and `name=`. Each method has the
    /// node of its name and the name. Such a method contains no definition.
    fn generated_methods<'tree>(
        &self,
        _node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Vec<(Node<'tree>, String, GeneratedFor)> {
        Vec::new()
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

    /// The bytes in which the parameters that the node at `node` binds are
    /// visible, when the node is a function value that opens no scope of its
    /// own. Without a range, a parameter is visible in the whole owner.
    fn parameter_block(
        &self,
        node: Node<'_>,
        ancestors: &[Node<'_>],
    ) -> Option<std::ops::Range<usize>> {
        let nearest = std::iter::once(node).chain(ancestors.iter().rev().take(2).copied());
        nearest
            .filter(|candidate| self.lambda_kinds().contains(&candidate.kind()))
            .map(|lambda| lambda.byte_range())
            .next()
    }

    /// The node kinds of the definitions and function values that a returned
    /// value belongs to. The walk keeps the nearest one for a construction
    /// whose type the return type gives.
    fn return_owner_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// The facts of a return owner. The walk asks once for each owner.
    fn return_owner<'tree>(&self, owner: Node<'tree>, _source: &[u8]) -> ReturnOwner<'tree> {
        ReturnOwner {
            node: owner,
            is_asynchronous: false,
            return_type: None,
        }
    }

    /// The node kinds of the function values that have parameters of their own.
    fn lambda_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// Tells if the names that the declaration at `node` binds end with the
    /// nearest block.
    fn is_block_local(&self, _parent: Option<Node<'_>>) -> bool {
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

    /// The table that tells where the language uses a function as a value.
    fn value_syntax(&self) -> ValueSyntax {
        ValueSyntax::default()
    }

    /// Each node kind for which the walk calls `function_values`.
    fn value_kinds(&self) -> Vec<&'static str> {
        self.value_syntax().position_kinds()
    }

    /// Adds each use of a name as a value at a node. The default reads the
    /// table. The resolver tells if the name is a function.
    fn function_values<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
        out: &mut Vec<ValueUse<'tree>>,
    ) {
        self.value_syntax()
            .values(&self.call_syntax(), node, context, out);
    }

    /// Each node kind that `function_values` compares with, and that is not
    /// in the table. The grammar lint examines these kinds.
    fn value_hook_kinds(&self) -> Vec<&'static str> {
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
    crate::parser::find_preorder(node, |node| kinds.contains(&node.kind()))
}

/// The nearest nodes below `node` that have one of the kinds. The search does
/// not go into a node that it collects.
pub fn collect_outermost<'tree>(node: Node<'tree>, kinds: &[&str], out: &mut Vec<Node<'tree>>) {
    crate::parser::walk_preorder(node, |descendant| {
        if descendant != node && kinds.contains(&descendant.kind()) {
            out.push(descendant);
            return Walk::SkipChildren;
        }
        Walk::Children
    });
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
fn doc(node: Node<'_>, comment_before: &HashMap<usize, Node<'_>>, source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut next_start_row = node.start_position().row;
    let mut previous = comment_before.get(&node.id());
    while let Some(comment) = previous {
        if comment.end_position().row + 1 != next_start_row {
            break;
        }
        lines.push(comment.utf8_text(source).unwrap_or("").to_string());
        next_start_row = comment.start_position().row;
        previous = comment_before.get(&comment.id());
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

/// What one declaration of a local name tells about the name.
#[derive(Clone, Copy)]
struct BoundName<'a> {
    /// The first byte of the value of the name. It is 0 for a name that has
    /// no value, as a parameter is.
    first_value: usize,
    /// The bytes of the block in which the name is visible. See `Local::block`.
    block: Option<&'a std::ops::Range<usize>>,
    /// The declaration has a type or a value.
    has_type_or_value: bool,
}

/// A range of bytes in `ValueSites::text`.
type TextRange = (u32, u32);

/// The expression before the name of a value.
#[derive(Debug, Clone, Copy)]
enum SiteReceiver {
    None,
    SelfValue,
    Super,
    Name(TextRange),
    Expression,
}

/// One use of a name as a value, before the resolution.
#[derive(Debug, Clone, Copy)]
struct ValueSite {
    name: TextRange,
    name_range: Span,
    start_byte: u32,
    end_byte: u32,
    /// The position in `FileSymbols::symbols` of the enclosing definition.
    owner: Option<u32>,
    receiver: SiteReceiver,
    qualifier: Option<TextRange>,
    /// The byte range of the receiver expression in the source.
    receiver_range: Option<(u32, u32)>,
    /// The source text of the receiver expression.
    receiver_text: Option<TextRange>,
    arrow: bool,
    via_closure: bool,
    variable: bool,
}

/// The uses of a name as a value in one file, before the resolution.
///
/// A repository has many uses, and the resolver keeps few of them. The uses
/// of a file are thus in two blocks of memory, one for the records and one
/// for the texts. A record of its own for each use, with a string for each
/// text, left the memory in small parts that the process did not use again.
#[derive(Debug, Clone, Default)]
pub struct ValueSites {
    text: String,
    sites: Vec<ValueSite>,
}

impl ValueSites {
    pub fn len(&self) -> usize {
        self.sites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    fn add_text(&mut self, text: &str) -> TextRange {
        let start = self.text.len() as u32;
        self.text.push_str(text);
        (start, self.text.len() as u32)
    }

    fn text_of(&self, range: TextRange) -> &str {
        &self.text[range.0 as usize..range.1 as usize]
    }

    /// The path before the name of each use that has one, as `jobs` in `jobs::worker`.
    pub fn qualifiers(&self) -> impl Iterator<Item = &str> {
        self.sites
            .iter()
            .filter_map(|site| site.qualifier.map(|qualifier| self.text_of(qualifier)))
    }

    /// The name of the use at `position`.
    pub fn name(&self, position: usize) -> &str {
        self.text_of(self.sites[position].name)
    }

    /// Tells if the use at `position` is one name, without a receiver and without a path.
    pub fn is_bare(&self, position: usize) -> bool {
        let site = &self.sites[position];
        matches!(site.receiver, SiteReceiver::None) && site.qualifier.is_none()
    }

    /// Keeps the uses for which `keep` is true. `keep` gets the position of the use.
    pub fn retain(&mut self, mut keep: impl FnMut(usize) -> bool) {
        let mut position = 0;
        self.sites.retain(|_| {
            position += 1;
            keep(position - 1)
        });
        self.sites.shrink_to_fit();
    }

    /// The reference and the resolver facts of the use at `position`. `file`
    /// is the path of the file, and `symbols` has the symbols of the file.
    pub fn reference(
        &self,
        position: usize,
        file: &SharedStr,
        symbols: &[Symbol],
    ) -> (Reference, ReferenceFacts) {
        let site = &self.sites[position];
        let receiver = match site.receiver {
            SiteReceiver::None => None,
            SiteReceiver::SelfValue => Some(Receiver::SelfValue),
            SiteReceiver::Super => Some(Receiver::Super),
            SiteReceiver::Name(name) => Some(Receiver::Name(self.text_of(name).to_string())),
            SiteReceiver::Expression => Some(Receiver::Expression),
        };
        let receiver_text = site.receiver_text.map(|text| self.text_of(text));
        let reference = Reference {
            file: file.clone(),
            name_range: site.name_range,
            name: self.text_of(site.name).to_string(),
            kind: ReferenceKind::FunctionReference,
            enclosing: site.owner.map(|owner| symbols[owner as usize].key.clone()),
            via_closure: site.via_closure,
            receiver,
            qualifier: site
                .qualifier
                .map(|qualifier| self.text_of(qualifier).to_string()),
            arity: NO_ARITY,
            resolution: Resolution {
                status: ResolutionStatus::Unresolved,
                basis: None,
                candidates: Vec::new(),
                superseded: Vec::new(),
            },
        };
        let facts = ReferenceFacts {
            start_byte: site.start_byte as usize,
            end_byte: site.end_byte as usize,
            owner: site.owner.map(|owner| owner as usize),
            arguments: Vec::new(),
            receiver: site
                .receiver_range
                .map(|(start, end)| (start as usize, end as usize)),
            receiver_path: receiver_text
                .and_then(|text| name_path(text).or_else(|| indexed_path(text))),
            receiver_indexed: receiver_text.is_some_and(|text| indexed_path(text).is_some()),
            tried: false,
            arrow: site.arrow,
            nullable_target: false,
            variable: site.variable,
        };
        (reference, facts)
    }
}

/// A decorator of an `export` statement that declares a class.
struct Decorated {
    depth: usize,
    /// The node ID of the class.
    class: Option<usize>,
    /// The owner of the decorator itself, outside the class.
    owner: Option<usize>,
}

/// A run of decorators in the body of a class, and the lengths of the
/// collected facts when the run began.
struct PendingDecorators {
    /// The node ID of the body of the class.
    body: usize,
    body_depth: usize,
    /// The owner of the calls of the run before the walk enters the method.
    owner: Option<usize>,
    calls: usize,
    values: usize,
    locals: usize,
}

struct Extractor<'a, 'tree> {
    spec: &'a dyn LanguageSpec,
    /// The comment before each node that follows a comment, by the id of the node.
    /// `Node::prev_sibling` searches from the root, so the walk records the sibling.
    comment_before: HashMap<usize, Node<'tree>>,
    rules: HashMap<&'static str, DefinitionRule>,
    file: &'a str,
    source: &'a [u8],
    scopes: Vec<Scope<'tree>>,
    scope_depths: Vec<usize>,
    found: Vec<(Draft<'tree>, Exported)>,
    import_kinds: Vec<&'static str>,
    generated_kinds: Vec<&'static str>,
    module_kinds: Vec<&'static str>,
    imports: Vec<ImportBinding>,
    modules: Vec<String>,
    /// For each scope, the node ID of the definition that opened it.
    scope_owners: Vec<Option<usize>>,
    /// The depths of the function values that have no name and contain the current node.
    closure_depths: Vec<usize>,
    /// The decorators of `export` statements that contain the current node.
    decorators: Vec<Decorated>,
    /// The runs of decorators in the body of a class whose method the walk
    /// has not entered.
    pending_decorators: Vec<PendingDecorators>,
    call_kinds: Vec<&'static str>,
    local_kinds: Vec<&'static str>,
    /// One entry for each kind ID that the walk found.
    facts_of_kinds: Vec<Option<KindFacts>>,
    /// The nodes that contain the current node, from the root.
    ancestors: Vec<Node<'tree>>,
    return_owner_kinds: &'static [&'static str],
    /// The ancestors that have a kind of `return_owner_kinds`, the nearest last.
    return_owners: Vec<ReturnOwner<'tree>>,
    return_type_field: &'static str,
    /// Each call with its start byte, its owner, and its closure flag.
    calls: Vec<RawCall<'tree>>,
    value_kinds: Vec<&'static str>,
    /// Each use of a name as a value, with its owner and its closure flag.
    values: Vec<(ValueUse<'tree>, Option<usize>, bool)>,
    locals: Vec<(LocalName, Option<usize>, Option<std::ops::Range<usize>>)>,
    block_kinds: Vec<&'static str>,
    /// The ancestors of a kind in `block_kinds`, the nearest last.
    blocks: Vec<Node<'tree>>,
    /// The local name and the exported name of each export of a name of this
    /// file under a different name.
    export_aliases: Vec<(String, String)>,
    language: Language,
    members: Option<Recorder>,
    memo: Memo,
    test_mark_kinds: &'static [&'static str],
    /// The ancestors that have a kind of `test_code::owner_kinds`, the nearest last.
    test_owners: Vec<test_code::Owner<'tree>>,
    test_callee_roots: test_code::CalleeRoots<'tree>,
    test_marks: Vec<TestMark>,
    in_test_file: bool,
}

/// What the walk does for the nodes of one kind.
#[derive(Clone, Copy)]
struct KindFacts {
    imports: bool,
    module: bool,
    call: bool,
    local: bool,
    value: bool,
    scope: bool,
    test_mark: bool,
    generated: bool,
    rule: Option<DefinitionRule>,
    /// The roles of the kind in the facts of the members, as `members::kind_bits` gives.
    member_bits: u8,
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
            value: self.value_kinds.contains(&kind),
            scope: self.spec.scope_kinds().contains(&kind),
            test_mark: self.test_mark_kinds.contains(&kind),
            generated: self.generated_kinds.contains(&kind),
            rule: self.rules.get(kind).copied(),
            member_bits: self.members.as_ref().map_or(0, |recorder| {
                members::kind_bits(self.language, &recorder.syntax, kind)
            }),
        };
        self.facts_of_kinds[id] = Some(facts);
        facts
    }

    /// The node ID of the nearest definition that contains the current node.
    fn owner(&self) -> Option<usize> {
        self.scope_owners.iter().rev().flatten().next().copied()
    }

    /// Gives the calls in a decorator the definition that it decorates as
    /// their caller, when the decorator is outside the node of that
    /// definition. TypeScript puts the decorator of
    /// `@Decorator() export class Name` in the `export` statement, and the
    /// decorator of a method in the body of the class before the method.
    ///
    /// The method is not known until the walk enters it. Thus the calls of
    /// a run of decorators in the body of a class first have the class as
    /// their caller, and the walk changes the caller when it enters the
    /// method that follows the run.
    fn track_decorators(&mut self, node: Node<'tree>, depth: usize) {
        let parent = self.ancestors.last().copied();
        let in_parent =
            |run: &PendingDecorators| parent.is_some_and(|parent| parent.id() == run.body);
        let consumes = !["decorator", "comment"].contains(&node.kind());
        if consumes && self.pending_decorators.last().is_some_and(in_parent) {
            let run = self.pending_decorators.pop().expect("a pending run");
            if node.kind() == "method_definition" {
                // A definition in a decorator argument stays the caller of
                // the calls in its body.
                let method = Some(node.id());
                for call in &mut self.calls[run.calls..] {
                    if call.2 == run.owner {
                        call.2 = method;
                    }
                }
                for value in &mut self.values[run.values..] {
                    if value.1 == run.owner {
                        value.1 = method;
                    }
                }
                for local in &mut self.locals[run.locals..] {
                    if local.1 == run.owner {
                        local.1 = method;
                    }
                }
            }
        }
        if node.kind() != "decorator" {
            return;
        }
        let Some(parent) = parent else {
            return;
        };
        match parent.kind() {
            "export_statement" => {
                let class = parent
                    .child_by_field_name("declaration")
                    .filter(|declaration| declaration.kind().ends_with("class_declaration"))
                    .map(|declaration| declaration.id());
                self.decorators.push(Decorated {
                    depth,
                    class,
                    owner: self.owner(),
                });
            }
            "class_body" if !self.pending_decorators.last().is_some_and(in_parent) => {
                self.pending_decorators.push(PendingDecorators {
                    body: parent.id(),
                    body_depth: depth - 1,
                    owner: self.owner(),
                    calls: self.calls.len(),
                    values: self.values.len(),
                    locals: self.locals.len(),
                });
            }
            _ => {}
        }
    }

    fn context(&self, node: Node<'tree>) -> Context<'_, 'tree> {
        Context {
            file: self.file,
            source: self.source,
            scopes: &self.scopes,
            ancestors: &self.ancestors,
            return_owner: self.return_owners.last().copied(),
            members: self.members.as_ref(),
            current: Some((node.id(), self.ancestors.last().copied())),
            memo: &self.memo,
        }
    }

    /// The methods that `node` defines without a body. Each draft has `node`
    /// as its node, and the span of its name.
    fn generated_drafts(&self, node: Node<'tree>) -> Vec<(Draft<'tree>, Exported)> {
        let context = self.context(node);
        let in_type = context
            .container()
            .is_some_and(|scope| scope.role == ScopeRole::Type);
        if !in_type {
            return Vec::new();
        }
        self.spec
            .generated_methods(node, &context)
            .into_iter()
            .map(|(name_node, name, owner)| {
                let mut draft = self.new_draft(node, name_node, SymbolKind::Method, &context);
                draft.name = name;
                draft.span = Span::of(&name_node);
                draft.body = None;
                draft.parameter_count = None;
                draft.parameter_types = None;
                draft.return_type = None;
                draft.generated = Some(owner);
                let exported = self.spec.exported(&draft, &context);
                (draft, exported)
            })
            .collect()
    }

    fn new_draft(
        &self,
        node: Node<'tree>,
        name_node: Node<'tree>,
        kind: SymbolKind,
        context: &Context<'_, 'tree>,
    ) -> Draft<'tree> {
        let parameters = node.child_by_field_name("parameters");
        Draft {
            node,
            name_node,
            name: context.text(name_node),
            kind,
            container: context.container().map(|scope| scope.name.clone()),
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
            type_parameters: Vec::new(),
            trait_implementation: false,
            default_export: false,
            function_value: false,
            c_linkage: false,
            defined_type: false,
            prepends: Vec::new(),
            bases: Vec::new(),
            named_by_binding: false,
            condition: None,
            self_wrapper: None,
            singleton: false,
            generated: None,
        }
    }

    fn draft(&self, node: Node<'tree>, rule: &DefinitionRule) -> Option<(Draft<'tree>, Exported)> {
        let context = self.context(node);
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
        let mut draft = self.new_draft(node, name_node, kind, &context);
        draft.named_by_binding = rule.name == NameRule::Hook
            && rule.kind != SymbolKind::Field
            && (name_node.start_byte() < node.start_byte()
                || name_node.end_byte() > node.end_byte());
        if draft.kind == SymbolKind::Module {
            draft.role = ScopeRole::Module;
        } else if draft.kind.is_type() {
            draft.role = ScopeRole::Type;
        }
        if !self.spec.refine(&mut draft, &context) || draft.name.is_empty() {
            return None;
        }
        if self.members.is_some() && draft.kind.is_type() {
            draft.bases = members::bases(self.language, node, self.source);
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
        self.track_decorators(node, depth);
        let KindFacts {
            imports,
            module,
            call,
            local,
            value,
            scope: opens_scope,
            test_mark,
            generated,
            rule,
            member_bits,
        } = self.kind_facts(node);
        if test_mark {
            test_code::marks(
                node,
                &MarkContext {
                    language: self.language,
                    source: self.source,
                    ancestors: &self.ancestors,
                    owners: &self.test_owners,
                    callee_roots: &self.test_callee_roots,
                    in_test_file: self.in_test_file,
                },
                &mut self.test_marks,
            );
        }
        if member_bits != 0 {
            if let Some(recorder) = self.members.as_mut() {
                recorder.visit(
                    node,
                    member_bits,
                    self.source,
                    &self.scopes,
                    &self.ancestors,
                );
            }
        }
        if imports || module {
            let context = Context {
                file: self.file,
                source: self.source,
                scopes: &self.scopes,
                ancestors: &self.ancestors,
                return_owner: self.return_owners.last().copied(),
                members: self.members.as_ref(),
                current: Some((node.id(), self.ancestors.last().copied())),
                memo: &self.memo,
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
        if call || local || value {
            let context = Context {
                file: self.file,
                source: self.source,
                scopes: &self.scopes,
                ancestors: &self.ancestors,
                return_owner: self.return_owners.last().copied(),
                members: self.members.as_ref(),
                current: Some((node.id(), self.ancestors.last().copied())),
                memo: &self.memo,
            };
            let owner = self.owner();
            // A definition in the decorator argument stays the owner.
            let owner = self
                .decorators
                .last()
                .filter(|decorated| decorated.owner == owner)
                .and_then(|decorated| decorated.class)
                .or(owner);
            if let Some(call) = call.then(|| self.spec.call(node, &context)).flatten() {
                let via_closure = !self.closure_depths.is_empty();
                let receiver = self.spec.call_syntax().receiver_range(node);
                self.calls
                    .push((call, node.byte_range(), owner, via_closure, receiver));
            }
            if value {
                let mut uses = Vec::new();
                self.spec.function_values(node, &context, &mut uses);
                let via_closure = !self.closure_depths.is_empty();
                self.values
                    .extend(uses.into_iter().map(|used| (used, owner, via_closure)));
            }
            if local {
                let mut names = Vec::new();
                self.spec.locals(node, &context, &mut names);
                let block = self.block_of();
                let parameter_block = self.spec.parameter_block(node, &self.ancestors);
                self.locals.extend(names.into_iter().map(|name| {
                    let block = if name.is_parameter {
                        parameter_block.clone()
                    } else {
                        (name.visible.is_none()).then(|| block.clone()).flatten()
                    };
                    (name, owner, block)
                }));
            }
        }
        if generated {
            let drafts = self.generated_drafts(node);
            self.found.extend(drafts);
        }
        let definition = rule.and_then(|rule| self.draft(node, &rule));
        let function_value =
            rule.is_some_and(|rule| rule.name == NameRule::Hook && rule.kind != SymbolKind::Field);
        if definition.is_none() && function_value {
            self.closure_depths.push(depth);
        }
        let owner = definition.as_ref().map(|_| node.id());
        let defines = definition.is_some();
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
                    ancestors: &self.ancestors,
                    return_owner: self.return_owners.last().copied(),
                    members: self.members.as_ref(),
                    current: Some((node.id(), self.ancestors.last().copied())),
                    memo: &self.memo,
                },
            ),
        };
        if let Some(scope) = scope {
            if !defines && scope.role == ScopeRole::Type {
                if let Some(recorder) = self.members.as_mut() {
                    recorder.blocks.push(PendingBlock {
                        node: node.id(),
                        path: scope.path.clone(),
                        end_line: node.end_position().row as u32 + 1,
                    });
                }
            }
            self.scopes.push(scope);
            self.scope_depths.push(depth);
            self.scope_owners.push(owner);
        }
    }

    /// The bytes of the nearest block that contains the declaration at
    /// `node`, in a language where a block limits its names.
    fn block_of(&self) -> Option<std::ops::Range<usize>> {
        if self.block_kinds.is_empty() || !self.spec.is_block_local(self.ancestors.last().copied())
        {
            return None;
        }
        self.blocks.last().map(|block| block.byte_range())
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
        if self
            .decorators
            .last()
            .is_some_and(|decorated| decorated.depth == depth)
        {
            self.decorators.pop();
        }
        if self
            .pending_decorators
            .last()
            .is_some_and(|run| run.body_depth == depth)
        {
            self.pending_decorators.pop();
        }
    }

    /// The walk does not use recursion, because generated source can have a very deep tree.
    fn walk(&mut self, root: Node<'tree>) {
        let mut cursor = root.walk();
        let mut depth = 0;
        let comment_kinds = self.spec.comment_kinds();
        let mut previous: Option<Node<'tree>> = None;
        loop {
            let node = cursor.node();
            if let Some(comment) =
                previous.filter(|sibling| comment_kinds.contains(&sibling.kind()))
            {
                self.comment_before.insert(node.id(), comment);
            }
            self.enter(node, depth);
            previous = None;
            if cursor.goto_first_child() {
                if self.return_owner_kinds.contains(&node.kind()) {
                    let owner = self.spec.return_owner(node, self.source);
                    self.return_owners.push(owner);
                }
                if self.block_kinds.contains(&node.kind()) {
                    self.blocks.push(node);
                }
                test_code::push_ancestor(
                    node,
                    self.language,
                    self.source,
                    &mut self.ancestors,
                    &mut self.test_owners,
                );
                depth += 1;
                continue;
            }
            loop {
                self.leave(depth);
                let left = cursor.node();
                if cursor.goto_next_sibling() {
                    previous = Some(left);
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
                if let Some(left) = self.ancestors.last().copied() {
                    if self
                        .return_owners
                        .last()
                        .is_some_and(|owner| owner.node.id() == left.id())
                    {
                        self.return_owners.pop();
                    }
                    if self
                        .blocks
                        .last()
                        .is_some_and(|block| block.id() == left.id())
                    {
                        self.blocks.pop();
                    }
                }
                test_code::pop_ancestor(&mut self.ancestors, &mut self.test_owners);
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
    /// The uses of a function as a value, in source order. They are not
    /// calls, and they are not in `references`. `resolve_references` gives
    /// the list: a use is in it only when it has a candidate.
    pub function_references: Vec<Reference>,
    /// Each use of a name as a value that can name a function.
    /// `resolve_references` reads the uses and empties the list.
    pub value_sites: ValueSites,
    /// The parameters and the local variables of each function.
    pub locals: Vec<Local>,
    /// The module file that each qualifier of a call names, by the text of
    /// the qualifier. `resolve_imports` gives the entries.
    pub path_targets: BTreeMap<String, String>,
    /// The names that the file binds again in a way that is not a symbol:
    /// a C macro, a Bash alias, a property of the global object of
    /// ECMAScript, and a Ruby singleton class of a constant.
    pub rebound: BTreeSet<String>,
    /// The facts that the class metrics use.
    pub members: MemberFacts,
    /// The BLAKE3 hash of the bytes that gave these facts.
    pub content_hash: [u8; 32],
    /// The byte ranges that the syntax makes test code, in order. No range is
    /// in a different range. See `crate::index::test_code`.
    pub test_ranges: Vec<std::ops::Range<usize>>,
    /// The first line and the last line of each range of `test_ranges`.
    pub test_lines: Vec<(u32, u32)>,
    /// The whole file is test code: its path follows a test convention, or a
    /// `#[cfg(test)]` declaration of a different file names it.
    pub test_file: bool,
    /// The facts of a Vue component, or `None` for a file of a language.
    pub component: Option<crate::parser::vue::Facts>,
}

impl FileSymbols {
    /// The byte is in test code. This is the one answer for each consumer
    /// that has a position and no symbol, as a sink has.
    pub fn is_test_at(&self, byte: usize) -> bool {
        self.test_file || test_code::is_in_ranges(&self.test_ranges, byte)
    }

    /// The line is in test code. Lines start at 1.
    pub fn is_test_at_line(&self, line: u32) -> bool {
        self.test_file || test_code::is_in_lines(&self.test_lines, line)
    }

    /// Tells if the symbols at `positions` are the definitions of one Rust
    /// item under different `cfg` conditions.
    ///
    /// The definitions have one name, one kind, one container path, and one
    /// `impl` block text. Each definition has a `cfg` attribute, and no two
    /// have the same attributes. Rust does not permit two definitions of one
    /// name in one scope, thus each build has a maximum of one of them.
    pub fn are_condition_twins(&self, positions: &[usize]) -> bool {
        let [first, _, ..] = positions else {
            return false;
        };
        let (lead, lead_facts) = (&self.symbols[*first], &self.link_facts[*first]);
        let mut seen = BTreeSet::new();
        positions.iter().all(|position| {
            let (symbol, facts) = (&self.symbols[*position], &self.link_facts[*position]);
            let same_item = symbol.name == lead.name
                && symbol.kind == lead.kind
                && facts.container_path == lead_facts.container_path;
            match (&facts.condition, &lead_facts.condition) {
                (Some(condition), Some(lead_condition)) => {
                    same_item
                        && condition.implementation == lead_condition.implementation
                        && seen.insert(condition.attributes.as_slice())
                }
                _ => false,
            }
        })
    }

    /// The bytes are the bytes that gave these facts.
    pub fn is_content_of_facts(&self, content: &[u8]) -> bool {
        *blake3::hash(content).as_bytes() == self.content_hash
    }
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

/// The names that a file binds again with a macro, an alias, an assignment to
/// the global object, or a singleton class. These bindings are not symbols.
/// Tells if a pattern of a parameter or a declaration binds `name`.
fn pattern_binds(pattern: Node<'_>, name: &str, source: &[u8]) -> bool {
    // A stack in place of recursion, because a pattern can nest to any depth.
    let mut pending = vec![pattern];
    while let Some(pattern) = pending.pop() {
        match pattern.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                if source[pattern.byte_range()] == *name.as_bytes() {
                    return true;
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                pending.extend(pattern.child_by_field_name("left"));
            }
            "pair_pattern" => pending.extend(pattern.child_by_field_name("value")),
            "required_parameter" | "optional_parameter" => {
                pending.extend(pattern.child_by_field_name("pattern"));
            }
            "object_pattern" | "array_pattern" | "rest_pattern" | "formal_parameters" => {
                let mut cursor = pattern.walk();
                let children: Vec<Node<'_>> = pattern.named_children(&mut cursor).collect();
                pending.extend(children.into_iter().rev());
            }
            _ => {}
        }
    }
    false
}

/// The names of the global objects of ECMAScript.
const GLOBAL_OBJECTS: [&str; 4] = ["globalThis", "window", "global", "self"];

/// The node kinds whose body is the scope of a `var`, besides the program.
/// The parameters of such a node are outside this scope.
const VARIABLE_SCOPES: [&str; 7] = [
    "function_declaration",
    "function_expression",
    "generator_function",
    "generator_function_declaration",
    "arrow_function",
    "method_definition",
    "class_static_block",
];

/// The node kinds that the global object name check reads.
pub fn rebound_scope_kinds(language: Language) -> Vec<&'static str> {
    match language {
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            let mut kinds = VARIABLE_SCOPES.to_vec();
            kinds.extend([
                "program",
                "statement_block",
                "for_in_statement",
                "for_statement",
                "lexical_declaration",
                "variable_declaration",
                "import_statement",
                "import_clause",
                "namespace_import",
                "import_specifier",
                "export_statement",
                "assignment_expression",
                "member_expression",
            ]);
            if matches!(language, Language::TypeScript | Language::Tsx) {
                kinds.extend(["import_alias", "import_require_clause"]);
            }
            kinds
        }
        _ => Vec::new(),
    }
}

/// Gives each local name that an `import` statement binds: the default
/// import, the namespace, and each named import under its alias.
fn import_bindings(statement: Node<'_>, mark: &mut dyn FnMut(Node<'_>)) {
    let mut stack = vec![statement];
    while let Some(current) = stack.pop() {
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            match (current.kind(), child.kind()) {
                ("import_clause" | "namespace_import" | "import_require_clause", "identifier") => {
                    mark(child);
                }
                ("import_specifier", _) => {
                    let local = current
                        .child_by_field_name("alias")
                        .or_else(|| current.child_by_field_name("name"));
                    if let Some(local) = local {
                        mark(local);
                    }
                    break;
                }
                _ => stack.push(child),
            }
        }
    }
}

/// Gives the name that a TypeScript `import name = Entity;` binds.
fn import_alias_binding(alias: Node<'_>, mark: &mut dyn FnMut(Node<'_>)) {
    if let Some(name) = alias
        .named_child(0)
        .filter(|name| name.kind() == "identifier")
    {
        mark(name);
    }
}

/// The global object names that a scope node binds, one bit for each name of
/// `GLOBAL_OBJECTS`: its parameters, the `let` and `const` variables of its
/// block, the variable of a loop, the parameter of a `catch` clause, and, for
/// a function or the program, each `var` that its body declares. A nested
/// function has its own `var` names.
fn scope_bindings(scope: Node<'_>, parent: Option<Node<'_>>, source: &[u8]) -> u8 {
    let mut mask = 0u8;
    let mut mark = |pattern: Node<'_>| {
        for (bit, name) in GLOBAL_OBJECTS.iter().enumerate() {
            if pattern_binds(pattern, name, source) {
                mask |= 1 << bit;
            }
        }
    };
    let declarators = |declaration: Node<'_>, mark: &mut dyn FnMut(Node<'_>)| {
        let mut cursor = declaration.walk();
        for declarator in declaration.named_children(&mut cursor) {
            if let Some(binding) = declarator.child_by_field_name("name") {
                mark(binding);
            }
        }
    };
    let parameters = scope
        .child_by_field_name("parameters")
        .or_else(|| scope.child_by_field_name("parameter"));
    if let Some(parameters) = parameters {
        mark(parameters);
    }
    match scope.kind() {
        "statement_block" | "program" => {
            let mut cursor = scope.walk();
            for statement in scope.named_children(&mut cursor) {
                if statement.kind() == "lexical_declaration" {
                    declarators(statement, &mut mark);
                } else if statement.kind() == "import_statement" {
                    import_bindings(statement, &mut mark);
                } else if statement.kind() == "import_alias" {
                    import_alias_binding(statement, &mut mark);
                } else if statement.kind() == "export_statement" {
                    let mut inner = statement.walk();
                    for exported in statement.named_children(&mut inner) {
                        if exported.kind() == "import_alias" {
                            import_alias_binding(exported, &mut mark);
                        }
                    }
                }
            }
        }
        "for_in_statement" => {
            let declares_variable = scope
                .child_by_field_name("kind")
                .is_some_and(|kind| source[kind.byte_range()] != *b"var");
            if declares_variable {
                if let Some(left) = scope.child_by_field_name("left") {
                    mark(left);
                }
            }
        }
        "for_statement" => {
            let initializer = scope
                .child_by_field_name("initializer")
                .filter(|initializer| initializer.kind() == "lexical_declaration");
            if let Some(initializer) = initializer {
                declarators(initializer, &mut mark);
            }
        }
        _ => {}
    }
    let is_body = scope.kind() == "statement_block"
        && parent.is_some_and(|parent| VARIABLE_SCOPES.contains(&parent.kind()));
    if is_body || scope.kind() == "program" {
        let mut stack = vec![scope];
        while let Some(current) = stack.pop() {
            let mut cursor = current.walk();
            for child in current.named_children(&mut cursor) {
                if VARIABLE_SCOPES.contains(&child.kind()) {
                    continue;
                }
                match child.kind() {
                    "variable_declaration" => declarators(child, &mut mark),
                    "for_in_statement" => {
                        let is_var = child
                            .child_by_field_name("kind")
                            .is_some_and(|kind| source[kind.byte_range()] == *b"var");
                        if let (true, Some(left)) = (is_var, child.child_by_field_name("left")) {
                            mark(left);
                        }
                    }
                    _ => {}
                }
                stack.push(child);
            }
        }
    }
    mask
}

/// The name that stands in the rebound names for a Rust `macro_rules!` whose
/// body has a `use` or an `extern crate`. The macro can bind any crate name.
pub const RUST_MACRO_IMPORT: &str = "<macro import>";

/// Tells if the tokens of a macro, outside string and character literals,
/// have `use` or `extern crate`.
fn has_import_tokens(node: Node<'_>, source: &[u8]) -> bool {
    let mut tokens: Vec<(usize, &[u8])> = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind().contains("literal") || current.kind() == "string_content" {
            continue;
        }
        if current.child_count() == 0 {
            tokens.push((current.start_byte(), &source[current.byte_range()]));
            continue;
        }
        let mut cursor = current.walk();
        stack.extend(current.children(&mut cursor));
    }
    tokens.sort_unstable_by_key(|(start, _)| *start);
    tokens
        .windows(2)
        .any(|pair| pair[0].1 == b"use" || (pair[0].1 == b"extern" && pair[1].1 == b"crate"))
        || tokens.last().is_some_and(|last| last.1 == b"use")
}

fn rebound_names(parsed: &ParseResult) -> BTreeSet<String> {
    // The walk is in source order. `path` holds the ancestors of the node, and
    // `bound[level]` the global object names that `path[..=level]` bind. The
    // tree-sitter parent of a node costs the depth of the node.
    let mut path: Vec<Node<'_>> = Vec::new();
    let mut bound: Vec<u8> = Vec::new();
    let text =
        |node: Node<'_>| String::from_utf8_lossy(&parsed.source[node.byte_range()]).to_string();
    let mut names = BTreeSet::new();
    let mut stack = vec![(parsed.root_node(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        path.truncate(depth);
        bound.truncate(depth);
        let mut cursor = node.walk();
        stack.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, depth + 1)),
        );
        match (parsed.language, node.kind()) {
            (Language::C | Language::Cpp, "preproc_def" | "preproc_function_def") => {
                names.extend(node.child_by_field_name("name").map(text));
            }
            (Language::Rust, "macro_definition" | "macro_invocation") => {
                if has_import_tokens(node, &parsed.source) {
                    names.insert(RUST_MACRO_IMPORT.to_string());
                }
            }
            (Language::Bash, "command") => {
                let command = text(node);
                if let Some(rest) = command.strip_prefix("alias ") {
                    for word in rest.split_whitespace() {
                        if let Some((name, _)) = word.split_once('=') {
                            names.insert(name.trim_matches(['\'', '"']).to_string());
                        }
                    }
                }
            }
            (
                Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx,
                "assignment_expression",
            ) => {
                let target = node
                    .child_by_field_name("left")
                    .filter(|left| left.kind() == "member_expression");
                let object = target.and_then(|target| target.child_by_field_name("object"));
                let property = target.and_then(|target| target.child_by_field_name("property"));
                if let (Some(object), Some(property)) = (object, property) {
                    let object_name = text(object);
                    let global = GLOBAL_OBJECTS
                        .iter()
                        .position(|name| *name == object_name.as_str());
                    if let Some(bit) = global {
                        while bound.len() < path.len() {
                            let scope = path[bound.len()];
                            let inherited = bound.last().copied().unwrap_or(0);
                            let parent = bound.len().checked_sub(1).map(|index| path[index]);
                            bound.push(inherited | scope_bindings(scope, parent, &parsed.source));
                        }
                        let hidden = bound.last().is_some_and(|mask| mask & (1 << bit) != 0);
                        if !hidden {
                            names.insert(text(property));
                        }
                    }
                }
            }
            (Language::Ruby, "singleton_class") => {
                names.extend(
                    node.child_by_field_name("value")
                        .filter(|value| value.kind() == "constant")
                        .map(text),
                );
            }
            _ => {}
        }
        path.push(node);
    }
    names
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
        comment_before: HashMap::new(),
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
        generated_kinds: spec.generated_kinds(),
        module_kinds: spec.module_kinds(),
        imports: Vec::new(),
        modules: Vec::new(),
        scope_owners: Vec::new(),
        closure_depths: Vec::new(),
        decorators: Vec::new(),
        pending_decorators: Vec::new(),
        call_kinds: spec.call_kinds(),
        local_kinds: spec.local_kinds(),
        facts_of_kinds: Vec::new(),
        ancestors: Vec::new(),
        return_owner_kinds: spec.return_owner_kinds(),
        return_owners: Vec::new(),
        return_type_field: spec.call_syntax().return_type_field,
        calls: Vec::new(),
        value_kinds: spec.value_kinds(),
        values: Vec::new(),
        locals: Vec::new(),
        block_kinds: spec.block_kinds(),
        blocks: Vec::new(),
        export_aliases: Vec::new(),
        language: parsed.language,
        members: Recorder::new(parsed.language),
        memo: Memo::default(),
        test_mark_kinds: test_code::mark_kinds(parsed.language),
        test_owners: Vec::new(),
        test_callee_roots: test_code::CalleeRoots::default(),
        test_marks: Vec::new(),
        in_test_file: test_code::is_test_path(file, parsed.language),
    };
    extractor.walk(parsed.root_node());
    let test_file = extractor.in_test_file;
    let mark_table = MarkTable::new(std::mem::take(&mut extractor.test_marks));

    let imports = extractor.imports;
    let modules = extractor.modules;
    let export_aliases = extractor.export_aliases;
    let mut found = extractor.found;
    let comment_before = extractor.comment_before;
    found.sort_by_key(|(draft, _)| (draft.span.start_byte, draft.span.end_byte));
    // A generated method owns nothing, and its node can name several methods.
    let positions: HashMap<usize, usize> = found
        .iter()
        .enumerate()
        .filter(|(_, (draft, _))| draft.generated.is_none())
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
            self_wrapper: name
                .value_start
                .filter(|_| parsed.language == Language::Rust)
                .and_then(|start| lang::rust::self_wrapper(&parsed.source, start)),
            name,
            owner: position_of(owner),
            block,
        })
        .collect();
    let member_facts = match extractor.members.take() {
        Some(recorder) => {
            let rows: Vec<SymbolRow<'_>> = found
                .iter()
                .map(|(draft, _)| SymbolRow {
                    node: draft.node.id(),
                    kind: draft.kind,
                    name: &draft.name,
                    container_path: &draft.container_path,
                    start_byte: draft.span.start_byte,
                    end_byte: draft.span.end_byte,
                    body: draft.body.map(|body| (body.start_byte(), body.end_byte())),
                    has_receiver: draft.receiver_type.is_some(),
                })
                .collect();
            recorder.finish(&parsed.source, &rows, &locals, &positions)
        }
        None => MemberFacts::default(),
    };
    let parents: Vec<Option<usize>> = found
        .iter()
        .map(|(draft, _)| position_of(draft.owner))
        .collect();
    // The generated methods and then the fields get their ordinals after the
    // other symbols. Thus they do not change the key of a method with the
    // same name that a definition with a body gives.
    // The count is for each rendered key. Two different paths can render one
    // key, for example a Bash function `a.b` and a function `b` in `a`.
    // A name that ends in `~2` can also render the key of a second definition.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut ordinals = vec![0; found.len()];
    let pass =
        |draft: &Draft<'_>| match (draft.generated.is_some(), draft.kind == SymbolKind::Field) {
            (false, false) => 0,
            (true, _) => 1,
            (false, true) => 2,
        };
    for current in 0..3 {
        for (position, (draft, _)) in found.iter().enumerate() {
            if pass(draft) == current {
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
        let (mut test_role, test_entry_in_test_code) =
            mark_table.role(draft.node.byte_range(), test_file);
        let is_callable = matches!(
            draft.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        );
        if test_file
            && is_callable
            && test_role == TestRole::Support
            && test_code::is_test_by_name(parsed.language, &draft.name)
        {
            test_role = TestRole::Entry;
        }
        symbols.push(Symbol {
            key: symbol_key(file, &draft.container_path, &draft.name, ordinal).into(),
            signature: signature(&draft, &parsed.source),
            doc: doc(draft.node, &comment_before, &parsed.source),
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
            test_role,
        });
        link_facts.push(LinkFacts {
            container_path: draft.container_path,
            parameter_count: draft.parameter_count,
            parameter_types: draft.parameter_types,
            parent,
            return_type: draft.return_type,
            type_parameters: draft.type_parameters,
            trait_implementation: draft.trait_implementation,
            default_export: draft.default_export,
            function_value: draft.function_value,
            c_linkage: draft.c_linkage,
            defined_type: draft.defined_type,
            prepends: draft.prepends,
            bases: draft.bases,
            named_by_binding: draft.named_by_binding,
            test_entry_in_test_code,
            condition: draft.condition,
            self_wrapper: draft.self_wrapper,
            generated: draft.generated,
            export_aliases,
            singleton: draft.singleton,
        });
    }
    let mut calls = extractor.calls;
    let shared_file = SharedStr::from(file);
    calls.sort_by_key(|(_, range, _, _, _)| range.start);
    let mut references = Vec::with_capacity(calls.len());
    let mut reference_facts = Vec::with_capacity(calls.len());
    for (call, range, owner, via_closure, receiver) in calls {
        let (start_byte, end_byte) = (range.start, range.end);
        let owner = position_of(owner);
        let nullable_target = call
            .callee
            .target
            .as_ref()
            .is_some_and(|target| target.nullable);
        let text = String::from_utf8_lossy(&parsed.source[call.callee.name_node.byte_range()]);
        let name = match call.kind {
            ReferenceKind::Construct => call
                .callee
                .target
                .as_ref()
                .map_or_else(|| split_type(&text).1, |target| target.name.clone()),
            ReferenceKind::Call | ReferenceKind::FunctionReference => text.to_string(),
        };
        references.push(Reference {
            file: shared_file.clone(),
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
            nullable_target,
            variable: false,
        });
    }
    // Most values are local names and fields, and a repository has many of
    // them. A use that the facts of this file show to name no function gets
    // no record. The rules agree with the rules of the resolver, which gives
    // a function reference no candidate that has only the name as evidence.
    let mut bound: HashMap<(usize, &str), Vec<BoundName<'_>>> = HashMap::new();
    for local in &locals {
        if let Some(owner) = local.owner {
            let names = bound.entry((owner, local.name.name.as_str())).or_default();
            names.push(BoundName {
                first_value: local.name.value_start.unwrap_or(0),
                block: local.block.as_ref(),
                has_type_or_value: local.name.type_name.is_some()
                    || local.name.value_start.is_some(),
            });
        }
    }
    let defined: std::collections::HashSet<&str> = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|symbol| symbol.name.as_str())
        .collect();
    let imported: std::collections::HashSet<&str> = imports
        .iter()
        .filter_map(|binding| binding.local_name.as_deref())
        .collect();
    let sees_other_files = imports.iter().any(|binding| {
        matches!(
            binding.kind,
            ImportKind::Load | ImportKind::Wildcard | ImportKind::ReExportWildcard
        )
    });
    // In these languages a bare name is a definition of the file or an
    // import, unless the file loads a file or has a wildcard import.
    let names_are_in_file = !sees_other_files
        && matches!(
            parsed.language,
            Language::Python
                | Language::JavaScript
                | Language::Jsx
                | Language::TypeScript
                | Language::Tsx
                | Language::Rust
                | Language::Bash
        );
    // The owner and each definition that contains it, the nearest first.
    let chain_of = |owner: Option<usize>| {
        std::iter::successors(owner, |position| link_facts[*position].parent)
    };
    let mut values = extractor.values;
    values.sort_by_key(|(used, _, _)| used.node.start_byte());
    let mut value_sites = ValueSites::default();
    for (used, owner, via_closure) in values {
        let bare = used.callee.receiver.is_none() && used.callee.qualifier.is_none();
        let name = match &used.name {
            Some(name) => name.as_str(),
            None => std::str::from_utf8(&parsed.source[used.callee.name_node.byte_range()])
                .unwrap_or(""),
        };
        let start = used.node.start_byte();
        let owner_position = position_of(owner);
        // A PHP variable has `$` before the name of the function value that it holds.
        let is_defined = defined.contains(name.trim_start_matches('$'));
        let names_no_function = match &used.callee.receiver {
            // A local name hides a function of a different file. The name
            // is not in scope before its first value, and a name of a block
            // is not in scope after the block.
            None if bare && !is_defined => {
                let hidden = chain_of(owner_position)
                    .filter_map(|position| bound.get(&(position, name)))
                    .flatten()
                    .any(|local| {
                        (local.first_value < start || local.first_value == 0)
                            && local.block.is_none_or(|block| block.contains(&start))
                    });
                hidden || (names_are_in_file && !imported.contains(name))
            }
            // A parameter without a type gives no type to its member.
            Some(Receiver::Name(receiver)) if !is_defined => {
                let mut locals = chain_of(owner_position)
                    .filter_map(|position| bound.get(&(position, receiver.as_str())))
                    .flatten()
                    .peekable();
                locals.peek().is_some()
                    && locals.all(|local| !local.has_type_or_value)
                    && !defined.contains(receiver.as_str())
                    && !imported.contains(receiver.as_str())
            }
            _ => false,
        };
        if names_no_function || name.is_empty() {
            continue;
        }
        let name = value_sites.add_text(name);
        let receiver = match &used.callee.receiver {
            None => SiteReceiver::None,
            Some(Receiver::SelfValue) => SiteReceiver::SelfValue,
            Some(Receiver::Super) => SiteReceiver::Super,
            Some(Receiver::Name(text)) => SiteReceiver::Name(value_sites.add_text(text)),
            Some(Receiver::Expression) => SiteReceiver::Expression,
        };
        let qualifier = used.callee.qualifier.as_deref();
        let qualifier = qualifier.map(|text| value_sites.add_text(text));
        let receiver_text = used
            .receiver
            .and_then(|(start, end)| parsed.source.get(start..end))
            .and_then(|text| std::str::from_utf8(text).ok())
            .map(|text| value_sites.add_text(text));
        value_sites.sites.push(ValueSite {
            name,
            name_range: Span::of(&used.callee.name_node),
            start_byte: start as u32,
            end_byte: used.node.end_byte() as u32,
            owner: owner_position.map(|owner| owner as u32),
            receiver,
            qualifier,
            receiver_range: used.receiver.map(|(start, end)| (start as u32, end as u32)),
            receiver_text,
            arrow: used.receiver.is_some_and(|(_, end)| {
                let rest = parsed.source.get(end..);
                rest.is_some_and(|rest| rest.trim_ascii_start().starts_with(b"->"))
            }),
            via_closure,
            variable: used.variable,
        });
    }
    value_sites.sites.shrink_to_fit();
    value_sites.text.shrink_to_fit();
    FileSymbols {
        file: file.to_string(),
        language: parsed.language,
        symbols,
        link_facts,
        imports,
        modules,
        references,
        reference_facts,
        function_references: Vec::new(),
        value_sites,
        locals,
        path_targets: BTreeMap::new(),
        rebound: rebound_names(parsed),
        members: member_facts,
        content_hash: parsed.component.as_ref().map_or_else(
            || *blake3::hash(&parsed.source).as_bytes(),
            |component| component.content_hash,
        ),
        test_ranges: mark_table.ranges,
        test_lines: mark_table.lines,
        test_file,
        component: parsed.component.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::testing::rows;
    use crate::core::Language;

    /// A comment after a chain of names does not change the value.
    #[test]
    fn test_value_path_before_a_comment() {
        let path = |source: &str| super::value_path(source.as_bytes(), 0);
        let expected = Some(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(path("a.b // note\n"), expected);
        assert_eq!(path("a.b /* note */\n"), expected);
        assert_eq!(path("a.b # note\n"), expected);
        assert_eq!(path("a.b / 2\n"), None);
        assert_eq!(path("a.b\n"), expected);
    }

    fn rebound_in(source: &str) -> Vec<String> {
        let parsed = crate::parser::Parser::new()
            .parse(
                source.as_bytes(),
                Language::JavaScript,
                std::path::Path::new("a.js"),
            )
            .unwrap();
        super::rebound_names(&parsed).into_iter().collect()
    }

    fn rebound_in_typescript(source: &str) -> Vec<String> {
        let parsed = crate::parser::Parser::new()
            .parse(
                source.as_bytes(),
                Language::TypeScript,
                std::path::Path::new("a.ts"),
            )
            .unwrap();
        super::rebound_names(&parsed).into_iter().collect()
    }

    #[test]
    fn test_typescript_import_alias_is_a_local_binding_of_a_global_object_name() {
        for source in [
            "import window = Objects.window; window.fetch = r;",
            "import self = require('objects'); self.fetch = r;",
            "export import global = Objects.Inner.global; global.fetch = r;",
        ] {
            assert!(rebound_in_typescript(source).is_empty(), "{source}");
        }
        let unrelated = "import other = window.objects; window.fetch = r;";
        assert_eq!(rebound_in_typescript(unrelated), ["fetch"]);
    }

    fn global_assignments_in_a_large_block_are_linear_at(size: usize) -> std::time::Duration {
        let mut source = String::from("function install() {\n  const local = 1;\n");
        for line in 0..size {
            source.push_str(&format!("  window.x{line} = 1;\n"));
        }
        source.push_str("}\n");
        let started = std::time::Instant::now();
        let rebound = rebound_in(&source);
        assert_eq!(rebound.len(), size);
        let elapsed = started.elapsed();
        let shadowed = source.replace("const local = 1;", "const window = {};");
        assert!(rebound_in(&shadowed).is_empty());
        elapsed
    }

    #[test]
    fn test_global_assignments_in_a_large_block_are_linear() {
        global_assignments_in_a_large_block_are_linear_at(2_000);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_global_assignments_in_a_large_block_are_linear() {
        let elapsed = global_assignments_in_a_large_block_are_linear_at(100_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn global_assignments_in_deeply_nested_blocks_are_linear_at(
        size: usize,
    ) -> std::time::Duration {
        let depth = size;
        let mut source = String::from("function install() {\n");
        for level in 0..depth {
            source.push_str(&format!("{{ window.x{level} = 1;\n"));
        }
        source.push_str(&"}".repeat(depth));
        source.push_str("\n}\n");
        let started = std::time::Instant::now();
        let rebound = rebound_in(&source);
        assert_eq!(rebound.len(), depth);
        started.elapsed()
    }

    #[test]
    fn test_global_assignments_in_deeply_nested_blocks_are_linear() {
        global_assignments_in_deeply_nested_blocks_are_linear_at(2_000);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_global_assignments_in_deeply_nested_blocks_are_linear() {
        let elapsed = global_assignments_in_deeply_nested_blocks_are_linear_at(50_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    /// The time of `extract_file` on `size` nested blocks of a Java method, each with a local
    /// class after a comment, without the parse.
    fn deeply_nested_java_classes_at(size: usize) -> std::time::Duration {
        let source = format!(
            "class A {{\nvoid f() {{\n{}{}}}\n}}\n",
            (0..size)
                .map(|level| format!("{{\n// L{level}\nclass L{level} {{}}\n"))
                .collect::<String>(),
            "}\n".repeat(size)
        );
        let parsed = crate::parser::Parser::new()
            .parse(
                source.as_bytes(),
                Language::Java,
                std::path::Path::new("A.java"),
            )
            .unwrap();
        let started = crate::index::timing::Stopwatch::start();
        let symbols = super::extract_file(&parsed, "A.java");
        let elapsed = started.elapsed();
        let local = symbols
            .symbols
            .iter()
            .find(|symbol| symbol.name == "L1")
            .expect("L1");
        assert_eq!(local.doc.as_deref(), Some("// L1"));
        elapsed
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_classes_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly(
            "local classes in nested blocks",
            5_000,
            deeply_nested_java_classes_at,
        );
    }

    /// The time of `extract_file` on `source`, without the parse.
    fn extract_time(language: Language, file: &'static str, source: String) -> std::time::Duration {
        let parsed = crate::parser::Parser::new()
            .parse(source.as_bytes(), language, std::path::Path::new(file))
            .unwrap();
        let started = crate::index::timing::Stopwatch::start();
        super::extract_file(&parsed, file);
        started.elapsed()
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_nested_arrow_functions_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("arrow functions", 5_000, |size| {
            let source = format!("const f = {}1;\n", "() => ".repeat(size));
            extract_time(Language::TypeScript, "a.ts", source)
        });
    }

    /// A Ruby class whose `class << self` has `size` nested blocks, each with a
    /// method.
    fn ruby_methods_in_nested_blocks(size: usize) -> String {
        format!(
            "class A\nclass << self\n{}{}end\nend\n",
            (0..size)
                .map(|level| format!("run do\ndef m{level}\nend\n"))
                .collect::<String>(),
            "end\n".repeat(size)
        )
    }

    #[test]
    fn test_ruby_methods_in_nested_blocks_of_the_singleton_class_are_singleton() {
        let source = ruby_methods_in_nested_blocks(3);
        let parsed = crate::parser::Parser::new()
            .parse(
                source.as_bytes(),
                Language::Ruby,
                std::path::Path::new("a.rb"),
            )
            .unwrap();
        let symbols = super::extract_file(&parsed, "a.rb");
        let methods: Vec<(&str, bool)> = symbols
            .symbols
            .iter()
            .zip(&symbols.link_facts)
            .filter(|(symbol, _)| symbol.name.starts_with('m'))
            .map(|(symbol, facts)| (symbol.name.as_str(), facts.singleton))
            .collect();
        assert_eq!(methods, [("m0", true), ("m1", true), ("m2", true)]);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_ruby_methods_in_nested_blocks_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("ruby methods", 5_000, |size| {
            extract_time(Language::Ruby, "a.rb", ruby_methods_in_nested_blocks(size))
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_go_closures_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("go closures", 5_000, |size| {
            let source = format!(
                "package p\nfunc f() {{\n{}{}\n}}\n",
                "func() {".repeat(size),
                "}()".repeat(size)
            );
            extract_time(Language::Go, "a.go", source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_python_lambdas_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("python lambdas", 5_000, |size| {
            let source = format!("def f():\n    return {}1\n", "lambda: ".repeat(size));
            extract_time(Language::Python, "a.py", source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_php_arrow_functions_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("php arrow functions", 5_000, |size| {
            let source = format!("<?php\n$f = {}1;\n", "fn() => ".repeat(size));
            extract_time(Language::Php, "a.php", source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deep_c_pointer_declarators_are_extracted_in_linear_time() {
        crate::index::timing::assert_scales_linearly("c pointer declarators", 5_000, |size| {
            let source = format!(
                "int {}x;\nint {}f(void) {{ return 0; }}\n",
                "*".repeat(size),
                "*".repeat(size)
            );
            extract_time(Language::C, "a.c", source)
        });
    }

    #[test]
    fn test_var_loop_binding_belongs_to_the_function() {
        for source in [
            "function f(xs) { for (var window of xs) { } window.fetch = r; }",
            "function f(xs) { for (var window in xs) { } window.fetch = r; }",
            "function f() { for (var window = {}; ; ) { break; } window.fetch = r; }",
            "function f() { if (true) { var window = {}; } window.fetch = r; }",
        ] {
            assert!(rebound_in(source).is_empty(), "{source}");
        }
        let block = "function f(xs) { for (let window of xs) { } window.fetch = r; }";
        assert_eq!(rebound_in(block), ["fetch"]);
        let nested = "function f() { function g() { var window = {}; } window.fetch = r; }";
        assert_eq!(rebound_in(nested), ["fetch"]);
    }

    #[test]
    fn test_top_level_declarations_static_blocks_and_default_parameters() {
        for source in [
            "const window = {}; window.fetch = r;",
            "let self; self.fetch = r;",
            "function f() { var window; window.fetch = r; }",
            "function* f() { var window; window.fetch = r; }",
            "const f = () => { var window; window.fetch = r; };",
            "class C { m() { var window; window.fetch = r; } }",
            "class C { static { var window = {}; window.fetch = r; } }",
        ] {
            assert!(rebound_in(source).is_empty(), "{source}");
        }
        for source in [
            "class C { static { var window = {}; } } window.fetch = r;",
            "function f(x = (window.fetch = r)) { var window; }",
            "const f = (x = (window.fetch = r)) => { var window; };",
            "class C { m(x = (window.fetch = r)) { var window; } }",
        ] {
            assert_eq!(rebound_in(source), ["fetch"], "{source}");
        }
    }

    #[test]
    fn test_import_bindings_are_local_bindings_of_a_global_object_name() {
        for source in [
            "import window from './objects.js'; window.fetch = r;",
            "import { window } from './objects.js'; window.fetch = r;",
            "import { objects as window } from './objects.js'; window.fetch = r;",
            "import * as window from './objects.js'; window.fetch = r;",
            "import self, { other } from './objects.js'; self.fetch = r;",
            "import other, * as globalThis from './objects.js'; globalThis.fetch = r;",
        ] {
            assert!(rebound_in(source).is_empty(), "{source}");
        }
        let unrelated = "import { window as other } from './objects.js'; window.fetch = r;";
        assert_eq!(rebound_in(unrelated), ["fetch"]);
    }

    #[test]
    fn test_loop_and_catch_bindings_are_local_bindings_of_a_global_object_name() {
        for source in [
            "function f(objects) { for (const window of objects) { window.fetch = r; } }",
            "function f(objects) { for (let window in objects) { window.fetch = r; } }",
            "function f() { for (let window = {}; ; ) { window.fetch = r; } }",
            "function f() { try { g(); } catch (window) { window.fetch = r; } }",
        ] {
            assert!(rebound_in(source).is_empty(), "{source}");
        }
        let assigned = "function f(objects) { for (window of objects) { } window.fetch = r; }";
        assert_eq!(rebound_in(assigned), ["fetch"]);
    }

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
            .map(|symbol| symbol.key.to_string())
            .collect()
    }
}
