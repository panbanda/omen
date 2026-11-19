//! TypeScript and JavaScript routes. The section "TypeScript and JavaScript routes" in
//! `docs/SECURITY.md` lists each form that omen reads and each form that it skips.
//!
//! - Registrations. `receiver.verb(path, ...middleware, handler)` registers a route when the
//!   receiver is a framework value: an Express application or router, a Fastify instance, or a
//!   Koa router. A framework value is a constructor call of the framework, a name that one
//!   declaration binds to a framework value, a parameter or a field with a framework type, or
//!   `this.name` for a field of the class or of a superclass.
//! - Decorators. A class decorator gives a prefix, and a method decorator gives a verb and a path.
//!   The decorators of NestJS, routing-controllers, and tsoa come from an import of the package. A
//!   decorator of the repository is a verb when its definition is a call `factory("verb")`.
//! - Paths. A string, a template, a `+` of strings, and a `const` name that the scope or an import
//!   resolves give the path. A part that omen cannot resolve makes the path `None`. The template
//!   keeps that part as `${expression}`.
//! - Mounts. `parent.use(prefix, router)` in the file of the router adds the prefix to the routes
//!   of the router.
//! - Inline handlers. A function value without a name gets a key that the security analysis
//!   gives to a node of its own in the call graph.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use tree_sitter::Node;

use super::{
    join_path, route, route_parameters, syntax, Context, Draft, EntryBasis, EntryKind,
    GuardsUnknownReason, InlineHandler, Route, HTTP_VERBS,
};
use crate::index::extract::FileSymbols;
use crate::index::model::{ImportBinding, ImportKind, ImportTarget, Span, SymbolKind};

/// The longest chain of names, imports, mounts, and superclasses that omen follows.
const MAX_HOPS: usize = 16;

/// The most prefixes that the mounts of one router give. A router with more prefixes has one
/// prefix that omen does not know, because the number of chains can grow exponentially with
/// the layers of mounts.
const MAX_MOUNT_CHAINS: usize = 64;

/// The framework name of a route that a decorator of the repository declares.
const REPOSITORY_DECORATORS: &str = "repository_decorators";

const FUNCTIONS: [&str; 7] = [
    "function_declaration",
    "function_expression",
    "function",
    "arrow_function",
    "method_definition",
    "generator_function",
    "generator_function_declaration",
];

/// The functions that give `this` a new value. An arrow function keeps the `this` of its scope.
const THIS_BINDERS: [&str; 5] = [
    "function_declaration",
    "function_expression",
    "function",
    "generator_function",
    "generator_function_declaration",
];

const BLOCKS: [&str; 7] = [
    "program",
    "statement_block",
    "class_static_block",
    "for_statement",
    "for_in_statement",
    "catch_clause",
    "switch_body",
];

const CLASSES: [&str; 3] = ["class_declaration", "class", "abstract_class_declaration"];

/// The expressions that give the value of the expression in them.
const WRAPPERS: [&str; 5] = [
    "parenthesized_expression",
    "as_expression",
    "satisfies_expression",
    "non_null_expression",
    "type_assertion",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Framework {
    Express,
    Fastify,
    Koa,
}

impl Framework {
    fn name(self) -> &'static str {
        match self {
            Self::Express => "express",
            Self::Fastify => "fastify",
            Self::Koa => "koa",
        }
    }
}

/// A package whose decorators declare routes.
struct DecoratorPackage {
    module: &'static str,
    framework: &'static str,
    /// The class decorators that give a prefix.
    prefixes: &'static [&'static str],
    /// The method decorators that give a route, with the method.
    verbs: &'static [(&'static str, &'static str)],
    /// The decorators whose arguments are guards, as `UseGuards(AuthGuard)`.
    guard_lists: &'static [&'static str],
    /// The decorators that are a guard themselves, as `Authorized()`.
    guard_markers: &'static [&'static str],
}

const PACKAGES: [DecoratorPackage; 3] = [
    DecoratorPackage {
        module: "@nestjs/common",
        framework: "nestjs",
        prefixes: &["Controller"],
        verbs: &[
            ("Get", "GET"),
            ("Post", "POST"),
            ("Put", "PUT"),
            ("Patch", "PATCH"),
            ("Delete", "DELETE"),
            ("Head", "HEAD"),
            ("Options", "OPTIONS"),
            ("All", "ANY"),
        ],
        guard_lists: &["UseGuards"],
        guard_markers: &[],
    },
    DecoratorPackage {
        module: "routing-controllers",
        framework: "routing-controllers",
        prefixes: &["Controller", "JsonController"],
        verbs: &[
            ("Get", "GET"),
            ("Post", "POST"),
            ("Put", "PUT"),
            ("Patch", "PATCH"),
            ("Delete", "DELETE"),
            ("Head", "HEAD"),
            ("All", "ANY"),
        ],
        guard_lists: &["UseBefore"],
        guard_markers: &["Authorized"],
    },
    DecoratorPackage {
        module: "tsoa",
        framework: "tsoa",
        prefixes: &["Route"],
        verbs: &[
            ("Get", "GET"),
            ("Post", "POST"),
            ("Put", "PUT"),
            ("Patch", "PATCH"),
            ("Delete", "DELETE"),
            ("Head", "HEAD"),
            ("Options", "OPTIONS"),
        ],
        guard_lists: &["Middlewares"],
        guard_markers: &["Security"],
    },
];

/// How one scope binds a name. A range is the range of a node in the file of the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Declared {
    /// `const name = value`, with the range of the value.
    Const(Range<usize>),
    /// `let` or `var`, with the range of the value that initializes it, if any.
    Variable(Option<Range<usize>>),
    /// A parameter, with the range of its type annotation.
    Parameter(Option<Range<usize>>),
    /// A class declaration, with the range of the class.
    Class(Range<usize>),
    /// A function declaration.
    Function,
    /// An import or a `require` call. The index has the binding.
    Import,
    /// Any other binding, or more than one binding in the scope.
    Other,
}

type Declarations = HashMap<String, Declared>;

/// The key of a binding: the id of the scope node and the name.
type BindingKey = (usize, String);

/// A part of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    /// The source text of an expression that omen cannot resolve.
    Unknown(String),
}

/// A `use` call that mounts a router of the file.
struct Mount {
    /// The receiver of the `use` call, when it is a name of the file.
    parent: Option<BindingKey>,
    prefix: Vec<Part>,
}

/// The positions of the import bindings of a file, by the local name.
type ImportsByName = HashMap<String, Vec<usize>>;

/// The facts of one file that the search of its routes reads more than one time.
#[derive(Default)]
pub(super) struct State {
    /// The bindings of each scope, by the position of the file and the id of the scope node.
    scopes: RefCell<HashMap<(usize, usize), Rc<Declarations>>>,
    /// The imports of each file, by the position of the file and then by the local name.
    imports: RefCell<HashMap<usize, Rc<ImportsByName>>>,
    mounts: OnceCell<HashMap<BindingKey, Vec<Mount>>>,
    mount_chains: OnceCell<HashMap<BindingKey, Vec<Vec<Part>>>>,
    /// The positions of the symbols of the file, by the end of their range.
    symbol_ends: OnceCell<HashMap<usize, Vec<usize>>>,
    /// The names that each file exports, by the position of the file.
    exports: RefCell<HashMap<usize, Rc<Exports>>>,
    /// The writes of each name of each file, by the position of the file.
    assignments: RefCell<HashMap<usize, Rc<WritesByName>>>,
}

/// The facts that the searches of all files share.
#[derive(Default)]
pub(super) struct Shared {
    positions: OnceLock<HashMap<String, usize>>,
    /// The framework of a field, by the position of the file, the start of the class, the name
    /// of the field, and the hops that the search had used. A search with fewer hops left can
    /// stop before a search with more hops, so the hops are part of the key.
    fields: parking_lot::Mutex<HashMap<FieldKey, Option<Framework>>>,
    /// The fields of each class body, by the position of the file and the start of the body.
    classes: parking_lot::Mutex<HashMap<(usize, usize), Arc<ClassFields>>>,
}

/// The fields of a class body, by the name of the field. A static field and an instance field
/// are apart.
#[derive(Default)]
struct ClassFields {
    instance: FieldTable,
    statics: FieldTable,
}

/// The declarations and the assignments of the fields of one kind, by the name of the field.
#[derive(Default)]
struct FieldTable {
    /// The ranges of the type annotations of the field definitions and of the parameter
    /// properties of the constructor.
    typed: HashMap<String, Vec<Range<usize>>>,
    /// The ranges of the initializers and of the values of each `this.name = value` in the class.
    values: HashMap<String, Vec<Range<usize>>>,
}

/// A field search: the position of the file, the start of the class, the name of the field,
/// whether the field is static, and the hops that the search had used.
type FieldKey = (usize, usize, String, bool, usize);

/// A file and its syntax tree.
#[derive(Clone, Copy)]
struct Unit<'t> {
    position: usize,
    source: &'t [u8],
    root: Node<'t>,
}

/// One decorator call: `@name(arguments)` or `@name`.
struct DecoratorCall<'t> {
    node: Node<'t>,
    callee: Node<'t>,
    arguments: Vec<Node<'t>>,
}

/// Where the definition of a decorator is.
enum Origin {
    Package(&'static DecoratorPackage, String),
    /// A definition of the repository. `module` is the import path, or empty for a definition of
    /// the same file.
    Repository {
        module: String,
        position: usize,
        declared: Declared,
    },
}

impl Origin {
    fn module(&self) -> &str {
        match self {
            Self::Package(package, _) => package.module,
            Self::Repository { module, .. } => module,
        }
    }
}

fn text<'t>(node: Node<'_>, source: &'t [u8]) -> &'t str {
    syntax::text(node, source)
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// The expression in the wrappers that do not change its value.
fn unwrap(node: Node<'_>) -> Node<'_> {
    let mut node = node;
    while WRAPPERS.contains(&node.kind()) {
        match node.named_child(0) {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// Tells if a member of a class body belongs to the class and not to its instances.
fn is_static(member: Node<'_>) -> bool {
    member.kind() == "class_static_block"
        || children(member)
            .iter()
            .any(|child| child.kind() == "static")
}

/// The static name and the value of each property of an object literal. A shorthand property
/// `{ name }` is its own value. A computed key has no static name and is not given.
fn object_properties<'t>(object: Node<'t>, source: &[u8]) -> Vec<(String, Node<'t>)> {
    let mut properties = Vec::new();
    for member in named_children(object) {
        match member.kind() {
            "pair" => {
                let (Some(key), Some(value)) = (
                    member.child_by_field_name("key"),
                    member.child_by_field_name("value"),
                ) else {
                    continue;
                };
                let name = match key.kind() {
                    "property_identifier" | "number" => Some(text(key, source).to_string()),
                    "string" => string_value(key, source),
                    _ => None,
                };
                if let Some(name) = name {
                    properties.push((name, value));
                }
            }
            "shorthand_property_identifier" => {
                properties.push((text(member, source).to_string(), member));
            }
            _ => {}
        }
    }
    properties
}

fn is_plus(node: Node<'_>) -> bool {
    node.kind() == "binary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|operator| operator.kind() == "+")
}

/// The node with the range. Of two nodes with the same range, the outer one.
fn node_at<'t>(root: Node<'t>, range: &Range<usize>) -> Option<Node<'t>> {
    let mut node = root.descendant_for_byte_range(range.start, range.end)?;
    while let Some(parent) = node.parent() {
        if parent.byte_range() != *range {
            break;
        }
        node = parent;
    }
    (node.byte_range() == *range).then_some(node)
}

/// The value of one escape sequence of a string, as ECMAScript gives it. A legacy octal escape
/// gives `None`.
fn escape_value(escape: &str) -> Option<String> {
    let body = escape.strip_prefix('\\')?;
    let mut characters = body.chars();
    let first = characters.next()?;
    let rest: &str = characters.as_str();
    let hex = |digits: &str| {
        u32::from_str_radix(digits, 16)
            .ok()
            .and_then(char::from_u32)
    };
    let value = match first {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'b' => '\u{8}',
        'f' => '\u{c}',
        'v' => '\u{b}',
        '0' if rest.is_empty() => '\0',
        '1'..='9' => return None,
        'x' => hex(rest)?,
        'u' => hex(rest.trim_start_matches('{').trim_end_matches('}'))?,
        '\r' | '\n' | '\u{2028}' | '\u{2029}' => return Some(String::new()),
        other => other,
    };
    Some(value.to_string())
}

/// The value of a string literal, or `None` when it has a part that omen does not evaluate.
fn string_value(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut value = String::new();
    for child in named_children(node) {
        match child.kind() {
            "string_fragment" => value.push_str(text(child, source)),
            "escape_sequence" => value.push_str(&escape_value(text(child, source))?),
            _ => return None,
        }
    }
    Some(value)
}

/// The text of the parts, when each part is a text.
fn resolved(parts: &[Part]) -> Option<String> {
    parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            Part::Unknown(_) => None,
        })
        .collect()
}

/// The outer prefix and then the inner prefix, with one `/` between them.
fn joined(mut outer: Vec<Part>, inner: &[Part]) -> Vec<Part> {
    let outer_slash = matches!(outer.last(), Some(Part::Text(text)) if text.ends_with('/'));
    let inner_slash = matches!(inner.first(), Some(Part::Text(text)) if text.starts_with('/'));
    match (
        outer.is_empty() || inner.is_empty(),
        outer_slash,
        inner_slash,
    ) {
        (true, _, _) => {}
        (false, true, true) => {
            if let Some(Part::Text(text)) = outer.last_mut() {
                text.pop();
            }
        }
        (false, false, false) => outer.push(Part::Text("/".to_string())),
        _ => {}
    }
    outer.extend(inner.iter().cloned());
    outer
}

/// The parts with each unknown part as `${expression}`, and with each unknown part as a mark that
/// no path parameter contains.
fn rendered(parts: &[Part]) -> (String, String) {
    let mut template = String::new();
    let mut marked = String::new();
    for part in parts {
        match part {
            Part::Text(text) => {
                template.push_str(text);
                marked.push_str(text);
            }
            Part::Unknown(expression) => {
                template.push_str(&format!("${{{expression}}}"));
                marked.push('\u{1}');
            }
        }
    }
    (template, marked)
}

/// The route of the parts. With a prefix, the prefix and the path join as `join_path` joins
/// them. Without a prefix, the path is as the source writes it.
fn route_of(method: &str, prefix: Option<&[Part]>, path: &[Part]) -> Route {
    let (template, marked) = rendered(path);
    let (template, marked) = match prefix {
        Some(prefix) => {
            let (prefix_template, prefix_marked) = rendered(prefix);
            (
                join_path(&prefix_template, &template),
                join_path(&prefix_marked, &marked),
            )
        }
        None => (template, marked),
    };
    let known = resolved(path).is_some() && prefix.is_none_or(|prefix| resolved(prefix).is_some());
    if known {
        return route(method, template);
    }
    Route {
        method: method.to_uppercase(),
        parameters: route_parameters(&marked),
        path: None,
        path_template: Some(template),
    }
}

/// Adds the identifiers that a pattern binds.
fn pattern_names<'t>(pattern: Node<'t>, out: &mut Vec<Node<'t>>) {
    // A loop with a stack, because a generated pattern can be deeper than the call stack.
    let mut stack = vec![pattern];
    while let Some(pattern) = stack.pop() {
        match pattern.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => out.push(pattern),
            "pair_pattern" => stack.extend(pattern.child_by_field_name("value")),
            "assignment_pattern" | "object_assignment_pattern" => {
                stack.extend(pattern.child_by_field_name("left"))
            }
            _ => stack.extend(named_children(pattern).into_iter().rev()),
        }
    }
}

fn is_require(value: Node<'_>, source: &[u8]) -> bool {
    value.kind() == "call_expression"
        && value
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "identifier" && text(function, source) == "require"
            })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Const,
    Let,
    Var,
}

/// Collects the bindings of one scope.
struct Collector<'s> {
    source: &'s [u8],
    found: HashMap<String, Vec<Declared>>,
    /// The local name and the exported name of `export { local as exported }`.
    aliases: Vec<(String, String)>,
}

impl<'s> Collector<'s> {
    fn add(&mut self, name: Node<'_>, declared: Declared) {
        self.found
            .entry(text(name, self.source).to_string())
            .or_default()
            .push(declared);
    }

    fn declarator(&mut self, declarator: Node<'_>, mode: Mode) {
        let Some(name) = declarator.child_by_field_name("name") else {
            return;
        };
        let value = declarator.child_by_field_name("value");
        let required = value.is_some_and(|value| is_require(value, self.source));
        if name.kind() == "identifier" {
            let declared = match (required, mode, value) {
                (true, _, _) => Declared::Import,
                (false, Mode::Const, Some(value)) => Declared::Const(value.byte_range()),
                (false, Mode::Const, None) => Declared::Other,
                (false, _, value) => Declared::Variable(value.map(|value| value.byte_range())),
            };
            self.add(name, declared);
            return;
        }
        let mut names = Vec::new();
        pattern_names(name, &mut names);
        for name in names {
            let declared = if required {
                Declared::Import
            } else {
                Declared::Other
            };
            self.add(name, declared);
        }
    }

    fn declaration(&mut self, node: Node<'_>, mode: Mode) {
        for child in named_children(node) {
            if child.kind() == "variable_declarator" {
                self.declarator(child, mode);
            }
        }
    }

    fn lexical(&mut self, node: Node<'_>) {
        let mode = match node.child(0).map(|keyword| keyword.kind()) {
            Some("const") => Mode::Const,
            _ => Mode::Let,
        };
        self.declaration(node, mode);
    }

    /// A statement of a block that binds a name in the block.
    fn statement(&mut self, node: Node<'_>) {
        match node.kind() {
            "lexical_declaration" => self.lexical(node),
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(name, Declared::Function);
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(name, Declared::Class(node.byte_range()));
                }
            }
            "enum_declaration" | "internal_module" | "module" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(name, Declared::Other);
                }
            }
            "import_statement" => self.import(node),
            "export_statement" => {
                if let Some(declaration) = node.child_by_field_name("declaration") {
                    self.statement(declaration);
                } else if node.child_by_field_name("source").is_some() {
                    for clause in named_children(node) {
                        match clause.kind() {
                            "export_clause" => {
                                for specifier in named_children(clause) {
                                    let name = specifier
                                        .child_by_field_name("alias")
                                        .or_else(|| specifier.child_by_field_name("name"));
                                    if let Some(name) = name {
                                        self.add(name, Declared::Import);
                                    }
                                }
                            }
                            "namespace_export" => {
                                if let Some(name) = clause.named_child(0) {
                                    self.add(name, Declared::Import);
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    for clause in named_children(node) {
                        if clause.kind() != "export_clause" {
                            continue;
                        }
                        for specifier in named_children(clause) {
                            let name = specifier.child_by_field_name("name");
                            let alias = specifier.child_by_field_name("alias");
                            if let (Some(name), Some(alias)) = (name, alias) {
                                self.aliases.push((
                                    text(name, self.source).to_string(),
                                    text(alias, self.source).to_string(),
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn import(&mut self, node: Node<'_>) {
        for clause in named_children(node) {
            match clause.kind() {
                "import_clause" => {
                    for part in named_children(clause) {
                        match part.kind() {
                            "identifier" => self.add(part, Declared::Import),
                            "namespace_import" => {
                                if let Some(name) = part.named_child(0) {
                                    self.add(name, Declared::Import);
                                }
                            }
                            "named_imports" => {
                                for specifier in named_children(part) {
                                    let name = specifier
                                        .child_by_field_name("alias")
                                        .or_else(|| specifier.child_by_field_name("name"));
                                    if let Some(name) = name {
                                        self.add(name, Declared::Import);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "import_require_clause" => {
                    if let Some(name) = clause.named_child(0) {
                        self.add(name, Declared::Import);
                    }
                }
                _ => {}
            }
        }
    }

    /// The `var` declarations in the body of a function or of the program. They do not enter a
    /// nested function or class.
    fn hoisted(&mut self, body: Node<'_>) {
        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            if node.kind() == "variable_declaration" {
                self.declaration(node, Mode::Var);
            }
            for child in named_children(node) {
                if !FUNCTIONS.contains(&child.kind())
                    && !CLASSES.contains(&child.kind())
                    && child.kind() != "class_body"
                {
                    stack.push(child);
                }
            }
        }
    }

    fn parameters(&mut self, parameters: Node<'_>) {
        for parameter in named_children(parameters) {
            match parameter.kind() {
                "required_parameter" | "optional_parameter" => {
                    let Some(pattern) = parameter.child_by_field_name("pattern") else {
                        continue;
                    };
                    if pattern.kind() == "identifier" {
                        let annotation = parameter
                            .child_by_field_name("type")
                            .map(|annotation| annotation.byte_range());
                        self.add(pattern, Declared::Parameter(annotation));
                    } else {
                        let mut names = Vec::new();
                        pattern_names(pattern, &mut names);
                        for name in names {
                            self.add(name, Declared::Other);
                        }
                    }
                }
                "identifier" => self.add(parameter, Declared::Parameter(None)),
                _ => {
                    let mut names = Vec::new();
                    pattern_names(parameter, &mut names);
                    for name in names {
                        self.add(name, Declared::Other);
                    }
                }
            }
        }
    }

    fn finish(self) -> Declarations {
        let mut declarations: Declarations = self
            .found
            .into_iter()
            .map(|(name, mut found)| {
                let declared = if found.len() == 1 {
                    found.remove(0)
                } else {
                    Declared::Other
                };
                (name, declared)
            })
            .collect();
        for (local, exported) in self.aliases {
            if let Some(declared) = declarations.get(&local).cloned() {
                declarations.entry(exported).or_insert(declared);
            }
        }
        declarations
    }
}

/// An assignment to a name: the range of the name, and the range of the value when the write
/// gives a plain value.
type WritesByName = HashMap<String, Vec<Write>>;

struct Write {
    target: Range<usize>,
    value: Option<Range<usize>>,
}

/// The names that the `export` statements of a file export.
#[derive(Default)]
struct Exports {
    /// The file has an `export` statement. A file without one can export with `module.exports`,
    /// which omen does not read, so each binding at the top of such a file can be an export.
    modular: bool,
    names: std::collections::HashSet<String>,
}

fn exports_of(unit: Unit<'_>) -> Exports {
    let mut exports = Exports::default();
    for statement in named_children(unit.root) {
        if statement.kind() != "export_statement" {
            continue;
        }
        exports.modular = true;
        if children(statement)
            .iter()
            .any(|child| child.kind() == "default")
        {
            exports.names.insert("default".to_string());
        }
        if let Some(declaration) = statement.child_by_field_name("declaration") {
            let mut names = Vec::new();
            match declaration.kind() {
                "lexical_declaration" | "variable_declaration" => {
                    for declarator in named_children(declaration) {
                        if let Some(name) = declarator.child_by_field_name("name") {
                            pattern_names(name, &mut names);
                        }
                    }
                }
                _ => names.extend(declaration.child_by_field_name("name")),
            }
            for name in names {
                exports.names.insert(text(name, unit.source).to_string());
            }
        }
        for clause in named_children(statement) {
            match clause.kind() {
                "export_clause" => {
                    for specifier in named_children(clause) {
                        let name = specifier
                            .child_by_field_name("alias")
                            .or_else(|| specifier.child_by_field_name("name"));
                        if let Some(name) = name {
                            exports.names.insert(text(name, unit.source).to_string());
                        }
                    }
                }
                "namespace_export" => {
                    if let Some(name) = clause.named_child(0) {
                        exports.names.insert(text(name, unit.source).to_string());
                    }
                }
                _ => {}
            }
        }
    }
    exports
}

/// The bindings of one scope node.
fn scope_bindings(node: Node<'_>, source: &[u8]) -> Declarations {
    let mut collector = Collector {
        source,
        found: HashMap::new(),
        aliases: Vec::new(),
    };
    let kind = node.kind();
    if FUNCTIONS.contains(&kind) {
        if let Some(parameters) = node.child_by_field_name("parameters") {
            collector.parameters(parameters);
        }
        if let Some(parameter) = node.child_by_field_name("parameter") {
            collector.add(parameter, Declared::Parameter(None));
        }
        if matches!(
            kind,
            "function_expression" | "function" | "generator_function"
        ) {
            if let Some(name) = node.child_by_field_name("name") {
                collector.add(name, Declared::Function);
            }
        }
        if let Some(body) = node.child_by_field_name("body") {
            collector.hoisted(body);
        }
        return collector.finish();
    }
    if kind == "program" {
        collector.hoisted(node);
    }
    match kind {
        "for_statement" => {
            if let Some(initializer) = node.child_by_field_name("initializer") {
                collector.statement(initializer);
            }
        }
        "for_in_statement" => {
            if let Some(left) = node.child_by_field_name("left") {
                let mut names = Vec::new();
                pattern_names(left, &mut names);
                for name in names {
                    collector.add(name, Declared::Other);
                }
            }
        }
        "catch_clause" => {
            if let Some(parameter) = node.child_by_field_name("parameter") {
                let mut names = Vec::new();
                pattern_names(parameter, &mut names);
                for name in names {
                    collector.add(name, Declared::Other);
                }
            }
        }
        _ => {}
    }
    let statements = if kind == "switch_body" {
        named_children(node)
            .into_iter()
            .flat_map(named_children)
            .collect()
    } else {
        named_children(node)
    };
    for statement in statements {
        collector.statement(statement);
    }
    collector.finish()
}

/// The identifier of a decorator and its arguments.
fn decorator_call<'t>(decorator: Node<'t>) -> Option<DecoratorCall<'t>> {
    let expression = decorator.named_child(0)?;
    let (callee, arguments) = match expression.kind() {
        "call_expression" => (
            expression.child_by_field_name("function")?,
            Context::arguments_of(expression),
        ),
        "identifier" => (expression, Vec::new()),
        _ => return None,
    };
    (callee.kind() == "identifier").then_some(DecoratorCall {
        node: decorator,
        callee,
        arguments,
    })
}

/// The text of a node with each run of white space as one space.
fn compact(node: Node<'_>, source: &[u8]) -> String {
    text(node, source)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The decorator as the source writes it, without `@`.
fn decorator_label(call: &DecoratorCall<'_>, source: &[u8]) -> String {
    call.node
        .named_child(0)
        .map(|expression| compact(expression, source))
        .unwrap_or_default()
}

/// The method that a verb name gives.
fn method_of(verb: &str) -> String {
    if verb.eq_ignore_ascii_case("all") {
        "ANY".to_string()
    } else {
        verb.to_uppercase()
    }
}

impl<'a> Context<'a> {
    fn unit(&self) -> Unit<'a> {
        Unit {
            position: self.position,
            source: self.source,
            root: self.parsed.root_node(),
        }
    }

    /// Runs `read` on the file at `position`.
    fn with_unit<R>(&self, position: usize, read: impl FnOnce(Unit<'_>) -> Option<R>) -> Option<R> {
        if position == self.position {
            return read(self.unit());
        }
        let tree = self.all_parsed.tree(position)?;
        read(Unit {
            position,
            source: &tree.source,
            root: tree.root_node(),
        })
    }

    fn position_of(&self, file: &str) -> Option<usize> {
        self.all_parsed
            .shared()
            .ecma
            .positions
            .get_or_init(|| {
                self.all
                    .iter()
                    .enumerate()
                    .map(|(position, file)| (file.file.clone(), position))
                    .collect()
            })
            .get(file)
            .copied()
    }

    fn target_position(&self, binding: &ImportBinding) -> Option<usize> {
        match &binding.target_file {
            ImportTarget::Resolved(file) => self.position_of(file),
            ImportTarget::Package(files) if files.len() == 1 => self.position_of(&files[0]),
            _ => None,
        }
    }

    fn scope_declarations(&self, unit: Unit<'_>, scope: Node<'_>) -> Rc<Declarations> {
        let key = (unit.position, scope.id());
        if let Some(found) = self.ecma.scopes.borrow().get(&key) {
            return Rc::clone(found);
        }
        let found = Rc::new(scope_bindings(scope, unit.source));
        self.ecma.scopes.borrow_mut().insert(key, Rc::clone(&found));
        found
    }

    /// The scope and the binding of the name at its use, as the scopes of ECMAScript give it.
    fn declared(&self, unit: Unit<'_>, at: Node<'_>, name: &str) -> Option<(usize, Declared)> {
        self.declared_scope(unit, at, name)
            .map(|(scope, declared)| (scope.id(), declared))
    }

    fn declared_scope<'t>(
        &self,
        unit: Unit<'_>,
        at: Node<'t>,
        name: &str,
    ) -> Option<(Node<'t>, Declared)> {
        let mut current = at.parent();
        while let Some(node) = current {
            let kind = node.kind();
            if BLOCKS.contains(&kind) || FUNCTIONS.contains(&kind) {
                if let Some(found) = self.scope_declarations(unit, node).get(name) {
                    return Some((node, found.clone()));
                }
            }
            current = node.parent();
        }
        None
    }

    /// The value of a `let` or `var` binding: its initializer when no assignment in the scope
    /// writes the name, or the one assignment when it has no initializer. Any other case has
    /// more than one value, and omen does not choose one.
    fn variable_value(
        &self,
        unit: Unit<'_>,
        scope: Node<'_>,
        name: &str,
        initial: Option<Range<usize>>,
    ) -> Option<Range<usize>> {
        let table = self.assignments(unit);
        let writes = table.get(name).map(Vec::as_slice).unwrap_or_default();
        let first = writes.partition_point(|write| write.target.start < scope.start_byte());
        let last = writes.partition_point(|write| write.target.start < scope.end_byte());
        match (&writes[first..last], initial) {
            ([], initial) => initial,
            ([write], None) => {
                let target = node_at(unit.root, &write.target)?;
                let (found, _) = self.declared_scope(unit, target, name)?;
                (found.id() == scope.id()).then(|| write.value.clone())?
            }
            _ => None,
        }
    }

    /// Each write of a name in the file, sorted by the start of the name.
    fn assignments(&self, unit: Unit<'_>) -> Rc<WritesByName> {
        if let Some(found) = self.ecma.assignments.borrow().get(&unit.position) {
            return Rc::clone(found);
        }
        let mut table: WritesByName = HashMap::new();
        let mut stack = vec![unit.root];
        while let Some(node) = stack.pop() {
            if matches!(
                node.kind(),
                "assignment_expression" | "augmented_assignment_expression"
            ) {
                let left = node.child_by_field_name("left").map(unwrap);
                let right = node.child_by_field_name("right");
                // A write through a pattern or through an operator such as `+=` has no plain
                // value, but it still is a write.
                let plain = node.kind() == "assignment_expression"
                    && left.is_some_and(|left| left.kind() == "identifier");
                let mut names = Vec::new();
                if let Some(left) = left.filter(|left| {
                    matches!(
                        left.kind(),
                        "identifier" | "object_pattern" | "array_pattern"
                    )
                }) {
                    pattern_names(left, &mut names);
                }
                for target in names {
                    table
                        .entry(text(target, unit.source).to_string())
                        .or_default()
                        .push(Write {
                            target: target.byte_range(),
                            value: right.filter(|_| plain).map(|right| right.byte_range()),
                        });
                }
            }
            if node.kind() == "update_expression" {
                if let Some(argument) = node.child_by_field_name("argument") {
                    if argument.kind() == "identifier" {
                        table
                            .entry(text(argument, unit.source).to_string())
                            .or_default()
                            .push(Write {
                                target: argument.byte_range(),
                                value: None,
                            });
                    }
                }
            }
            stack.extend(named_children(node));
        }
        for writes in table.values_mut() {
            writes.sort_by_key(|write| write.target.start);
        }
        let table = Rc::new(table);
        self.ecma
            .assignments
            .borrow_mut()
            .insert(unit.position, Rc::clone(&table));
        table
    }

    /// The names that the file at `position` exports with `export` statements.
    fn export_names(&self, position: usize) -> Option<Rc<Exports>> {
        if let Some(found) = self.ecma.exports.borrow().get(&position) {
            return Some(Rc::clone(found));
        }
        let found = self.with_unit(position, |unit| Some(Rc::new(exports_of(unit))))?;
        self.ecma
            .exports
            .borrow_mut()
            .insert(position, Rc::clone(&found));
        Some(found)
    }

    /// The one import binding of the file with the local name.
    fn import_named(&self, position: usize, name: &str) -> Option<&'a ImportBinding> {
        let file: &'a FileSymbols = &self.all[position];
        let table = {
            let mut imports = self.ecma.imports.borrow_mut();
            Rc::clone(imports.entry(position).or_insert_with(|| {
                let mut table: ImportsByName = HashMap::new();
                for (index, binding) in file.imports.iter().enumerate() {
                    if let Some(local) = &binding.local_name {
                        table.entry(local.clone()).or_default().push(index);
                    }
                }
                Rc::new(table)
            }))
        };
        let indexes = table.get(name)?;
        let first = &file.imports[*indexes.first()?];
        indexes
            .iter()
            .map(|index| &file.imports[*index])
            .all(|binding| {
                binding.module_path == first.module_path
                    && binding.imported_name == first.imported_name
            })
            .then_some(first)
    }

    /// The declaration of a name that the file at `position` exports, through the re-exports.
    fn exported(&self, position: usize, name: &str, hops: usize) -> Option<(usize, Declared)> {
        if hops > MAX_HOPS {
            return None;
        }
        let top = self.with_unit(position, |unit| {
            Some(self.scope_declarations(unit, unit.root))
        })?;
        let exports = self.export_names(position)?;
        let local = top
            .get(name)
            .filter(|_| !exports.modular || exports.names.contains(name));
        match local {
            Some(Declared::Import) => {
                let binding = self.import_named(position, name)?;
                let imported = binding
                    .imported_name
                    .as_deref()
                    .filter(|imported| !matches!(*imported, "*" | "default"))?;
                let target = self.target_position(binding)?;
                self.exported(target, imported, hops + 1)
            }
            Some(found) => Some((position, found.clone())),
            None => {
                let targets: Vec<usize> = self.all[position]
                    .imports
                    .iter()
                    .filter(|binding| binding.kind == ImportKind::ReExportWildcard)
                    .filter_map(|binding| self.target_position(binding))
                    .collect();
                // Two `export *` that give one declaration give one export, as ECMAScript says.
                let mut found: Vec<(usize, Declared)> = targets
                    .into_iter()
                    .filter_map(|target| self.exported(target, name, hops + 1))
                    .collect();
                found.dedup();
                let first = found.first()?;
                found
                    .iter()
                    .all(|other| other == first)
                    .then(|| first.clone())
            }
        }
    }

    /// The parts of the path that the expression gives.
    fn path_of(&self, unit: Unit<'_>, node: Node<'_>, hops: usize) -> Vec<Part> {
        let node = unwrap(node);
        let unknown = || vec![Part::Unknown(text(node, unit.source).to_string())];
        // Each nested operand and each name uses a hop, so the hops bound the depth of the
        // recursion.
        if hops > MAX_HOPS {
            return unknown();
        }
        let mut parts = match node.kind() {
            "string" => match string_value(node, unit.source) {
                Some(value) => vec![Part::Text(value)],
                None => unknown(),
            },
            "template_string" => {
                let mut parts = Vec::new();
                for child in named_children(node) {
                    match child.kind() {
                        "string_fragment" => {
                            parts.push(Part::Text(text(child, unit.source).to_string()))
                        }
                        "escape_sequence" => match escape_value(text(child, unit.source)) {
                            Some(value) => parts.push(Part::Text(value)),
                            None => return unknown(),
                        },
                        "template_substitution" => {
                            let Some(inner) = child.named_child(0) else {
                                return unknown();
                            };
                            let inner_parts = self.path_of(unit, inner, hops + 1);
                            match resolved(&inner_parts) {
                                Some(value) => parts.push(Part::Text(value)),
                                None => {
                                    parts.push(Part::Unknown(text(inner, unit.source).to_string()))
                                }
                            }
                        }
                        _ => return unknown(),
                    }
                }
                parts
            }
            "binary_expression" if is_plus(node) => {
                // `a + b + c` nests on the left, so a long chain is a deep tree. The left spine
                // is read with a loop, and only a nested right operand uses a hop.
                let mut rights = Vec::new();
                let mut left = node;
                while is_plus(left) {
                    let (Some(next), Some(right)) = (
                        left.child_by_field_name("left"),
                        left.child_by_field_name("right"),
                    ) else {
                        return unknown();
                    };
                    rights.push(right);
                    left = unwrap(next);
                }
                let mut parts = self.path_of(unit, left, hops + 1);
                for right in rights.into_iter().rev() {
                    parts.extend(self.path_of(unit, right, hops + 1));
                }
                parts
            }
            "identifier" => match self.constant_text(unit, node, hops) {
                Some(value) => vec![Part::Text(value)],
                None => unknown(),
            },
            _ => unknown(),
        };
        let mut merged: Vec<Part> = Vec::new();
        for part in parts.drain(..) {
            match (merged.last_mut(), part) {
                (Some(Part::Text(previous)), Part::Text(next)) => previous.push_str(&next),
                (_, part) => merged.push(part),
            }
        }
        merged
    }

    /// The text of a `const` name, in its scope or through an import.
    fn constant_text(&self, unit: Unit<'_>, name: Node<'_>, hops: usize) -> Option<String> {
        if hops >= MAX_HOPS {
            return None;
        }
        let (position, declared) = match self.declared(unit, name, text(name, unit.source))? {
            (_, Declared::Import) => {
                let binding = self.import_named(unit.position, text(name, unit.source))?;
                let imported = binding
                    .imported_name
                    .as_deref()
                    .filter(|imported| !matches!(*imported, "*" | "default"))?;
                let target = self.target_position(binding)?;
                self.exported(target, imported, hops + 1)?
            }
            (_, declared) => (unit.position, declared),
        };
        let Declared::Const(range) = declared else {
            return None;
        };
        self.with_unit(position, |unit| {
            let value = node_at(unit.root, &range)?;
            resolved(&self.path_of(unit, value, hops + 1))
        })
    }

    /// The framework of the value of the expression.
    fn framework_of(&self, unit: Unit<'_>, node: Node<'_>, hops: usize) -> Option<Framework> {
        if hops > MAX_HOPS {
            return None;
        }
        let node = unwrap(node);
        match node.kind() {
            "call_expression" => {
                self.constructed(unit, node.child_by_field_name("function")?, false)
            }
            "new_expression" => {
                self.constructed(unit, node.child_by_field_name("constructor")?, true)
            }
            "identifier" => {
                let name = text(node, unit.source);
                let (scope, declared) = self.declared_scope(unit, node, name)?;
                match declared {
                    Declared::Const(range) => {
                        let value = node_at(unit.root, &range)?;
                        self.framework_of(unit, value, hops + 1)
                    }
                    Declared::Variable(initial) => {
                        let range = self.variable_value(unit, scope, name, initial)?;
                        let value = node_at(unit.root, &range)?;
                        self.framework_of(unit, value, hops + 1)
                    }
                    Declared::Parameter(Some(range)) => {
                        self.framework_of_type(unit, node_at(unit.root, &range)?)
                    }
                    Declared::Import => {
                        let binding = self.import_named(unit.position, name)?;
                        let imported = binding.imported_name.as_deref()?;
                        let target = self.target_position(binding)?;
                        let (position, declared) = self.exported(target, imported, hops + 1)?;
                        self.with_unit(position, |unit| {
                            let range = match declared {
                                Declared::Const(range) => range,
                                Declared::Variable(initial) => {
                                    self.variable_value(unit, unit.root, imported, initial)?
                                }
                                _ => return None,
                            };
                            let value = node_at(unit.root, &range)?;
                            self.framework_of(unit, value, hops + 1)
                        })
                    }
                    _ => None,
                }
            }
            "member_expression" => {
                let object = node.child_by_field_name("object")?;
                if object.kind() != "this" {
                    return None;
                }
                let name = text(node.child_by_field_name("property")?, unit.source);
                let (class, is_static) = Self::class_of_this(node)?;
                self.class_field(unit, class, name, is_static, hops + 1)
            }
            _ => None,
        }
    }

    /// The framework that a call of the callee, or a `new` of it, gives.
    fn constructed(&self, unit: Unit<'_>, callee: Node<'_>, is_new: bool) -> Option<Framework> {
        let (name, member) = match callee.kind() {
            "identifier" => (callee, None),
            "member_expression" => {
                let object = callee.child_by_field_name("object")?;
                let property = callee.child_by_field_name("property")?;
                if object.kind() != "identifier" {
                    return None;
                }
                (object, Some(text(property, unit.source)))
            }
            _ => return None,
        };
        let local = text(name, unit.source);
        if self.declared(unit, name, local)?.1 != Declared::Import {
            return None;
        }
        let binding = self.import_named(unit.position, local)?;
        let itself = binding.kind == ImportKind::Module
            || matches!(binding.imported_name.as_deref(), Some("default" | "*"));
        let imported = binding.imported_name.as_deref();
        match (binding.module_path.as_str(), member, is_new) {
            ("express", None, false) if itself => Some(Framework::Express),
            // Express makes a router with or without `new`.
            ("express", None, _) if imported == Some("Router") => Some(Framework::Express),
            ("express", Some("Router"), _) if itself => Some(Framework::Express),
            ("fastify", None, false) if itself || imported == Some("fastify") => {
                Some(Framework::Fastify)
            }
            ("@koa/router" | "koa-router", None, true) if itself || imported == Some("Router") => {
                Some(Framework::Koa)
            }
            _ => None,
        }
    }

    /// The framework that a type annotation names.
    fn framework_of_type(&self, unit: Unit<'_>, annotation: Node<'_>) -> Option<Framework> {
        let mut node = if annotation.kind() == "type_annotation" {
            annotation.named_child(0)?
        } else {
            annotation
        };
        if node.kind() == "generic_type" {
            node = node.child_by_field_name("name")?;
        }
        let (local, name) = match node.kind() {
            "type_identifier" => (text(node, unit.source), None),
            "nested_type_identifier" => {
                let module = node.child_by_field_name("module")?;
                let name = node.child_by_field_name("name")?;
                if module.kind() != "identifier" {
                    return None;
                }
                (text(module, unit.source), Some(text(name, unit.source)))
            }
            _ => return None,
        };
        let binding = self.import_named(unit.position, local)?;
        let itself = binding.kind == ImportKind::Module
            || matches!(binding.imported_name.as_deref(), Some("default" | "*"));
        let type_name = match name {
            Some(name) if itself => name,
            Some(_) => return None,
            None => binding.imported_name.as_deref()?,
        };
        match (binding.module_path.as_str(), type_name) {
            ("express", "Application" | "Express" | "Router" | "IRouter") => {
                Some(Framework::Express)
            }
            ("fastify", "FastifyInstance") => Some(Framework::Fastify),
            ("@koa/router" | "koa-router", "default" | "Router") => Some(Framework::Koa),
            _ => None,
        }
    }

    /// The class whose instance `this` is at the node.
    fn class_of_this<'t>(node: Node<'t>) -> Option<(Node<'t>, bool)> {
        let mut member = node;
        let mut current = node.parent();
        while let Some(ancestor) = current {
            if THIS_BINDERS.contains(&ancestor.kind()) {
                return None;
            }
            if ancestor.kind() == "class_body" {
                return Some((ancestor.parent()?, is_static(member)));
            }
            member = ancestor;
            current = ancestor.parent();
        }
        None
    }

    /// The framework of a field of the class or of its nearest superclass that declares or
    /// assigns the field. A static field and an instance field with one name are two fields.
    fn class_field(
        &self,
        unit: Unit<'_>,
        class: Node<'_>,
        name: &str,
        is_static: bool,
        hops: usize,
    ) -> Option<Framework> {
        if hops > MAX_HOPS {
            return None;
        }
        let key = (
            unit.position,
            class.start_byte(),
            name.to_string(),
            is_static,
            hops,
        );
        if let Some(found) = self.all_parsed.shared().ecma.fields.lock().get(&key) {
            return *found;
        }
        let found = self.read_class_field(unit, class, name, is_static, hops);
        self.all_parsed
            .shared()
            .ecma
            .fields
            .lock()
            .insert(key, found);
        found
    }

    /// The fields of the class body, read in one pass and shared by each search of a field.
    fn class_fields(&self, unit: Unit<'_>, body: Node<'_>) -> Arc<ClassFields> {
        let key = (unit.position, body.start_byte());
        if let Some(found) = self.all_parsed.shared().ecma.classes.lock().get(&key) {
            return Arc::clone(found);
        }
        let mut fields = ClassFields::default();
        for member in named_children(body) {
            let table = if is_static(member) {
                &mut fields.statics
            } else {
                &mut fields.instance
            };
            match member.kind() {
                "public_field_definition" | "field_definition" => {
                    let Some(field_name) = member
                        .child_by_field_name("name")
                        .or_else(|| member.child_by_field_name("property"))
                    else {
                        continue;
                    };
                    let field_name = text(field_name, unit.source).to_string();
                    if let Some(annotation) = member.child_by_field_name("type") {
                        table
                            .typed
                            .entry(field_name.clone())
                            .or_default()
                            .push(annotation.byte_range());
                    }
                    if let Some(value) = member.child_by_field_name("value") {
                        table
                            .values
                            .entry(field_name)
                            .or_default()
                            .push(value.byte_range());
                    }
                }
                "method_definition" => {
                    let is_constructor = member
                        .child_by_field_name("name")
                        .is_some_and(|method| text(method, unit.source) == "constructor");
                    let parameters = member.child_by_field_name("parameters");
                    let (true, Some(parameters)) = (is_constructor, parameters) else {
                        continue;
                    };
                    for parameter in named_children(parameters) {
                        let declares = children(parameter).iter().any(|child| {
                            matches!(child.kind(), "accessibility_modifier" | "readonly")
                        });
                        let (true, Some(pattern), Some(annotation)) = (
                            declares,
                            parameter.child_by_field_name("pattern"),
                            parameter.child_by_field_name("type"),
                        ) else {
                            continue;
                        };
                        table
                            .typed
                            .entry(text(pattern, unit.source).to_string())
                            .or_default()
                            .push(annotation.byte_range());
                    }
                }
                _ => {}
            }
        }
        for member in named_children(body) {
            let table = if is_static(member) {
                &mut fields.statics
            } else {
                &mut fields.instance
            };
            let mut stack = vec![member];
            while let Some(node) = stack.pop() {
                if node.kind() == "assignment_expression" {
                    let left = node.child_by_field_name("left").map(unwrap);
                    let property = left
                        .filter(|left| {
                            left.kind() == "member_expression"
                                && left
                                    .child_by_field_name("object")
                                    .is_some_and(|object| object.kind() == "this")
                        })
                        .and_then(|left| left.child_by_field_name("property"));
                    if let (Some(property), Some(right)) =
                        (property, node.child_by_field_name("right"))
                    {
                        table
                            .values
                            .entry(text(property, unit.source).to_string())
                            .or_default()
                            .push(right.byte_range());
                    }
                }
                for child in named_children(node).into_iter().rev() {
                    if !THIS_BINDERS.contains(&child.kind()) && child.kind() != "class_body" {
                        stack.push(child);
                    }
                }
            }
        }
        let fields = Arc::new(fields);
        self.all_parsed
            .shared()
            .ecma
            .classes
            .lock()
            .insert(key, Arc::clone(&fields));
        fields
    }

    fn read_class_field(
        &self,
        unit: Unit<'_>,
        class: Node<'_>,
        name: &str,
        is_static: bool,
        hops: usize,
    ) -> Option<Framework> {
        let body = class.child_by_field_name("body")?;
        let fields = self.class_fields(unit, body);
        let table = if is_static {
            &fields.statics
        } else {
            &fields.instance
        };
        let typed: Vec<Option<Framework>> = table
            .typed
            .get(name)
            .into_iter()
            .flatten()
            .map(|range| {
                node_at(unit.root, range)
                    .and_then(|annotation| self.framework_of_type(unit, annotation))
            })
            .collect();
        let values: Vec<Option<Framework>> = table
            .values
            .get(name)
            .into_iter()
            .flatten()
            .map(|range| {
                node_at(unit.root, range).and_then(|value| self.framework_of(unit, value, hops + 1))
            })
            .collect();
        if let Some(found) = typed.iter().flatten().next() {
            return Some(*found);
        }
        if !values.is_empty() {
            let first = values[0]?;
            return values
                .iter()
                .all(|value| *value == Some(first))
                .then_some(first);
        }
        if !typed.is_empty() {
            return None;
        }
        self.superclass_field(unit, class, name, is_static, hops)
    }

    fn superclass_field(
        &self,
        unit: Unit<'_>,
        class: Node<'_>,
        name: &str,
        is_static: bool,
        hops: usize,
    ) -> Option<Framework> {
        let heritage = named_children(class)
            .into_iter()
            .find(|child| child.kind() == "class_heritage")?;
        let extends = named_children(heritage).into_iter().next()?;
        let superclass = if extends.kind() == "extends_clause" {
            extends.child_by_field_name("value")?
        } else {
            extends
        };
        if superclass.kind() != "identifier" {
            return None;
        }
        let local = text(superclass, unit.source);
        let (position, declared) = match self.declared(unit, superclass, local)?.1 {
            Declared::Import => {
                let binding = self.import_named(unit.position, local)?;
                let imported = binding
                    .imported_name
                    .as_deref()
                    .filter(|imported| !matches!(*imported, "*" | "default"))?;
                let target = self.target_position(binding)?;
                self.exported(target, imported, hops + 1)?
            }
            declared => (unit.position, declared),
        };
        let Declared::Class(range) = declared else {
            return None;
        };
        self.with_unit(position, |unit| {
            let parent = node_at(unit.root, &range)?;
            self.class_field(unit, parent, name, is_static, hops + 1)
        })
    }

    /// The key of the binding of a name of this file.
    fn binding_key(&self, unit: Unit<'_>, node: Node<'_>) -> Option<BindingKey> {
        let mut node = unwrap(node);
        // `const alias = router` names the same router, so the key is the key of `router`.
        for _ in 0..=MAX_HOPS {
            if node.kind() != "identifier" {
                return None;
            }
            let name = text(node, unit.source);
            let (scope, declared) = self.declared(unit, node, name)?;
            let alias = match declared {
                Declared::Const(range) => {
                    node_at(unit.root, &range).filter(|value| unwrap(*value).kind() == "identifier")
                }
                _ => None,
            };
            match alias {
                Some(value) => node = unwrap(value),
                None => return Some((scope, name.to_string())),
            }
        }
        None
    }

    /// The prefix of a `use` call, or `None` when the first argument is not a path. An expression
    /// that omen cannot tell from a middleware gives an unknown prefix.
    fn use_prefix(&self, unit: Unit<'_>, first: Node<'_>) -> Option<Vec<Part>> {
        let first = unwrap(first);
        let is_function = |value: Node<'_>| {
            matches!(
                unwrap(value).kind(),
                "arrow_function" | "function_expression" | "function" | "call_expression" | "array"
            )
        };
        match first.kind() {
            "string" | "template_string" | "binary_expression" => {
                Some(self.path_of(unit, first, 0))
            }
            "identifier" => {
                if self.framework_of(unit, first, 0).is_some() {
                    return None;
                }
                match self
                    .declared(unit, first, text(first, unit.source))
                    .map(|found| found.1)
                {
                    Some(Declared::Function | Declared::Class(_)) => None,
                    Some(Declared::Const(range) | Declared::Variable(Some(range)))
                        if node_at(unit.root, &range).is_some_and(is_function) =>
                    {
                        None
                    }
                    _ => Some(self.path_of(unit, first, 0)),
                }
            }
            "member_expression" => {
                if self.framework_of(unit, first, 0).is_some() {
                    return None;
                }
                Some(self.path_of(unit, first, 0))
            }
            _ => None,
        }
    }

    /// The `use` calls of the file that mount a router of the file, by the router.
    fn mounts(&self) -> &HashMap<BindingKey, Vec<Mount>> {
        self.ecma.mounts.get_or_init(|| {
            let unit = self.unit();
            let mut mounts: HashMap<BindingKey, Vec<Mount>> = HashMap::new();
            let mut stack = vec![unit.root];
            let mut calls = Vec::new();
            while let Some(node) = stack.pop() {
                if node.kind() == "call_expression" {
                    calls.push(node);
                }
                stack.extend(named_children(node).into_iter().rev());
            }
            for call in calls {
                let Some(callee) = call.child_by_field_name("function") else {
                    continue;
                };
                let property = callee.child_by_field_name("property");
                if callee.kind() != "member_expression"
                    || property.is_none_or(|property| text(property, unit.source) != "use")
                {
                    continue;
                }
                let Some(receiver) = callee.child_by_field_name("object") else {
                    continue;
                };
                let arguments = Self::arguments_of(call);
                let Some(first) = arguments.first() else {
                    continue;
                };
                if self.framework_of(unit, receiver, 0).is_none() {
                    continue;
                }
                let (prefix, routers) = match self.use_prefix(unit, *first) {
                    Some(prefix) => (prefix, &arguments[1..]),
                    None => (Vec::new(), &arguments[..]),
                };
                let parent = self.binding_key(unit, receiver);
                for router in routers {
                    if self.framework_of(unit, *router, 0).is_none() {
                        continue;
                    }
                    if let Some(key) = self.binding_key(unit, *router) {
                        mounts.entry(key).or_default().push(Mount {
                            parent: parent.clone(),
                            prefix: prefix.clone(),
                        });
                    }
                }
            }
            mounts
        })
    }

    /// The prefixes of the routes of a router, one for each chain of mounts. `None` when no
    /// `use` call of the file mounts the router.
    fn mount_prefixes(&self, unit: Unit<'_>, receiver: Node<'_>) -> Option<Vec<Vec<Part>>> {
        let key = self.binding_key(unit, receiver)?;
        self.mount_chains().get(&key).cloned()
    }

    /// The prefixes of each router that a `use` call of the file mounts. The search starts at
    /// the routers that no `use` call mounts and goes down the mounts, so it reads each mount
    /// one time.
    fn mount_chains(&self) -> &HashMap<BindingKey, Vec<Vec<Part>>> {
        self.ecma.mount_chains.get_or_init(|| {
            let mounts = self.mounts();
            let mut graph: DiGraph<BindingKey, ()> = DiGraph::new();
            let mut nodes: HashMap<BindingKey, NodeIndex> = HashMap::new();
            let mut node_of = |graph: &mut DiGraph<BindingKey, ()>, key: &BindingKey| {
                *nodes
                    .entry(key.clone())
                    .or_insert_with(|| graph.add_node(key.clone()))
            };
            let mut keys: Vec<&BindingKey> = mounts.keys().collect();
            keys.sort();
            for key in keys {
                let child = node_of(&mut graph, key);
                for parent in mounts[key].iter().filter_map(|mount| mount.parent.as_ref()) {
                    let parent = node_of(&mut graph, parent);
                    graph.add_edge(parent, child, ());
                }
            }
            // `tarjan_scc` gives each component after the components that it has edges to.
            let mut components = tarjan_scc(&graph);
            components.reverse();
            let mut chains: HashMap<BindingKey, Vec<Vec<Part>>> = HashMap::new();
            let mut depths: HashMap<BindingKey, usize> = HashMap::new();
            for component in components {
                let cyclic = component.len() > 1 || graph.contains_edge(component[0], component[0]);
                for node in component {
                    let key = graph[node].clone();
                    if cyclic {
                        chains.insert(key, vec![vec![Part::Unknown("mount cycle".to_string())]]);
                        continue;
                    }
                    let Some(own) = mounts.get(&key) else {
                        continue;
                    };
                    let parent_depth = own
                        .iter()
                        .filter_map(|mount| mount.parent.as_ref())
                        .filter_map(|parent| depths.get(parent))
                        .max()
                        .copied()
                        .unwrap_or(0);
                    depths.insert(key.clone(), parent_depth + 1);
                    if parent_depth >= MAX_HOPS {
                        chains.insert(key, vec![vec![Part::Unknown("...".to_string())]]);
                        continue;
                    }
                    let mut found: Vec<Vec<Part>> = Vec::new();
                    for mount in own {
                        let empty = vec![Vec::new()];
                        let outer = match &mount.parent {
                            Some(parent) => chains.get(parent).unwrap_or(&empty),
                            None => &empty,
                        };
                        for chain in outer {
                            let chain = joined(chain.clone(), &mount.prefix);
                            if !found.contains(&chain) {
                                found.push(chain);
                            }
                            if found.len() > MAX_MOUNT_CHAINS {
                                break;
                            }
                        }
                        if found.len() > MAX_MOUNT_CHAINS {
                            break;
                        }
                    }
                    if found.len() > MAX_MOUNT_CHAINS {
                        found = vec![vec![Part::Unknown("prefix".to_string())]];
                    }
                    chains.insert(key, found);
                }
            }
            chains
        })
    }

    /// The nearest symbol that contains the node, other than a field.
    fn enclosing_symbol(&self, node: Node<'_>) -> Option<&'a crate::index::model::Symbol> {
        let ends = self.ecma.symbol_ends.get_or_init(|| {
            let mut ends: HashMap<usize, Vec<usize>> = HashMap::new();
            for (position, symbol) in self.file.symbols.iter().enumerate() {
                if symbol.kind != SymbolKind::Field {
                    ends.entry(symbol.range.end_byte)
                        .or_default()
                        .push(position);
                }
            }
            ends
        });
        let mut current = node.parent();
        while let Some(ancestor) = current {
            let found = ends.get(&ancestor.end_byte()).and_then(|positions| {
                positions
                    .iter()
                    .map(|position| &self.file.symbols[*position])
                    .filter(|symbol| symbol.range.start_byte <= ancestor.start_byte())
                    .max_by_key(|symbol| symbol.range.start_byte)
            });
            if found.is_some() {
                return found;
            }
            current = ancestor.parent();
        }
        None
    }

    /// The symbol of the handler, or the key and the facts of an inline handler.
    fn ecma_handler(&self, handler: Node<'_>) -> (Option<String>, Option<InlineHandler>) {
        let handler = unwrap(handler);
        if !matches!(
            handler.kind(),
            "arrow_function" | "function_expression" | "function"
        ) {
            return (self.handler_symbol(handler), None);
        }
        let enclosing_symbol = self.enclosing_symbol(handler);
        // The index gives the function of `{ handler: () => {} }` the symbol of its property.
        // That symbol is the handler, not a function around it.
        if let (Some(symbol), Some(parent)) = (enclosing_symbol, handler.parent()) {
            if parent.kind() == "pair" && symbol.range.start_byte == parent.start_byte() {
                return (Some(symbol.key.to_string()), None);
            }
        }
        let enclosing = enclosing_symbol.map(|symbol| symbol.key.to_string());
        let start = handler.start_position();
        let place = format!("<inline@{}:{}>", start.row + 1, start.column);
        let key = match &enclosing {
            Some(enclosing) => format!("{enclosing}.{place}"),
            None => format!("{}#{place}", self.file.file),
        };
        (
            Some(key),
            Some(InlineHandler {
                range: Span::of(&handler),
                enclosing,
            }),
        )
    }

    pub(super) fn ecma(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        match node.kind() {
            "call_expression" => self.ecma_registration(node, out),
            kind if CLASSES.contains(&kind) => self.ecma_class(node, out),
            _ => {}
        }
    }

    fn ecma_registration(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        if callee.kind() != "member_expression" {
            return;
        }
        let Some(property) = callee.child_by_field_name("property") else {
            return;
        };
        let verb = self.text(property);
        if !HTTP_VERBS.contains(&verb) {
            return;
        }
        let arguments = Self::arguments_of(node);
        if arguments.len() < 2 {
            return;
        }
        let Some(receiver) = callee.child_by_field_name("object") else {
            return;
        };
        let unit = self.unit();
        let Some(framework) = self.framework_of(unit, receiver, 0) else {
            return;
        };
        let path = self.path_of(unit, arguments[0], 0);
        if resolved(&path).is_some_and(|path| !path.starts_with('/')) {
            return;
        }
        let mut handler = arguments[arguments.len() - 1];
        if framework == Framework::Fastify && unwrap(handler).kind() == "object" {
            // Fastify takes `get(path, { handler })` as well as `get(path, options, handler)`.
            let Some(value) = object_properties(unwrap(handler), unit.source)
                .into_iter()
                .find_map(|(key, value)| (key == "handler").then_some(value))
            else {
                return;
            };
            handler = value;
        }
        let guards: Vec<String> = arguments[1..arguments.len() - 1]
            .iter()
            .filter(|argument| matches!(argument.kind(), "identifier" | "member_expression"))
            .map(|argument| self.text(*argument).to_string())
            .collect();
        // A middleware that is a call or a function has no name to report. The options object
        // of Fastify is not a middleware.
        let is_options = |argument: Node<'_>| {
            framework == Framework::Fastify && unwrap(argument).kind() == "object"
        };
        let unread = arguments[1..arguments.len() - 1].iter().any(|argument| {
            let named = matches!(argument.kind(), "identifier" | "member_expression");
            !named && !is_options(*argument)
        });
        let (symbol, inline_handler) = self.ecma_handler(handler);
        let method = method_of(verb);
        let routes: Vec<Route> = match self.mount_prefixes(unit, receiver) {
            Some(prefixes) => prefixes
                .iter()
                .map(|prefix| route_of(&method, Some(prefix), &path))
                .collect(),
            None => vec![route_of(&method, None, &path)],
        };
        for route in routes {
            out.push(Draft {
                range: Span::of(&node),
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework: framework.name(),
                route: Some(route),
                guards: guards.clone(),
                basis: EntryBasis::Registered,
                variant: None,
                inline_handler: inline_handler.clone(),
                unknown: unread.then_some(GuardsUnknownReason::UnreadMiddleware),
            });
        }
    }

    /// Where the definition of the decorator is.
    fn decorator_origin(&self, unit: Unit<'_>, call: &DecoratorCall<'_>) -> Option<Origin> {
        let name = text(call.callee, unit.source);
        match self.declared(unit, call.callee, name)?.1 {
            Declared::Import => {
                let binding = self.import_named(unit.position, name)?;
                let imported = binding.imported_name.clone()?;
                if let Some(package) = PACKAGES
                    .iter()
                    .find(|package| package.module == binding.module_path)
                {
                    return Some(Origin::Package(package, imported));
                }
                if matches!(imported.as_str(), "*" | "default") {
                    return None;
                }
                let target = self.target_position(binding)?;
                let (position, declared) = self.exported(target, &imported, 0)?;
                Some(Origin::Repository {
                    module: binding.module_path.clone(),
                    position,
                    declared,
                })
            }
            declared @ Declared::Const(_) => Some(Origin::Repository {
                module: String::new(),
                position: unit.position,
                declared,
            }),
            _ => None,
        }
    }

    /// The method of a decorator of the repository whose definition is `factory("verb")`.
    fn repository_verb(&self, position: usize, declared: &Declared) -> Option<String> {
        let Declared::Const(range) = declared else {
            return None;
        };
        self.with_unit(position, |unit| {
            let value = unwrap(node_at(unit.root, range)?);
            if value.kind() != "call_expression" {
                return None;
            }
            let first = *Self::arguments_of(value).first()?;
            if first.kind() != "string" {
                return None;
            }
            let verb = string_value(first, unit.source)?;
            HTTP_VERBS
                .contains(&verb.to_ascii_lowercase().as_str())
                .then(|| method_of(&verb))
        })
    }

    /// The default value of the first parameter of a decorator of the repository, as a path.
    fn repository_default_path(&self, position: usize, declared: &Declared) -> Option<Vec<Part>> {
        let Declared::Const(range) = declared else {
            return None;
        };
        self.with_unit(position, |unit| {
            let value = unwrap(node_at(unit.root, range)?);
            if !matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function"
            ) {
                return None;
            }
            let parameters = value.child_by_field_name("parameters")?;
            let first = named_children(parameters).into_iter().next()?;
            let default = match first.kind() {
                "required_parameter" | "optional_parameter" => first.child_by_field_name("value"),
                "assignment_pattern" => first.child_by_field_name("right"),
                _ => None,
            }?;
            let parts = self.path_of(unit, default, 0);
            resolved(&parts).is_some().then_some(parts)
        })
    }

    /// The paths that a decorator argument gives: a path, a list of paths, or an object with the
    /// key `path`. No argument gives the empty path.
    fn decorator_paths(&self, unit: Unit<'_>, argument: Option<Node<'_>>) -> Vec<Vec<Part>> {
        let mut argument = argument;
        // A loop in place of recursion, because objects in `path` can nest to any depth.
        loop {
            let Some(current) = argument.map(unwrap) else {
                return vec![Vec::new()];
            };
            match current.kind() {
                "array" => {
                    return named_children(current)
                        .into_iter()
                        .map(|element| self.path_of(unit, element, 0))
                        .collect()
                }
                "object" => {
                    let path = object_properties(current, unit.source)
                        .into_iter()
                        .find_map(|(key, value)| (key == "path").then_some(value));
                    match path {
                        Some(path) => argument = Some(path),
                        None => return vec![Vec::new()],
                    }
                }
                _ => return vec![self.path_of(unit, current, 0)],
            }
        }
    }

    /// The guards that the options object of a verb decorator of the repository declares: each
    /// key with the value `true`, and each element of a `middlewares` list, or the name that is
    /// the value of `middlewares`.
    fn option_guards(&self, unit: Unit<'_>, options: Node<'_>) -> Vec<String> {
        let mut guards = Vec::new();
        if unwrap(options).kind() != "object" {
            return guards;
        }
        for (key, value) in object_properties(unwrap(options), unit.source) {
            let key = key.as_str();
            match value.kind() {
                "true" => guards.push(format!("{key}: true")),
                "array" if key == "middlewares" => guards.extend(
                    named_children(value)
                        .into_iter()
                        .map(|element| compact(element, unit.source)),
                ),
                "identifier" | "shorthand_property_identifier" | "member_expression"
                    if key == "middlewares" =>
                {
                    guards.push(compact(value, unit.source));
                }
                _ => {}
            }
        }
        guards
    }

    /// The guards of a decorator of a package: the arguments of a guard list, or the decorator.
    fn package_guards(
        package: &DecoratorPackage,
        imported: &str,
        call: &DecoratorCall<'_>,
        source: &[u8],
    ) -> Vec<String> {
        if package.guard_lists.contains(&imported) {
            return call
                .arguments
                .iter()
                .map(|argument| compact(*argument, source))
                .collect();
        }
        if package.guard_markers.contains(&imported) {
            return vec![decorator_label(call, source)];
        }
        Vec::new()
    }

    /// The decorators of the class: on the class and on the export statement around it.
    fn class_decorators<'t>(class: Node<'t>) -> Vec<Node<'t>> {
        let mut decorators = Vec::new();
        if let Some(parent) = class
            .parent()
            .filter(|parent| parent.kind() == "export_statement")
        {
            decorators.extend(
                children(parent)
                    .into_iter()
                    .filter(|child| child.kind() == "decorator"),
            );
        }
        decorators.extend(
            children(class)
                .into_iter()
                .filter(|child| child.kind() == "decorator"),
        );
        decorators
    }

    /// Each method of the class body with its decorators. In TypeScript a decorator of a method
    /// is a node of the class body before the method. In JavaScript it is a child of the method.
    fn decorated_methods<'t>(body: Node<'t>) -> Vec<(Node<'t>, Vec<Node<'t>>)> {
        let mut methods = Vec::new();
        let mut pending = Vec::new();
        for child in children(body) {
            match child.kind() {
                "decorator" => pending.push(child),
                "method_definition" => {
                    let mut decorators = std::mem::take(&mut pending);
                    decorators.extend(
                        children(child)
                            .into_iter()
                            .filter(|inner| inner.kind() == "decorator"),
                    );
                    if !decorators.is_empty() {
                        methods.push((child, decorators));
                    }
                }
                "comment" | ";" | "{" | "}" => {}
                _ => pending.clear(),
            }
        }
        methods
    }

    fn ecma_class(&self, class: Node<'_>, out: &mut Vec<Draft>) {
        let Some(body) = class.child_by_field_name("body") else {
            return;
        };
        let methods = Self::decorated_methods(body);
        if methods.is_empty() {
            return;
        }
        let unit = self.unit();
        let class_calls: Vec<(DecoratorCall<'_>, Option<Origin>)> = Self::class_decorators(class)
            .into_iter()
            .filter_map(decorator_call)
            .map(|call| {
                let origin = self.decorator_origin(unit, &call);
                (call, origin)
            })
            .collect();
        for (method, decorators) in methods {
            let calls: Vec<(DecoratorCall<'_>, Option<Origin>)> = decorators
                .into_iter()
                .filter_map(decorator_call)
                .map(|call| {
                    let origin = self.decorator_origin(unit, &call);
                    (call, origin)
                })
                .collect();
            self.decorated_method(unit, method, &class_calls, &calls, out);
        }
    }

    /// The routes of one decorated method.
    fn decorated_method(
        &self,
        unit: Unit<'_>,
        method: Node<'_>,
        class_calls: &[(DecoratorCall<'_>, Option<Origin>)],
        calls: &[(DecoratorCall<'_>, Option<Origin>)],
        out: &mut Vec<Draft>,
    ) {
        for (index, (call, origin)) in calls.iter().enumerate() {
            let Some(origin) = origin else {
                continue;
            };
            let (verb, framework) = match origin {
                Origin::Package(package, imported) => {
                    let Some((_, verb)) = package
                        .verbs
                        .iter()
                        .find(|(name, _)| *name == imported.as_str())
                    else {
                        continue;
                    };
                    (verb.to_string(), package.framework)
                }
                Origin::Repository {
                    position, declared, ..
                } => {
                    let Some(verb) = self.repository_verb(*position, declared) else {
                        continue;
                    };
                    (verb, REPOSITORY_DECORATORS)
                }
            };
            let module = origin.module();
            let prefixes = self.class_prefixes(unit, origin, class_calls);
            let mut guards = Vec::new();
            for (class_call, class_origin) in class_calls {
                if prefixes.1.contains(&class_call.node.id()) {
                    continue;
                }
                guards.extend(self.guards_of(unit, origin, class_call, class_origin.as_ref()));
            }
            for (other_index, (other, other_origin)) in calls.iter().enumerate() {
                if other_index == index {
                    if let (Origin::Repository { .. }, Some(options)) =
                        (origin, call.arguments.get(1))
                    {
                        guards.extend(self.option_guards(unit, *options));
                    }
                    continue;
                }
                let is_verb = match other_origin {
                    Some(Origin::Package(package, imported)) if package.module == module => {
                        package.verbs.iter().any(|(name, _)| name == imported)
                    }
                    Some(Origin::Repository {
                        module: other_module,
                        position,
                        declared,
                    }) if other_module == module => {
                        self.repository_verb(*position, declared).is_some()
                    }
                    _ => false,
                };
                if !is_verb {
                    guards.extend(self.guards_of(unit, origin, other, other_origin.as_ref()));
                }
            }
            let symbol = method
                .child_by_field_name("name")
                .and_then(|name| self.symbol_at(name));
            for prefix in &prefixes.0 {
                for path in self.decorator_paths(unit, call.arguments.first().copied()) {
                    out.push(Draft {
                        range: Span::of(&method),
                        kind: EntryKind::HttpHandler,
                        symbol: symbol.clone(),
                        framework,
                        route: Some(route_of(&verb, Some(prefix), &path)),
                        guards: guards.clone(),
                        basis: EntryBasis::Declared,
                        variant: None,
                        inline_handler: None,
                        unknown: None,
                    });
                }
            }
        }
    }

    /// The guards that one decorator gives to a route of `route_origin`.
    fn guards_of(
        &self,
        unit: Unit<'_>,
        route_origin: &Origin,
        call: &DecoratorCall<'_>,
        origin: Option<&Origin>,
    ) -> Vec<String> {
        match (route_origin, origin) {
            (Origin::Package(package, _), Some(Origin::Package(other, imported)))
                if package.module == other.module =>
            {
                Self::package_guards(other, imported, call, unit.source)
            }
            (Origin::Repository { module, .. }, Some(Origin::Repository { module: other, .. }))
                if module == other =>
            {
                vec![decorator_label(call, unit.source)]
            }
            _ => Vec::new(),
        }
    }

    /// The prefixes of the class for a route of `origin`, and the ids of the decorators that give
    /// them. A class without a prefix decorator has the empty prefix.
    fn class_prefixes(
        &self,
        unit: Unit<'_>,
        origin: &Origin,
        class_calls: &[(DecoratorCall<'_>, Option<Origin>)],
    ) -> (Vec<Vec<Part>>, Vec<usize>) {
        let mut candidates: Vec<(usize, Vec<Vec<Part>>)> = Vec::new();
        for (call, class_origin) in class_calls {
            let Some(class_origin) = class_origin else {
                continue;
            };
            match (origin, class_origin) {
                (Origin::Package(package, _), Origin::Package(other, imported))
                    if package.module == other.module
                        && other.prefixes.contains(&imported.as_str()) =>
                {
                    candidates.push((
                        call.node.id(),
                        self.decorator_paths(unit, call.arguments.first().copied()),
                    ));
                }
                (
                    Origin::Repository { module, .. },
                    Origin::Repository {
                        module: other,
                        position,
                        declared,
                    },
                ) if module == other => {
                    if self.repository_verb(*position, declared).is_some() {
                        continue;
                    }
                    let prefix = match call.arguments.first() {
                        Some(first) => {
                            let first = unwrap(*first);
                            let parts = self.path_of(unit, first, 0);
                            let is_path = matches!(
                                first.kind(),
                                "string" | "template_string" | "binary_expression"
                            ) || resolved(&parts).is_some();
                            is_path.then_some(parts)
                        }
                        None => self.repository_default_path(*position, declared),
                    };
                    if let Some(prefix) = prefix {
                        candidates.push((call.node.id(), vec![prefix]));
                    }
                }
                _ => {}
            }
        }
        match candidates.len() {
            0 => (vec![Vec::new()], Vec::new()),
            1 => {
                let (id, prefixes) = candidates.remove(0);
                (prefixes, vec![id])
            }
            _ => (
                vec![vec![Part::Unknown("prefix".to_string())]],
                candidates.iter().map(|(id, _)| *id).collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{assert_scales_linearly, entries, entries_timed};
    use crate::core::Language;
    use crate::index::security::entry::{EntryKind, EntryPoint, GuardsUnknownReason};

    /// The method, the path, the handler symbol, the guards, and the framework of each route.
    type Row = (String, Option<String>, Option<String>, Vec<String>, String);

    fn http(files: &[(&str, &str)]) -> Vec<EntryPoint> {
        entries(Language::TypeScript, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .collect()
    }

    fn rows(files: &[(&str, &str)]) -> Vec<Row> {
        http(files)
            .into_iter()
            .map(|entry| {
                let route = entry.route.expect("a route");
                (
                    route.method,
                    route.path,
                    entry.symbol,
                    entry.guards,
                    entry.framework,
                )
            })
            .collect()
    }

    fn row(
        method: &str,
        path: Option<&str>,
        symbol: Option<&str>,
        guards: &[&str],
        framework: &str,
    ) -> Row {
        (
            method.to_string(),
            path.map(str::to_string),
            symbol.map(str::to_string),
            guards.iter().map(|guard| guard.to_string()).collect(),
            framework.to_string(),
        )
    }

    /// The method and the path, or the template, of each route.
    fn paths(files: &[(&str, &str)]) -> Vec<(String, Option<String>, Option<String>)> {
        http(files)
            .into_iter()
            .map(|entry| {
                let route = entry.route.expect("a route");
                (route.method, route.path, route.path_template)
            })
            .collect()
    }

    fn known(method: &str, path: &str) -> (String, Option<String>, Option<String>) {
        (method.to_string(), Some(path.to_string()), None)
    }

    fn unknown(method: &str, template: &str) -> (String, Option<String>, Option<String>) {
        (method.to_string(), None, Some(template.to_string()))
    }

    const NEST: &str = "import { Controller, Get, Post, UseGuards } from '@nestjs/common';\nimport express from 'express';\nimport { exec } from 'child_process';\nconst PATH = '/metrics';\n@Controller('cats')\nexport class CatsController {\n  @Get(':id') find() { return 1; }\n  @UseGuards(AuthGuard) @Post() run(cmd: string) { exec(cmd); }\n}\nexport class Server {\n  app = express();\n  setup() {\n    const app = this.app; app.get('/health', (_req, res) => res.send('ok'));\n    this.app.post(PATH, (req, res) => { exec(req.body.cmd); res.end(); });\n  }\n}\n";

    /// The key of the inline handler that starts at the first `marker` on the line.
    fn inline_key(source: &str, enclosing: &str, line: usize, marker: &str) -> String {
        let text = source.lines().nth(line - 1).expect("the line");
        let column = text.find(marker).expect("the marker");
        format!("{enclosing}.<inline@{line}:{column}>")
    }

    #[test]
    fn a_nest_controller_and_express_registrations_through_a_field_are_routes() {
        let found = rows(&[("src/c.ts", NEST)]);
        assert_eq!(
            found,
            [
                row(
                    "GET",
                    Some("/cats/:id"),
                    Some("src/c.ts#CatsController.find"),
                    &[],
                    "nestjs"
                ),
                row(
                    "POST",
                    Some("/cats"),
                    Some("src/c.ts#CatsController.run"),
                    &["AuthGuard"],
                    "nestjs"
                ),
                row(
                    "GET",
                    Some("/health"),
                    Some(&inline_key(NEST, "src/c.ts#Server.setup", 13, "(_req")),
                    &[],
                    "express"
                ),
                row(
                    "POST",
                    Some("/metrics"),
                    Some(&inline_key(NEST, "src/c.ts#Server.setup", 14, "(req")),
                    &[],
                    "express"
                ),
            ]
        );
        let inline = http(&[("src/c.ts", NEST)])[3]
            .inline_handler
            .clone()
            .expect("an inline handler");
        assert_eq!(inline.enclosing.as_deref(), Some("src/c.ts#Server.setup"));
        assert_eq!(inline.range.start_line, 14);
    }

    /// `decorator_paths` reads each level of an object in the `path` of an object.
    #[test]
    fn a_nest_controller_with_a_deep_path_object() {
        use crate::parser::deep::{nest, on_small_stack};
        let source = format!(
            "import {{ Controller, Get }} from '@nestjs/common';\n@Controller({})\nexport class Dogs {{\n  @Get('a')\n  one() {{}}\n}}\n",
            nest("{ path: ", "'dogs'", " }")
        );
        let found = on_small_stack(move || paths(&[("src/dogs.ts", source.as_str())]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.as_deref(), Some("/dogs/a"));
    }

    #[test]
    fn nest_paths_join_the_controller_prefix_and_each_path_of_a_list() {
        let source = "import { All, Controller, Delete, Get, UseGuards } from '@nestjs/common';\n@UseGuards(RolesGuard)\n@Controller({ path: 'dogs' })\nexport class DogsController {\n  @Get(['a', 'b/:id'])\n  many() {}\n  @UseGuards(AuthGuard, OtherGuard)\n  @All()\n  any() {}\n}\n@Controller()\nexport class RootController {\n  @Delete('/x')\n  remove() {}\n}\n";
        let found = rows(&[("src/dogs.ts", source)]);
        assert_eq!(
            found,
            [
                row(
                    "GET",
                    Some("/dogs/a"),
                    Some("src/dogs.ts#DogsController.many"),
                    &["RolesGuard"],
                    "nestjs"
                ),
                row(
                    "GET",
                    Some("/dogs/b/:id"),
                    Some("src/dogs.ts#DogsController.many"),
                    &["RolesGuard"],
                    "nestjs"
                ),
                row(
                    "ANY",
                    Some("/dogs"),
                    Some("src/dogs.ts#DogsController.any"),
                    &["RolesGuard", "AuthGuard", "OtherGuard"],
                    "nestjs"
                ),
                row(
                    "DELETE",
                    Some("/x"),
                    Some("src/dogs.ts#RootController.remove"),
                    &[],
                    "nestjs"
                ),
            ]
        );
    }

    #[test]
    fn routing_controllers_and_tsoa_decorators_are_routes() {
        let users = "import { Authorized, Get, JsonController, Post, UseBefore } from 'routing-controllers';\n@JsonController('/users')\nexport class UserController {\n  @Authorized()\n  @Get('/:id')\n  one() {}\n  @UseBefore(rateLimit)\n  @Post('/')\n  create() {}\n}\n";
        let items = "import { Get, Route, Security } from 'tsoa';\n@Route('items')\n@Security('api_key')\nexport class ItemsController {\n  @Get('{itemId}')\n  public async item() {}\n}\n";
        let found = rows(&[("src/users.ts", users), ("src/items.ts", items)]);
        assert_eq!(
            found,
            [
                row(
                    "GET",
                    Some("/items/{itemId}"),
                    Some("src/items.ts#ItemsController.item"),
                    &["Security('api_key')"],
                    "tsoa"
                ),
                row(
                    "GET",
                    Some("/users/:id"),
                    Some("src/users.ts#UserController.one"),
                    &["Authorized()"],
                    "routing-controllers"
                ),
                row(
                    "POST",
                    Some("/users/"),
                    Some("src/users.ts#UserController.create"),
                    &["rateLimit"],
                    "routing-controllers"
                ),
            ]
        );
        let parameters = http(&[("src/items.ts", items)])[0]
            .route
            .clone()
            .unwrap()
            .parameters;
        assert_eq!(parameters, ["itemId"]);
    }

    const ROUTE_FACTORY: &str = "type Method = 'get' | 'post' | 'delete';\nconst RouteFactory =\n  (method: Method) =>\n  (path: string, options: { skipAuth?: boolean; middlewares?: unknown[] } = {}): MethodDecorator =>\n  (target, handlerName) => {};\nexport const Get = RouteFactory('get');\nexport const Post = RouteFactory('post');\nexport const Delete = RouteFactory('delete');\nexport const Fetch = RouteFactory('fetch' as Method);\n";
    const REST_CONTROLLER: &str = "export const RestController =\n  (basePath: `/${string}` = '/'): ClassDecorator =>\n  (target) => target;\n";
    const SCOPED: &str = "export const GlobalScope = (scope: string): MethodDecorator => () => {};\nexport const Licensed = (feature: string): MethodDecorator => () => {};\n";
    const DECORATORS_INDEX: &str = "export { Get, Post, Delete, Fetch } from './route';\nexport { RestController } from './rest-controller';\nexport * from './scoped';\n";
    const WORKFLOWS: &str = "import { Delete, Fetch, Get, GlobalScope, Licensed, Post, RestController } from '../decorators';\nimport { Service } from '../di';\n@RestController('/workflows')\n@Service()\nexport class WorkflowsController {\n  @Get('/:id')\n  @GlobalScope('workflow:read')\n  async one() {}\n  @Post('/', { skipAuth: true, middlewares: [audit] })\n  @Licensed('feat:sharing')\n  async create() {}\n  @Fetch('/never')\n  async never() {}\n}\n@RestController()\nexport class RootController {\n  @Delete('/x')\n  async remove() {}\n}\n";

    #[test]
    fn a_decorator_of_the_repository_is_a_verb_when_a_factory_call_with_a_verb_defines_it() {
        let found = rows(&[
            ("src/decorators/route.ts", ROUTE_FACTORY),
            ("src/decorators/rest-controller.ts", REST_CONTROLLER),
            ("src/decorators/scoped.ts", SCOPED),
            ("src/decorators/index.ts", DECORATORS_INDEX),
            (
                "src/di.ts",
                "export const Service = () => (target: unknown) => target;\n",
            ),
            ("src/controllers/workflows.ts", WORKFLOWS),
        ]);
        assert_eq!(
            found,
            [
                row(
                    "GET",
                    Some("/workflows/:id"),
                    Some("src/controllers/workflows.ts#WorkflowsController.one"),
                    &["GlobalScope('workflow:read')"],
                    "repository_decorators"
                ),
                row(
                    "POST",
                    Some("/workflows/"),
                    Some("src/controllers/workflows.ts#WorkflowsController.create"),
                    &["skipAuth: true", "audit", "Licensed('feat:sharing')"],
                    "repository_decorators"
                ),
                row(
                    "DELETE",
                    Some("/x"),
                    Some("src/controllers/workflows.ts#RootController.remove"),
                    &[],
                    "repository_decorators"
                ),
            ]
        );
    }

    #[test]
    fn a_decorator_named_like_a_verb_without_evidence_is_not_a_route() {
        let cache = "export function Get(key: string) {\n  return (target: unknown) => target;\n}\nexport const Post = memo;\n";
        let source = "import { Get, Post } from './cache';\nimport { Put } from 'some-library';\nimport { Controller } from '@nestjs/common';\n@Controller('a')\nexport class Store {\n  @Get('/x')\n  one() {}\n  @Post('/y')\n  two() {}\n  @Put('/z')\n  three() {}\n  // @Get('/comment')\n  four() { return \"@Get('/string')\"; }\n}\n";
        assert!(http(&[("src/cache.ts", cache), ("src/store.ts", source)]).is_empty());
    }

    #[test]
    fn a_member_receiver_is_a_framework_value_by_its_field_its_type_or_its_superclass() {
        let source = "import express, { Router } from 'express';\nclass A { app = express(); start() { this.app.get('/a', a); } }\nclass B {\n  private app: express.Application;\n  constructor() { this.app = express(); }\n  start() { this.app.post('/b', b); }\n}\nclass C {\n  constructor(private readonly router: Router) {}\n  start() { this.router.put('/c', c); }\n}\nclass D { cache = new Map(); start() { this.cache.get('/d', d); } }\nclass E { app = express(); start() { [1].forEach(function () { this.app.get('/e', e); }); } }\nclass F { app = express(); start() { const app = this.app; app.patch('/f', f); } }\n";
        let base = "import express from 'express';\nexport abstract class AbstractServer {\n  protected app: express.Application;\n  constructor() { this.app = express(); }\n}\n";
        let server = "import { AbstractServer } from './base';\nexport class Server extends AbstractServer {\n  configure() { this.app.delete('/s', handler); }\n}\n";
        assert_eq!(
            paths(&[
                ("src/a.ts", source),
                ("src/base.ts", base),
                ("src/server.ts", server)
            ]),
            [
                known("GET", "/a"),
                known("POST", "/b"),
                known("PUT", "/c"),
                known("PATCH", "/f"),
                known("DELETE", "/s"),
            ]
        );
    }

    #[test]
    fn a_path_of_constants_resolves_and_any_other_part_stays_in_the_template() {
        let constants = "export const METRICS_PATH = '/metrics';\n";
        let source = "import express from 'express';\nimport { METRICS_PATH } from './constants';\nconst BASE = '/api';\nconst app = express();\napp.get(METRICS_PATH, one);\napp.get(`${BASE}/users/:id`, two);\napp.get(BASE + '/x', three);\nlet mutable = '/m';\napp.get(mutable, four);\napp.get(`/${config.rest}/e2e/:id`, five);\nfunction mount(prefix: string) {\n  const BASE = prefix;\n  app.get(BASE, six);\n}\napp.get('/a\\u0062', seven);\n";
        assert_eq!(
            paths(&[("src/constants.ts", constants), ("src/app.ts", source)]),
            [
                known("GET", "/metrics"),
                known("GET", "/api/users/:id"),
                known("GET", "/api/x"),
                unknown("GET", "${mutable}"),
                unknown("GET", "/${config.rest}/e2e/:id"),
                unknown("GET", "${BASE}"),
                known("GET", "/ab"),
            ]
        );
        let parameters: Vec<Vec<String>> =
            http(&[("src/constants.ts", constants), ("src/app.ts", source)])
                .into_iter()
                .map(|entry| entry.route.unwrap().parameters)
                .collect();
        assert_eq!(parameters[4], ["id"]);
    }

    #[test]
    fn a_mount_prefix_joins_the_routes_of_the_router_of_the_file() {
        let source = "import express from 'express';\nconst app = express();\nconst api = express.Router();\nconst admin = express.Router();\napi.get('/users', list);\nadmin.post('/reset', reset);\napi.use('/admin', admin);\napp.use('/api', api);\napp.use(`/${rest}/v2`, api);\nconst lone = express.Router();\nlone.get('/lone', single);\n";
        assert_eq!(
            paths(&[("src/app.ts", source)]),
            [
                unknown("GET", "/${rest}/v2/users"),
                known("GET", "/api/users"),
                unknown("POST", "/${rest}/v2/admin/reset"),
                known("POST", "/api/admin/reset"),
                known("GET", "/lone"),
            ]
        );
    }

    #[test]
    fn fastify_and_koa_router_registrations_are_routes() {
        let source = "import Fastify from 'fastify';\nimport Router from '@koa/router';\nconst server = Fastify();\nserver.get('/f', handlerF);\nconst router = new Router();\nrouter.post('/k', auth, handlerK);\n";
        let found: Vec<(String, String, Vec<String>)> = http(&[("src/app.ts", source)])
            .into_iter()
            .map(|entry| {
                (
                    entry.route.unwrap().path_text().to_string(),
                    entry.framework,
                    entry.guards,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("/f".to_string(), "fastify".to_string(), Vec::new()),
                (
                    "/k".to_string(),
                    "koa".to_string(),
                    vec!["auth".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn a_top_level_inline_handler_has_a_key_without_an_enclosing_symbol() {
        let source = "const express = require('express');\nconst app = express();\napp.post('/y', (req, res) => {});\n";
        let found = entries(Language::JavaScript, &[("server.js", source)]);
        assert_eq!(found[0].symbol.as_deref(), Some("server.js#<inline@3:15>"));
        assert_eq!(found[0].inline_handler.as_ref().unwrap().enclosing, None);
    }

    /// `count` routes in one function, each with a constant path, a mount, and an inline
    /// handler.
    fn many_routes(count: usize) -> std::time::Duration {
        let mut source = String::from("import express from 'express';\nconst BASE = '/api';\nexport class Server {\n  app = express();\n  setup() {\n");
        for index in 0..count {
            source.push_str(&format!(
                "    const r{index} = express.Router();\n    r{index}.get(`${{BASE}}/{index}`, (req, res) => res.end());\n    this.app.use('/m{index}', r{index});\n"
            ));
        }
        source.push_str("  }\n}\n");
        let (found, took) =
            entries_timed(Language::TypeScript, &[("src/server.ts", &source)], true);
        assert_eq!(found.len(), count);
        assert_eq!(
            found[count - 1].route.as_ref().unwrap().path.as_deref(),
            Some(format!("/m{}/api/{}", count - 1, count - 1).as_str())
        );
        took
    }

    #[test]
    fn many_routes_in_one_function_resolve() {
        many_routes(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_routes_in_one_function_resolve_in_linear_time() {
        assert_scales_linearly("express routes in one function", 4_000, many_routes);
    }

    /// A file with `layers` layers of `width` routers. Each router is mounted under each router
    /// of the layer above it, and the last router has the route `/leaf`.
    fn layered_mounts(layers: usize, width: usize) -> String {
        let mut source = String::from("import express from 'express';\nconst app = express();\n");
        for layer in 0..layers {
            for index in 0..width {
                source.push_str(&format!("const r{layer}_{index} = express.Router();\n"));
                let parents: Vec<String> = if layer == 0 {
                    vec!["app".to_string()]
                } else {
                    (0..width)
                        .map(|parent| format!("r{}_{parent}", layer - 1))
                        .collect()
                };
                for parent in parents {
                    source.push_str(&format!(
                        "{parent}.use('/l{layer}x{index}', r{layer}_{index});\n"
                    ));
                }
            }
        }
        source.push_str(&format!("r{}_0.get('/leaf', handler);\n", layers - 1));
        source
    }

    #[test]
    fn a_router_with_more_chains_of_mounts_than_the_limit_has_an_unknown_prefix() {
        let small = layered_mounts(3, 2);
        assert_eq!(paths(&[("src/app.ts", &small)]).len(), 4);
        let large = layered_mounts(8, 2);
        assert_eq!(
            paths(&[("src/app.ts", &large)]),
            [unknown("GET", "/${prefix}/leaf")]
        );
    }

    #[test]
    fn a_cycle_of_mounts_gives_an_unknown_prefix() {
        let source = "import express from 'express';\nconst app = express();\nconst a = express.Router();\nconst b = express.Router();\napp.use('/a', a);\na.use('/b', b);\nb.use('/a', a);\nb.get('/x', handler);\n";
        assert_eq!(
            paths(&[("src/app.ts", source)]),
            [unknown("GET", "/${mount cycle}/x")]
        );
    }

    fn many_mounts(count: usize) -> std::time::Duration {
        let source = layered_mounts(count / 4, 4);
        let (found, took) = entries_timed(Language::TypeScript, &[("src/app.ts", &source)], true);
        assert_eq!(found.len(), 1);
        took
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_layers_of_mounts_resolve_in_linear_time() {
        assert_scales_linearly("layers of mounts", 2_000, many_mounts);
    }

    fn many_assigned_variables(count: usize) -> std::time::Duration {
        let mut source = String::from("import express from 'express';\nfunction setup() {\n");
        for index in 0..count {
            source.push_str(&format!(
                "  let r{index};\n  r{index} = express();\n  r{index}.get('/r{index}', handler);\n"
            ));
        }
        source.push_str("}\nfunction handler() {}\n");
        let (found, took) = entries_timed(Language::TypeScript, &[("src/a.ts", &source)], true);
        assert_eq!(found.len(), count);
        took
    }

    #[test]
    fn each_variable_with_one_assignment_is_a_receiver() {
        many_assigned_variables(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_assigned_variables_resolve_in_linear_time() {
        assert_scales_linearly("assigned variables", 2_000, many_assigned_variables);
    }

    fn many_fields(count: usize) -> std::time::Duration {
        let mut source = String::from("import express from 'express';\nexport class Server {\n");
        for index in 0..count {
            source.push_str(&format!("  f{index} = express();\n"));
        }
        source.push_str("  setup() {\n");
        for index in 0..count {
            source.push_str(&format!("    this.f{index}.get('/r{index}', handler);\n"));
        }
        source.push_str("  }\n}\n");
        let (found, took) =
            entries_timed(Language::TypeScript, &[("src/server.ts", &source)], true);
        assert_eq!(found.len(), count);
        took
    }

    #[test]
    fn each_field_of_a_class_is_a_receiver() {
        many_fields(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_fields_of_one_class_resolve_in_linear_time() {
        assert_scales_linearly("fields of one class", 4_000, many_fields);
    }

    /// Each route as `METHOD path handler [guards]`.
    fn lines(files: &[(&str, &str)]) -> Vec<String> {
        http(files)
            .into_iter()
            .map(|entry| {
                let route = entry.route.expect("a route");
                format!(
                    "{} {} {} {:?}",
                    route.method,
                    route.path_text(),
                    entry.symbol.unwrap_or_default(),
                    entry.guards
                )
            })
            .collect()
    }

    #[test]
    fn a_comment_is_not_an_argument_of_a_decorator_or_a_registration() {
        let source = "import { Controller, Get } from '@nestjs/common';\nimport express from 'express';\n@Controller('cats')\nexport class Cats {\n  @Get(/* public */ 'health')\n  health() {}\n}\nconst app = express();\napp.get(/* public */ '/ping', ping);\nfunction ping() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            [
                "GET /cats/health src/a.ts#Cats.health []",
                "GET /ping src/a.ts#ping []"
            ]
        );
    }

    #[test]
    fn a_router_that_new_makes_is_a_router() {
        let source = "import express, { Router } from 'express';\nconst one = new express.Router();\none.get('/one', handler);\nconst two = new Router();\ntwo.get('/two', handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            [
                "GET /one src/a.ts#handler []",
                "GET /two src/a.ts#handler []"
            ]
        );
    }

    #[test]
    fn a_field_that_a_long_chain_did_not_resolve_resolves_from_a_nearer_start() {
        let mut source = String::from("import express from 'express';\nexport class Server {\n");
        for index in 0..12 {
            source.push_str(&format!("  f{index} = this.f{};\n", index + 1));
        }
        source.push_str("  f12 = express();\n  setup() {\n");
        for index in 0..13 {
            source.push_str(&format!("    this.f{index}.get('/f{index}', handler);\n"));
        }
        source.push_str("  }\n}\nfunction handler() {}\n");
        let found = lines(&[("src/a.ts", &source)]);
        for index in 7..13 {
            let route = format!("GET /f{index} src/a.ts#handler []");
            assert!(found.contains(&route), "{route}: {found:?}");
        }
    }

    #[test]
    fn a_static_field_is_not_the_instance_field_of_the_same_name() {
        let source = "import express from 'express';\nexport class Server {\n  app = express();\n  static app = new Map();\n  static reset() {\n    this.app = new Map();\n  }\n  setup() {\n    this.app.get('/health', handler);\n  }\n}\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            ["GET /health src/a.ts#handler []"]
        );
    }

    #[test]
    fn a_fastify_route_reads_the_handler_of_its_options() {
        let source = "import fastify from 'fastify';\nconst server = fastify();\nserver.get('/health', { handler: health });\nserver.post('/run', { schema: {}, handler: (request, reply) => reply.send() });\nfunction health() {}\n";
        let found = lines(&[("src/a.ts", source)]);
        assert_eq!(
            found,
            [
                "GET /health src/a.ts#health []",
                "POST /run src/a.ts#handler []"
            ]
        );
    }

    #[test]
    fn a_quoted_option_key_and_a_shorthand_option_are_read() {
        let route = "type Method = 'get';\nconst RouteFactory = (method: Method) => (path: string, options = {}) => () => {};\nexport const Get = RouteFactory('get');\n";
        let source = "import { Controller, Get as NestGet } from '@nestjs/common';\nimport { Get } from './route';\n@Controller({ 'path': 'cats' })\nexport class Cats {\n  @NestGet('all')\n  all() {}\n}\nconst middlewares = [audit];\nexport class Mine {\n  @Get('/list', { middlewares })\n  list() {}\n}\nfunction audit() {}\n";
        assert_eq!(
            lines(&[("src/route.ts", route), ("src/a.ts", source)]),
            [
                "GET /cats/all src/a.ts#Cats.all []",
                "GET /list src/a.ts#Mine.list [\"middlewares\"]"
            ]
        );
    }

    #[test]
    fn an_unresolved_part_keeps_its_source_text() {
        let source = "import express from 'express';\nconst app = express();\napp.get(`/${config['tenant  name']}/x`, handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            ["GET /${config['tenant  name']}/x src/a.ts#handler []"]
        );
    }

    #[test]
    fn a_long_chain_of_plus_does_not_exhaust_the_stack() {
        let terms: Vec<String> = (0..50_000).map(|index| format!("'/p{index}'")).collect();
        let source = format!(
            "import express from 'express';\nconst app = express();\napp.get({}, handler);\nfunction handler() {{}}\n",
            terms.join(" + ")
        );
        let found = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || http(&[("src/a.ts", &source)]).len())
            .expect("a thread")
            .join()
            .expect("no stack overflow");
        assert_eq!(found, 1);
    }

    fn completeness(files: &[(&str, &str)]) -> Vec<(String, bool, Option<GuardsUnknownReason>)> {
        http(files)
            .into_iter()
            .map(|entry| {
                (
                    entry.framework,
                    entry.guards_complete,
                    entry.guards_unknown_reason,
                )
            })
            .collect()
    }

    #[test]
    fn each_typescript_framework_has_a_guard_layer_that_omen_does_not_read() {
        let layer = Some(GuardsUnknownReason::LayerNotRead);
        let express = "import express from 'express';\nconst app = express();\napp.use(auth);\napp.get('/a', handler);\nfunction auth() {}\nfunction handler() {}\n";
        let fastify = "import fastify from 'fastify';\nconst server = fastify();\nserver.get('/a', handler);\nfunction handler() {}\n";
        let koa = "import Router from '@koa/router';\nconst router = new Router();\nrouter.get('/a', handler);\nfunction handler() {}\n";
        let nest = "import { Controller, Get } from '@nestjs/common';\n@Controller('c')\nexport class C {\n  @Get('a')\n  a() {}\n}\n";
        let route = "type Method = 'get';\nconst RouteFactory = (method: Method) => (path: string) => () => {};\nexport const Get = RouteFactory('get');\n";
        let mine =
            "import { Get } from './route';\nexport class Mine {\n  @Get('/a')\n  a() {}\n}\n";
        for (files, framework) in [
            (vec![("src/a.ts", express)], "express"),
            (vec![("src/a.ts", fastify)], "fastify"),
            (vec![("src/a.ts", koa)], "koa"),
            (vec![("src/a.ts", nest)], "nestjs"),
            (
                vec![("src/route.ts", route), ("src/mine.ts", mine)],
                "repository_decorators",
            ),
        ] {
            assert_eq!(
                completeness(&files),
                [(framework.to_string(), false, layer)],
                "{framework}"
            );
        }
    }

    #[test]
    fn a_middleware_of_a_route_that_is_not_a_name_is_unread() {
        let source = "import express from 'express';\nimport fastify from 'fastify';\nconst app = express();\napp.get('/call', limit(5), handler);\napp.get('/inline', (req, res, next) => next(), handler);\napp.get('/name', auth, handler);\nconst server = fastify();\nserver.get('/options', { schema: {} }, handler);\nfunction auth() {}\nfunction limit(count: number) { return auth; }\nfunction handler() {}\n";
        let unread = Some(GuardsUnknownReason::UnreadMiddleware);
        let layer = Some(GuardsUnknownReason::LayerNotRead);
        assert_eq!(
            completeness(&[("src/a.ts", source)]),
            [
                ("express".to_string(), false, unread),
                ("express".to_string(), false, unread),
                ("express".to_string(), false, layer),
                ("fastify".to_string(), false, layer),
            ]
        );
    }

    fn routes_on_a_small_stack(source: String) -> usize {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || http(&[("src/a.ts", &source)]).len())
            .expect("a thread")
            .join()
            .expect("no stack overflow")
    }

    #[test]
    fn a_deep_nest_of_templates_does_not_exhaust_the_stack() {
        let depth = 20_000;
        let path = format!("{}x{}", "`/${".repeat(depth), "}`".repeat(depth));
        let source = format!(
            "import express from 'express';\nconst app = express();\napp.get({path}, handler);\nfunction handler() {{}}\n"
        );
        assert_eq!(routes_on_a_small_stack(source), 1);
    }

    #[test]
    fn a_deep_destructuring_assignment_does_not_exhaust_the_stack() {
        let depth = 20_000;
        let pattern = format!("{}app{}", "[".repeat(depth), "]".repeat(depth));
        let source = format!(
            "import express from 'express';\nlet app = express();\n{pattern} = [];\napp.get('/x', handler);\nfunction handler() {{}}\n"
        );
        assert_eq!(routes_on_a_small_stack(source), 0);
    }

    #[test]
    fn two_re_exports_of_one_decorator_are_one_decorator() {
        let route = "const RouteFactory = (method: string) => (path: string) => () => {};\nexport const Get = RouteFactory('get');\n";
        let source =
            "import { Get } from './index';\nexport class Mine {\n  @Get('/x')\n  x() {}\n}\n";
        assert_eq!(
            lines(&[
                ("src/route.ts", route),
                ("src/a.ts", "export { Get } from './route';\n"),
                ("src/b.ts", "export { Get } from './route';\n"),
                (
                    "src/index.ts",
                    "export * from './a';\nexport * from './b';\n"
                ),
                ("src/mine.ts", source),
            ]),
            ["GET /x src/mine.ts#Mine.x []"]
        );
    }

    #[test]
    fn a_private_binding_of_a_module_is_not_its_export() {
        let source = "import express from 'express';\nimport { PATH } from './barrel';\nconst app = express();\napp.get(PATH, handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[
                ("src/paths.ts", "export const PATH = '/public';\n"),
                (
                    "src/barrel.ts",
                    "const PATH = '/internal';\nexport * from './paths';\n"
                ),
                ("src/a.ts", source),
            ]),
            ["GET /public src/a.ts#handler []"]
        );
    }

    #[test]
    fn a_variable_with_one_assignment_has_the_assigned_value() {
        let source = "import express from 'express';\nlet app;\napp = express();\napp.get('/health', handler);\nlet other;\nother = express();\nother = new Map();\nother.get('/never', handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            ["GET /health src/a.ts#handler []"]
        );
    }

    #[test]
    fn a_write_to_a_property_is_not_a_write_of_the_variable() {
        let source = "import express from 'express';\nlet app = express();\napp.locals = {};\napp.get('/health', handler);\nlet other = express();\nother = new Map();\nother.get('/never', handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            ["GET /health src/a.ts#handler []"]
        );
    }

    #[test]
    fn an_alias_of_a_router_is_the_router_in_a_mount() {
        let source = "import express from 'express';\nconst app = express();\nconst router = express.Router();\nconst alias = router;\napp.use('/api', alias);\nrouter.get('/health', handler);\nfunction handler() {}\n";
        assert_eq!(
            lines(&[("src/a.ts", source)]),
            ["GET /api/health src/a.ts#handler []"]
        );
    }
}
