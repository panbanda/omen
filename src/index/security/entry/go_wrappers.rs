//! Router registrations through a router type of the repository.
//!
//! A router type of the repository is a type whose route methods send their pattern to a
//! router of a framework. The evidence is in the body of each method: a call of a framework
//! router that a field of the type holds, or a call of another route method of the type. A
//! list of type names is not evidence. A route method has one of these names: `Get`, `Post`,
//! `Put`, `Patch`, `Delete`, `Head`, `Options`, `Any`, `Methods`, `Combo`, `Group`, `Route`,
//! `Mount`, `Handle`, and `HandleFunc`.
//!
//! A method has one of these shapes:
//!
//! - A route method sends its pattern to a registration of the framework. Of the other
//!   arguments of a call, the last is the handler and the arguments before it are guards.
//! - A mount method sends its pattern to `Mount` of the framework, with a router of the type.
//! - A group method calls its `func()` argument, and it keeps its pattern in a field of the
//!   value. A route inside the function gets the pattern as a prefix. The other arguments are
//!   guards of each route inside the function.
//! - A combo method returns a value that keeps the pattern. Each method of the value sends the
//!   pattern to a route method of the type.
//!
//! The walk reads each function of the repository one time. A router is a value that a literal of
//! the type makes, a parameter of the type, or the result of a function that returns the type.
//! These make links between the routers: a call that gives a router to a parameter, a mount, and
//! the result of a function. The path of a route comes from each chain of links up to a root. A
//! root is a router that a literal gives, and that no link attaches. A parameter that no call of
//! the repository gives has no root, so its routes give no entry. A group joins the text of its
//! prefix to the path, as the type does. A mount joins the paths with the rules of the framework.
//! The files of tests give no routes and no links.
//!
//! These forms give no entry: a router in a package-level variable, a pattern that is not a
//! string literal, a mount prefix that is not a string literal, and a router in a cycle of links.
//! A middleware that a method other than a group method adds, as `Use`, is not a guard here.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;

use tree_sitter::Node;

use super::go_routes::{
    directory_of, is_http_token, package_names, parse_signature, path_parameters,
    servemux_parameters, servemux_pattern, Framework,
};
use super::{Context, Draft, EntryBasis, EntryKind, Route, TreeSource};
use crate::core::{is_test_file, Language};
use crate::index::extract::FileSymbols;
use crate::index::model::{ImportKind, ImportTarget, ResolutionStatus, Span, Symbol, SymbolKind};

/// The names of the methods that can be route methods of a router type.
const ROUTE_METHODS: [&str; 15] = [
    "Get",
    "Post",
    "Put",
    "Patch",
    "Delete",
    "Head",
    "Options",
    "Any",
    "Methods",
    "Combo",
    "Group",
    "Route",
    "Mount",
    "Handle",
    "HandleFunc",
];

/// The depth of an expression that the walk reads.
const EVAL_DEPTH_LIMIT: usize = 1000;

/// The number of chains of links that a router can have. A router with more chains gives no
/// entry, so that the cost stays in proportion to the size of the repository.
const PLACEMENT_LIMIT: usize = 256;

/// The router of a framework that a field of a router type holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Inner {
    Framework(Framework),
    ServeMux,
}

impl Inner {
    fn name(self) -> &'static str {
        match self {
            Self::Framework(framework) => framework.name(),
            Self::ServeMux => "net/http",
        }
    }

    /// The position of the pattern argument and the use of the other arguments, for a method
    /// of the router that registers a route or mounts a handler.
    fn registration(self, method: &str) -> Option<(usize, Forward)> {
        let fixed = || Some((0, Forward::Verbs(vec![Verb::Fixed(method.to_uppercase())])));
        let any = || Some((0, Forward::Verbs(vec![Verb::Any])));
        match self {
            Self::Framework(Framework::Chi) => match method {
                "Get" | "Post" | "Put" | "Patch" | "Delete" | "Head" | "Options" | "Connect"
                | "Trace" => fixed(),
                "Handle" | "HandleFunc" => any(),
                "Method" | "MethodFunc" => Some((1, Forward::Verbs(vec![Verb::Parameter(0)]))),
                "Mount" => Some((0, Forward::Mount(1))),
                _ => None,
            },
            Self::Framework(Framework::Echo) => match method {
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" | "CONNECT"
                | "TRACE" => fixed(),
                "Any" => any(),
                "Add" => Some((1, Forward::Verbs(vec![Verb::Parameter(0)]))),
                _ => None,
            },
            Self::Framework(Framework::Gin) => match method {
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => fixed(),
                "Any" => any(),
                "Handle" => Some((1, Forward::Verbs(vec![Verb::Parameter(0)]))),
                _ => None,
            },
            Self::Framework(Framework::Mux) | Self::ServeMux => match method {
                "Handle" | "HandleFunc" => any(),
                _ => None,
            },
        }
    }

    /// The path of a route from the prefixes of its mounts and its own path.
    fn compose(self, mounts: &[String], path: &str) -> Option<String> {
        match self {
            Self::Framework(framework) => framework.compose(mounts, path),
            Self::ServeMux => Some(mounts.iter().map(String::as_str).chain([path]).collect()),
        }
    }
}

/// What a call does with the pattern that a method sends.
#[derive(Clone, Debug)]
enum Forward {
    Verbs(Vec<Verb>),
    /// The argument at this position is the mounted router.
    Mount(usize),
}

/// The method of a route.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Verb {
    /// A text that the source writes: one method, or methods with a comma between them.
    Fixed(String),
    Any,
    /// The argument of the call at this position gives the method.
    Parameter(usize),
}

/// The shape of a route method of a router type.
#[derive(Clone, Debug)]
enum Shape {
    Route { pattern: usize, verbs: Vec<Verb> },
    Mount { pattern: usize, mounted: usize },
    Group { pattern: usize, body: usize },
    Combo { pattern: usize, combo: usize },
}

struct RouterType {
    inner: Inner,
    methods: HashMap<String, Shape>,
}

/// The type that a combo method returns: its route methods and their methods of HTTP.
struct ComboType {
    methods: HashMap<String, Vec<Verb>>,
}

/// A package directory and the name of a type of the package.
type TypeKey = (String, String);

/// An argument of a call in the body of a method.
#[derive(Clone, Default)]
struct ArgumentFact {
    /// The parameters that the argument mentions, directly or through local variables.
    parameters: BTreeSet<usize>,
    /// The value of a string literal.
    literal: Option<String>,
    /// The field of `receiver.field`.
    receiver_field: Option<String>,
    /// The argument mentions the receiver.
    receiver: bool,
}

enum CallTarget {
    /// `receiver.field.Method(...)`, also through a chain of `With` calls.
    Field { field: String, method: String },
    /// `receiver.Method(...)`.
    Own(String),
}

struct CallFact {
    target: CallTarget,
    arguments: Vec<ArgumentFact>,
}

/// The facts of a method declaration that the classification of its shape uses.
struct MethodFact {
    receiver_type: TypeKey,
    name: String,
    /// The type text of each parameter, in order.
    parameters: Vec<String>,
    calls: Vec<CallFact>,
    /// The parameters of type `func()` that the body calls.
    called_functions: BTreeSet<usize>,
    /// The parameters that an assignment to a field of the receiver mentions.
    stored: BTreeSet<usize>,
    /// The literals that a return statement gives: the type and the elements.
    returned_literals: Vec<(String, Vec<Element>)>,
}

/// An element of a composite literal: the key, when the source gives one, and the value.
type Element = (Option<String>, ArgumentFact);

/// An open group: its prefix, when the source gives it as a string literal, and its guards.
type OpenGroup = (Option<String>, Vec<String>);

enum FieldKind {
    Router(Inner),
    /// A type of the same package, with or without a pointer.
    Repository(String),
    Other,
}

/// The fields of each struct type, in order.
type Structs = HashMap<TypeKey, Vec<(String, FieldKind)>>;

/// The result of the analysis: the routes of each file.
#[derive(Default)]
pub(super) struct WrapperRoutes {
    by_file: HashMap<String, Vec<Found>>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    range: Span,
    method: String,
    path: String,
    guards: Vec<String>,
    symbol: Option<String>,
    framework: &'static str,
    parameters: Vec<String>,
}

pub(super) fn collect(context: &Context<'_>, _root: Node<'_>, out: &mut Vec<Draft>) {
    let routes = context
        .all_parsed
        .shared()
        .go_wrappers
        .get_or_init(|| WrapperRoutes::build(context.all, context.all_parsed));
    for found in routes.by_file.get(&context.file.file).into_iter().flatten() {
        out.push(Draft {
            range: found.range,
            kind: EntryKind::HttpHandler,
            symbol: found.symbol.clone(),
            framework: found.framework,
            route: Some(Route {
                method: found.method.clone(),
                path: found.path.clone(),
                parameters: found.parameters.clone(),
            }),
            guards: found.guards.clone(),
            basis: EntryBasis::Registered,
            variant: None,
            unknown: None,
        });
    }
}

/// Tells if a file takes part in the analysis: a Go file that is not a test.
fn is_source(file: &FileSymbols) -> bool {
    file.language == Language::Go && !is_test_file(&file.file)
}

/// The directory of the package that an import of the file gives to a qualifier.
fn import_directory<'f>(file: &'f FileSymbols, qualifier: &str) -> Option<&'f str> {
    let binding = file.imports.iter().find(|binding| {
        binding.kind == ImportKind::Module && binding.local_name.as_deref() == Some(qualifier)
    })?;
    let target = match &binding.target_file {
        ImportTarget::Resolved(file) => file.as_str(),
        ImportTarget::Package(files) => files.first()?.as_str(),
        _ => return None,
    };
    Some(directory_of(target))
}

/// The type of the repository that a type text of the file names, as `*Router` or `*web.Router`.
fn resolve_type(file: &FileSymbols, text: &str) -> Option<TypeKey> {
    let text = text.trim();
    let text = text.strip_prefix('*').unwrap_or(text);
    if text.is_empty()
        || !text
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '.')
    {
        return None;
    }
    match text.split_once('.') {
        Some((qualifier, name)) => Some((
            import_directory(file, qualifier)?.to_string(),
            name.to_string(),
        )),
        None => Some((directory_of(&file.file).to_string(), text.to_string())),
    }
}

/// The router of a framework that a field type names, as `chi.Router` or `*http.ServeMux`.
fn field_kind(file: &FileSymbols, text: &str) -> FieldKind {
    let text = text.trim();
    let bare = text.strip_prefix('*').unwrap_or(text);
    let Some((qualifier, name)) = bare.split_once('.') else {
        let simple = !bare.is_empty()
            && bare
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_');
        return if simple {
            FieldKind::Repository(bare.to_string())
        } else {
            FieldKind::Other
        };
    };
    for framework in Framework::ALL {
        let names = package_names(file, |module| framework.owns(module), framework.package());
        if names.iter().any(|known| known == qualifier) && framework.router_types().contains(&name)
        {
            return FieldKind::Router(Inner::Framework(framework));
        }
    }
    let http = package_names(file, |module| module == "net/http", "http");
    if name == "ServeMux" && http.iter().any(|known| known == qualifier) {
        return FieldKind::Router(Inner::ServeMux);
    }
    FieldKind::Other
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// The arguments of a call, in order.
fn call_arguments(call: Node<'_>) -> Vec<Node<'_>> {
    call.child_by_field_name("arguments")
        .map(named_children)
        .unwrap_or_default()
}

/// The names and the type texts of the parameters of a parameter list, in order.
fn parameter_list(context: &Context<'_>, list: Option<Node<'_>>) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for declaration in list.map(named_children).unwrap_or_default() {
        if !matches!(
            declaration.kind(),
            "parameter_declaration" | "variadic_parameter_declaration"
        ) {
            continue;
        }
        let kind = declaration
            .child_by_field_name("type")
            .map(|kind| context.text(kind).to_string())
            .unwrap_or_default();
        let mut cursor = declaration.walk();
        let names: Vec<Node<'_>> = declaration
            .children_by_field_name("name", &mut cursor)
            .collect();
        if names.is_empty() {
            found.push((String::new(), kind.clone()));
        }
        for name in names {
            found.push((context.text(name).to_string(), kind.clone()));
        }
    }
    found
}

/// The facts of the struct types and the methods of one file.
struct FileFacts {
    structs: Vec<(TypeKey, Vec<(String, FieldKind)>)>,
    methods: Vec<MethodFact>,
}

fn file_facts(context: &Context<'_>, root: Node<'_>) -> FileFacts {
    let directory = directory_of(&context.file.file).to_string();
    let mut facts = FileFacts {
        structs: Vec::new(),
        methods: Vec::new(),
    };
    for declaration in named_children(root) {
        match declaration.kind() {
            "type_declaration" => {
                for spec in named_children(declaration) {
                    let (Some(name), Some(kind)) = (
                        spec.child_by_field_name("name"),
                        spec.child_by_field_name("type"),
                    ) else {
                        continue;
                    };
                    if spec.kind() != "type_spec" || kind.kind() != "struct_type" {
                        continue;
                    }
                    let fields = struct_fields(context, kind);
                    facts
                        .structs
                        .push(((directory.clone(), context.text(name).to_string()), fields));
                }
            }
            "method_declaration" => {
                if let Some(method) = method_fact(context, declaration, &directory) {
                    facts.methods.push(method);
                }
            }
            _ => {}
        }
    }
    facts
}

fn struct_fields(context: &Context<'_>, kind: Node<'_>) -> Vec<(String, FieldKind)> {
    let mut fields = Vec::new();
    let mut pending = named_children(kind);
    pending.reverse();
    while let Some(node) = pending.pop() {
        if node.kind() == "field_declaration_list" {
            let mut children = named_children(node);
            children.reverse();
            pending.extend(children);
            continue;
        }
        if node.kind() != "field_declaration" {
            continue;
        }
        let Some(kind) = node.child_by_field_name("type") else {
            continue;
        };
        let text = context.text(kind);
        let mut cursor = node.walk();
        let names: Vec<String> = node
            .children_by_field_name("name", &mut cursor)
            .map(|name| context.text(name).to_string())
            .collect();
        let names = if names.is_empty() {
            let bare = text.trim_start_matches('*');
            vec![bare.rsplit('.').next().unwrap_or(bare).to_string()]
        } else {
            names
        };
        for name in names {
            fields.push((name, field_kind(context.file, text)));
        }
    }
    fields
}

/// The parameters that an expression mentions, directly or through the local variables.
fn mentions(
    context: &Context<'_>,
    node: Node<'_>,
    derived: &HashMap<String, BTreeSet<usize>>,
) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if current.kind() == "identifier" {
            if let Some(parameters) = derived.get(context.text(current)) {
                found.extend(parameters.iter().copied());
            }
        }
        pending.extend(named_children(current));
    }
    found
}

/// Tells if an expression mentions the name.
fn mentions_name(context: &Context<'_>, node: Node<'_>, name: &str) -> bool {
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if current.kind() == "identifier" && context.text(current) == name {
            return true;
        }
        pending.extend(named_children(current));
    }
    false
}

fn argument_fact(
    context: &Context<'_>,
    node: Node<'_>,
    derived: &HashMap<String, BTreeSet<usize>>,
    receiver: &str,
) -> ArgumentFact {
    let receiver_field = (node.kind() == "selector_expression")
        .then(|| {
            let operand = node.child_by_field_name("operand")?;
            let field = node.child_by_field_name("field")?;
            (operand.kind() == "identifier" && context.text(operand) == receiver)
                .then(|| context.text(field).to_string())
        })
        .flatten();
    ArgumentFact {
        parameters: mentions(context, node, derived),
        literal: context.go_string(node),
        receiver_field,
        receiver: !receiver.is_empty() && mentions_name(context, node, receiver),
    }
}

/// The receiver of a call `receiver.field.Method(...)` or `receiver.Method(...)`. A chain of
/// `With` calls between the field and the method adds middleware, so the walk passes it.
fn call_target(context: &Context<'_>, function: Node<'_>, receiver: &str) -> Option<CallTarget> {
    if function.kind() != "selector_expression" {
        return None;
    }
    let method = context
        .text(function.child_by_field_name("field")?)
        .to_string();
    let mut operand = function.child_by_field_name("operand")?;
    while operand.kind() == "call_expression" {
        let inner = operand.child_by_field_name("function")?;
        if inner.kind() != "selector_expression"
            || inner
                .child_by_field_name("field")
                .is_none_or(|field| context.text(field) != "With")
        {
            return None;
        }
        operand = inner.child_by_field_name("operand")?;
    }
    match operand.kind() {
        "identifier" if context.text(operand) == receiver => Some(CallTarget::Own(method)),
        "selector_expression" => {
            let base = operand.child_by_field_name("operand")?;
            let field = operand.child_by_field_name("field")?;
            (base.kind() == "identifier" && context.text(base) == receiver).then(|| {
                CallTarget::Field {
                    field: context.text(field).to_string(),
                    method,
                }
            })
        }
        _ => None,
    }
}

fn method_fact(context: &Context<'_>, node: Node<'_>, directory: &str) -> Option<MethodFact> {
    let receiver_list = parameter_list(context, node.child_by_field_name("receiver"));
    let [(receiver, receiver_kind)] = receiver_list.as_slice() else {
        return None;
    };
    let receiver_type = receiver_kind.trim_start_matches('*');
    let receiver_type = receiver_type.split('[').next().unwrap_or(receiver_type);
    let name = context.text(node.child_by_field_name("name")?).to_string();
    let parameters = parameter_list(context, node.child_by_field_name("parameters"));
    let mut fact = MethodFact {
        receiver_type: (directory.to_string(), receiver_type.to_string()),
        name,
        parameters: parameters.iter().map(|(_, kind)| kind.clone()).collect(),
        calls: Vec::new(),
        called_functions: BTreeSet::new(),
        stored: BTreeSet::new(),
        returned_literals: Vec::new(),
    };
    let mut derived: HashMap<String, BTreeSet<usize>> = HashMap::new();
    for (position, (parameter, _)) in parameters.iter().enumerate() {
        if !parameter.is_empty() && parameter != "_" {
            derived.insert(parameter.clone(), BTreeSet::from([position]));
        }
    }
    let Some(body) = node.child_by_field_name("body") else {
        return Some(fact);
    };
    let mut pending = vec![body];
    while let Some(current) = pending.pop() {
        match current.kind() {
            "short_var_declaration" | "assignment_statement" => {
                let left = current
                    .child_by_field_name("left")
                    .map(named_children)
                    .unwrap_or_default();
                let right = current
                    .child_by_field_name("right")
                    .map(named_children)
                    .unwrap_or_default();
                let all: BTreeSet<usize> = right
                    .iter()
                    .flat_map(|value| mentions(context, *value, &derived))
                    .collect();
                for (position, target) in left.iter().enumerate() {
                    let value = if left.len() == right.len() {
                        mentions(context, right[position], &derived)
                    } else {
                        all.clone()
                    };
                    match target.kind() {
                        "identifier" => {
                            derived
                                .entry(context.text(*target).to_string())
                                .or_default()
                                .extend(value);
                        }
                        "selector_expression" => {
                            let on_receiver =
                                target
                                    .child_by_field_name("operand")
                                    .is_some_and(|operand| {
                                        operand.kind() == "identifier"
                                            && context.text(operand) == receiver
                                    });
                            if on_receiver {
                                fact.stored.extend(value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "range_clause" => {
                if let (Some(left), Some(right)) = (
                    current.child_by_field_name("left"),
                    current.child_by_field_name("right"),
                ) {
                    let value = mentions(context, right, &derived);
                    for target in named_children(left) {
                        derived
                            .entry(context.text(target).to_string())
                            .or_default()
                            .extend(value.iter().copied());
                    }
                }
            }
            "call_expression" => {
                if let Some(function) = current.child_by_field_name("function") {
                    let called = call_arguments(current);
                    if function.kind() == "identifier" && called.is_empty() {
                        let name = context.text(function);
                        if let Some(position) = parameters.iter().position(|(parameter, kind)| {
                            parameter == name
                                && kind.split_whitespace().collect::<String>() == "func()"
                        }) {
                            fact.called_functions.insert(position);
                        }
                    }
                    if let Some(target) = call_target(context, function, receiver) {
                        let arguments = called
                            .iter()
                            .map(|argument| argument_fact(context, *argument, &derived, receiver))
                            .collect();
                        fact.calls.push(CallFact { target, arguments });
                    }
                }
            }
            "return_statement" => {
                for value in current
                    .named_child(0)
                    .map(named_children)
                    .unwrap_or_default()
                {
                    let literal = if value.kind() == "unary_expression" {
                        value.child_by_field_name("operand").unwrap_or(value)
                    } else {
                        value
                    };
                    if literal.kind() != "composite_literal" {
                        continue;
                    }
                    let (Some(kind), Some(body)) = (
                        literal.child_by_field_name("type"),
                        literal.child_by_field_name("body"),
                    ) else {
                        continue;
                    };
                    if kind.kind() != "type_identifier" {
                        continue;
                    }
                    let mut elements = Vec::new();
                    for element in named_children(body) {
                        match element.kind() {
                            "literal_element" => {
                                let value = element.named_child(0).unwrap_or(element);
                                elements.push((
                                    None,
                                    argument_fact(context, value, &derived, receiver),
                                ));
                            }
                            "keyed_element" => {
                                let parts = named_children(element);
                                if let [key, value] = parts.as_slice() {
                                    let key = key.named_child(0).unwrap_or(*key);
                                    let value = value.named_child(0).unwrap_or(*value);
                                    elements.push((
                                        Some(context.text(key).to_string()),
                                        argument_fact(context, value, &derived, receiver),
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                    fact.returned_literals
                        .push((context.text(kind).to_string(), elements));
                }
            }
            "func_literal" => continue,
            _ => {}
        }
        let mut children = named_children(current);
        children.reverse();
        pending.extend(children);
    }
    Some(fact)
}

/// The router types and the combo types of the repository, from the facts of their packages.
fn classify(
    structs: &Structs,
    methods: &[MethodFact],
) -> (
    HashMap<TypeKey, RouterType>,
    Vec<ComboType>,
    HashSet<TypeKey>,
) {
    let mut by_type: HashMap<&TypeKey, Vec<&MethodFact>> = HashMap::new();
    for method in methods {
        by_type
            .entry(&method.receiver_type)
            .or_default()
            .push(method);
    }
    for list in by_type.values_mut() {
        list.sort_by(|left, right| left.name.cmp(&right.name));
    }
    let mut keys: Vec<&TypeKey> = by_type.keys().copied().collect();
    keys.sort();
    let mut router_types: HashMap<TypeKey, RouterType> = HashMap::new();
    let mut combos: Vec<ComboType> = Vec::new();
    let mut combo_types: HashSet<TypeKey> = HashSet::new();
    for key in keys {
        let Some(fields) = structs.get(key) else {
            continue;
        };
        let candidates: Vec<&MethodFact> = by_type[key]
            .iter()
            .copied()
            .filter(|method| ROUTE_METHODS.contains(&method.name.as_str()))
            .collect();
        let (mut shapes, inner) = forwards(key, fields, &candidates);
        let Some(inner) = inner else {
            continue;
        };
        for method in &candidates {
            if shapes.contains_key(&method.name) {
                continue;
            }
            if let Some(shape) = group_shape(method) {
                shapes.insert(method.name.clone(), shape);
                continue;
            }
            let combo = combo_shape(key, method, structs, &by_type, &shapes);
            if let Some((pattern, combo_key, combo)) = combo {
                combos.push(combo);
                combo_types.insert(combo_key);
                shapes.insert(
                    method.name.clone(),
                    Shape::Combo {
                        pattern,
                        combo: combos.len() - 1,
                    },
                );
            }
        }
        router_types.insert(
            key.clone(),
            RouterType {
                inner,
                methods: shapes,
            },
        );
    }
    (router_types, combos, combo_types)
}

/// Tells if a parameter type is a string.
fn is_string_type(kind: &str) -> bool {
    kind.trim() == "string"
}

/// The verbs of a forward, with the arguments of the call in place of the parameters of the
/// callee. None means that the call does not give a method.
fn translate(verbs: &[Verb], arguments: &[ArgumentFact], method: &MethodFact) -> Option<Vec<Verb>> {
    verbs
        .iter()
        .map(|verb| match verb {
            Verb::Fixed(text) => Some(Verb::Fixed(text.clone())),
            Verb::Any => Some(Verb::Any),
            Verb::Parameter(position) => {
                let argument = arguments.get(*position)?;
                if let Some(literal) = &argument.literal {
                    return Some(Verb::Fixed(literal.clone()));
                }
                let strings: Vec<usize> = argument
                    .parameters
                    .iter()
                    .copied()
                    .filter(|parameter| {
                        method
                            .parameters
                            .get(*parameter)
                            .is_some_and(|kind| is_string_type(kind))
                    })
                    .collect();
                match strings.as_slice() {
                    [only] => Some(Verb::Parameter(*only)),
                    _ => None,
                }
            }
        })
        .collect()
}

/// The route and mount shapes of the methods of a type, from the calls that send their pattern
/// to a framework router or to another route method. The result has the router of the first
/// forward to a framework.
fn forwards(
    key: &TypeKey,
    fields: &[(String, FieldKind)],
    methods: &[&MethodFact],
) -> (HashMap<String, Shape>, Option<Inner>) {
    let mut shapes: HashMap<String, Shape> = HashMap::new();
    let mut inner: Option<Inner> = None;
    loop {
        let mut changed = false;
        for method in methods {
            if shapes.contains_key(&method.name) {
                continue;
            }
            let mut found: Option<Shape> = None;
            for call in &method.calls {
                let (pattern_at, forward, framework) = match &call.target {
                    CallTarget::Field {
                        field,
                        method: name,
                    } => {
                        let router = fields.iter().find_map(|(known, kind)| match kind {
                            FieldKind::Router(router) if known == field => Some(*router),
                            _ => None,
                        });
                        let Some(router) = router else {
                            continue;
                        };
                        let Some((pattern_at, forward)) = router.registration(name) else {
                            continue;
                        };
                        (pattern_at, forward, Some(router))
                    }
                    CallTarget::Own(name) => match shapes.get(name) {
                        Some(Shape::Route { pattern, verbs }) => {
                            (*pattern, Forward::Verbs(verbs.clone()), None)
                        }
                        Some(Shape::Mount { pattern, mounted }) => {
                            (*pattern, Forward::Mount(*mounted), None)
                        }
                        _ => continue,
                    },
                };
                let Some(argument) = call.arguments.get(pattern_at) else {
                    continue;
                };
                let Some(pattern) = argument.parameters.iter().copied().find(|parameter| {
                    method
                        .parameters
                        .get(*parameter)
                        .is_some_and(|kind| is_string_type(kind))
                }) else {
                    continue;
                };
                let shape = match forward {
                    Forward::Verbs(verbs) => {
                        let Some(verbs) = translate(&verbs, &call.arguments, method) else {
                            continue;
                        };
                        Shape::Route { pattern, verbs }
                    }
                    Forward::Mount(at) => {
                        let mounted = call.arguments.get(at).and_then(|argument| {
                            argument.parameters.iter().copied().find(|parameter| {
                                method.parameters.get(*parameter).is_some_and(|kind| {
                                    kind.trim().trim_start_matches('*') == key.1
                                })
                            })
                        });
                        let Some(mounted) = mounted else {
                            continue;
                        };
                        Shape::Mount { pattern, mounted }
                    }
                };
                if inner.is_none() {
                    inner = framework;
                }
                found = Some(match (found, shape) {
                    (
                        Some(Shape::Route {
                            pattern: first,
                            verbs: mut known,
                        }),
                        Shape::Route { pattern, verbs },
                    ) if first == pattern => {
                        for verb in verbs {
                            if !known.contains(&verb) {
                                known.push(verb);
                            }
                        }
                        Shape::Route {
                            pattern: first,
                            verbs: known,
                        }
                    }
                    (Some(earlier), _) => earlier,
                    (None, shape) => shape,
                });
            }
            if let Some(shape) = found {
                shapes.insert(method.name.clone(), shape);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    (shapes, inner)
}

/// A group method calls its `func()` parameter and keeps its pattern in a field of the value.
fn group_shape(method: &MethodFact) -> Option<Shape> {
    let body = *method.called_functions.iter().next()?;
    let pattern = method.stored.iter().copied().find(|parameter| {
        method
            .parameters
            .get(*parameter)
            .is_some_and(|kind| is_string_type(kind))
    })?;
    Some(Shape::Group { pattern, body })
}

/// A combo method returns a literal of a struct type of the package that keeps the receiver and
/// the pattern. Each method of the struct type that sends the kept pattern to a route method
/// of the router type is a route method of the combo.
fn combo_shape(
    key: &TypeKey,
    method: &MethodFact,
    structs: &Structs,
    by_type: &HashMap<&TypeKey, Vec<&MethodFact>>,
    shapes: &HashMap<String, Shape>,
) -> Option<(usize, TypeKey, ComboType)> {
    for (type_name, elements) in &method.returned_literals {
        let combo_key = (key.0.clone(), type_name.clone());
        let Some(fields) = structs.get(&combo_key) else {
            continue;
        };
        let field_of = |position: usize, element_key: &Option<String>| -> Option<String> {
            match element_key {
                Some(name) => Some(name.clone()),
                None => fields.get(position).map(|(name, _)| name.clone()),
            }
        };
        let mut pattern: Option<(usize, String)> = None;
        let mut router_field: Option<String> = None;
        for (position, (element_key, argument)) in elements.iter().enumerate() {
            let Some(field) = field_of(position, element_key) else {
                continue;
            };
            if argument.receiver {
                router_field = Some(field.clone());
            }
            let strings: Vec<usize> = argument
                .parameters
                .iter()
                .copied()
                .filter(|parameter| {
                    method
                        .parameters
                        .get(*parameter)
                        .is_some_and(|kind| is_string_type(kind))
                })
                .collect();
            if let ([only], None) = (strings.as_slice(), &pattern) {
                pattern = Some((*only, field));
            }
        }
        let (Some((pattern, pattern_field)), Some(router_field)) = (pattern, router_field) else {
            continue;
        };
        let holds_the_router = fields.iter().any(|(name, kind)| {
            *name == router_field && matches!(kind, FieldKind::Repository(own) if *own == key.1)
        });
        if !holds_the_router {
            continue;
        }
        let mut methods: HashMap<String, Vec<Verb>> = HashMap::new();
        for combo_method in by_type.get(&combo_key).into_iter().flatten() {
            for call in &combo_method.calls {
                let CallTarget::Field {
                    field,
                    method: name,
                } = &call.target
                else {
                    continue;
                };
                if *field != router_field {
                    continue;
                }
                let Some(Shape::Route { pattern: at, verbs }) = shapes.get(name) else {
                    continue;
                };
                let sends_pattern = call
                    .arguments
                    .get(*at)
                    .and_then(|argument| argument.receiver_field.as_deref())
                    == Some(pattern_field.as_str());
                if !sends_pattern {
                    continue;
                }
                let translated: Option<Vec<Verb>> = verbs
                    .iter()
                    .map(|verb| match verb {
                        Verb::Parameter(position) => call
                            .arguments
                            .get(*position)?
                            .literal
                            .clone()
                            .map(Verb::Fixed),
                        other => Some(other.clone()),
                    })
                    .collect();
                if let Some(translated) = translated {
                    methods.insert(combo_method.name.clone(), translated);
                    break;
                }
            }
        }
        if !methods.is_empty() {
            return Some((pattern, combo_key, ComboType { methods }));
        }
    }
    None
}

/// The data that the walk of the functions uses.
struct Tables<'a> {
    files: &'a [FileSymbols],
    router_types: Vec<RouterType>,
    by_type: HashMap<TypeKey, usize>,
    combos: Vec<ComboType>,
    /// The types whose methods the walk does not read.
    skipped_types: HashSet<TypeKey>,
    /// The position of the file and of the symbol of each Go function and method, by key.
    symbols: HashMap<&'a str, (usize, usize)>,
}

impl Tables<'_> {
    /// The router type that the result of a function gives.
    fn result_type(&self, symbol: &Symbol, file: &FileSymbols) -> Option<usize> {
        let signature = parse_signature(&symbol.signature, &symbol.name)?;
        let key = resolve_type(file, &signature.result)?;
        self.by_type.get(&key).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Literal,
    Parameter,
    Result,
}

struct RouterNode {
    origin: Origin,
    router_type: usize,
}

struct Link {
    child: usize,
    parent: usize,
    /// A mount starts a new prefix with the rules of the framework. Another link joins text.
    mount: bool,
    /// None when the source does not give the prefix as a string literal.
    prefix: Option<String>,
    guards: Vec<String>,
}

struct Record {
    router: usize,
    file: usize,
    range: Span,
    /// The prefixes of the groups and the path, joined.
    segment: String,
    methods: Vec<String>,
    symbol: Option<String>,
    guards: Vec<String>,
}

#[derive(Default)]
struct Graph {
    routers: Vec<RouterNode>,
    links: Vec<Link>,
    records: Vec<Record>,
    /// The router of each parameter, by the key of the function and the position.
    parameters: HashMap<(String, usize), usize>,
    /// The calls of the functions that return a router: the key and the router of the result.
    calls: Vec<(String, usize)>,
    /// The router that each function returns. None when the returns do not agree.
    returns: HashMap<String, Option<usize>>,
}

impl Graph {
    fn add(&mut self, origin: Origin, router_type: usize) -> usize {
        self.routers.push(RouterNode {
            origin,
            router_type,
        });
        self.routers.len() - 1
    }

    fn parameter(&mut self, function: &str, position: usize, router_type: usize) -> usize {
        if let Some(found) = self.parameters.get(&(function.to_string(), position)) {
            return *found;
        }
        let id = self.add(Origin::Parameter, router_type);
        self.parameters.insert((function.to_string(), position), id);
        id
    }
}

#[derive(Clone)]
enum Value {
    Router(usize),
    Combo(Rc<ComboValue>),
    /// A function literal, by its key.
    Function(String),
}

struct ComboValue {
    router: usize,
    combo: usize,
    pattern: String,
    guards: Vec<String>,
}

enum Step<'t> {
    Read(Node<'t>),
    Function {
        key: String,
        parameters: Option<Node<'t>>,
        body: Node<'t>,
    },
    EndFunction,
    LeaveScope,
    PushGroup {
        router: usize,
        prefix: Option<String>,
        guards: Vec<String>,
    },
    PopGroup(usize),
}

struct Frame {
    key: String,
    returns: Vec<Option<usize>>,
}

struct Walker<'w, 'c, 'a> {
    context: &'c Context<'a>,
    tables: &'w Tables<'w>,
    graph: &'w mut Graph,
    file: usize,
    /// The position of the reference at each start of a name.
    references: HashMap<usize, usize>,
    scopes: Vec<HashMap<String, Option<Value>>>,
    frames: Vec<Frame>,
    /// The prefixes and the guards of the open groups of each router, from the outside.
    groups: HashMap<usize, Vec<OpenGroup>>,
    deferred: Vec<Step<'a>>,
    depth: usize,
}

impl<'w, 'c, 'a> Walker<'w, 'c, 'a> {
    fn text(&self, node: Node<'a>) -> &'a str {
        self.context.text(node)
    }

    fn run(&mut self, root: Node<'a>) {
        for declaration in named_children(root) {
            match declaration.kind() {
                "function_declaration" => {}
                "method_declaration" => {
                    let receiver =
                        parameter_list(self.context, declaration.child_by_field_name("receiver"));
                    let skipped = receiver.first().is_some_and(|(_, kind)| {
                        resolve_type(self.context.file, kind.split('[').next().unwrap_or(kind))
                            .is_some_and(|key| {
                                self.tables.by_type.contains_key(&key)
                                    || self.tables.skipped_types.contains(&key)
                            })
                    });
                    if skipped {
                        continue;
                    }
                }
                _ => continue,
            }
            let (Some(name), Some(body)) = (
                declaration.child_by_field_name("name"),
                declaration.child_by_field_name("body"),
            ) else {
                continue;
            };
            let key = self
                .context
                .symbol_at(name)
                .unwrap_or_else(|| self.local_key(declaration));
            self.walk(Step::Function {
                key,
                parameters: declaration.child_by_field_name("parameters"),
                body,
            });
        }
    }

    fn local_key(&self, node: Node<'a>) -> String {
        format!("{}@{}", self.context.file.file, node.start_byte())
    }

    fn walk(&mut self, first: Step<'a>) {
        let mut stack = vec![first];
        while let Some(step) = stack.pop() {
            match step {
                Step::Read(node) => self.read(node, &mut stack),
                Step::Function {
                    key,
                    parameters,
                    body,
                } => {
                    self.frames.push(Frame {
                        key: key.clone(),
                        returns: Vec::new(),
                    });
                    self.scopes.push(HashMap::new());
                    for (position, (name, kind)) in parameter_list(self.context, parameters)
                        .into_iter()
                        .enumerate()
                    {
                        let router = resolve_type(self.context.file, &kind)
                            .and_then(|key| self.tables.by_type.get(&key).copied())
                            .map(|router_type| {
                                Value::Router(self.graph.parameter(&key, position, router_type))
                            });
                        self.bind(&name, router, true);
                    }
                    stack.push(Step::EndFunction);
                    stack.push(Step::Read(body));
                }
                Step::EndFunction => {
                    self.scopes.pop();
                    if let Some(frame) = self.frames.pop() {
                        if !frame.returns.is_empty() {
                            let first = frame.returns[0];
                            let agreed = frame.returns.iter().all(|value| *value == first);
                            self.graph
                                .returns
                                .insert(frame.key, if agreed { first } else { None });
                        }
                    }
                }
                Step::LeaveScope => {
                    self.scopes.pop();
                }
                Step::PushGroup {
                    router,
                    prefix,
                    guards,
                } => self
                    .groups
                    .entry(router)
                    .or_default()
                    .push((prefix, guards)),
                Step::PopGroup(router) => {
                    if let Some(open) = self.groups.get_mut(&router) {
                        open.pop();
                    }
                }
            }
        }
    }

    fn read(&mut self, node: Node<'a>, stack: &mut Vec<Step<'a>>) {
        match node.kind() {
            "block"
            | "if_statement"
            | "for_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
            | "expression_case"
            | "default_case"
            | "type_case"
            | "communication_case" => {
                self.scopes.push(HashMap::new());
                stack.push(Step::LeaveScope);
                let mut children = named_children(node);
                children.reverse();
                stack.extend(children.into_iter().map(Step::Read));
                return;
            }
            "short_var_declaration" => self.binding(node, true),
            "assignment_statement" => {
                let plain = node
                    .child_by_field_name("operator")
                    .is_some_and(|operator| self.text(operator) == "=");
                if plain {
                    self.binding(node, false);
                } else if let Some(right) = node.child_by_field_name("right") {
                    self.eval(right);
                }
            }
            "var_spec" => {
                let mut cursor = node.walk();
                let names: Vec<Node<'a>> =
                    node.children_by_field_name("name", &mut cursor).collect();
                let values = node
                    .child_by_field_name("value")
                    .map(named_children)
                    .unwrap_or_default();
                self.bind_values(&names, &values, true);
            }
            "return_statement" => {
                let values = node.named_child(0).map(named_children).unwrap_or_default();
                let mut first = None;
                for (position, value) in values.into_iter().enumerate() {
                    let result = self.eval(value);
                    if position == 0 {
                        first = match result {
                            Some(Value::Router(router)) => Some(router),
                            _ => None,
                        };
                    }
                }
                if let Some(frame) = self.frames.last_mut() {
                    frame.returns.push(first);
                }
            }
            "function_declaration" | "method_declaration" => {}
            "call_expression"
            | "func_literal"
            | "expression_statement"
            | "go_statement"
            | "defer_statement" => {
                let expression = if matches!(node.kind(), "call_expression" | "func_literal") {
                    Some(node)
                } else {
                    node.named_child(0)
                };
                if let Some(expression) = expression {
                    self.eval(expression);
                }
            }
            _ => {
                let mut children = named_children(node);
                children.reverse();
                stack.extend(children.into_iter().map(Step::Read));
                return;
            }
        }
        let deferred = std::mem::take(&mut self.deferred);
        stack.extend(deferred.into_iter().rev());
    }

    fn binding(&mut self, node: Node<'a>, define: bool) {
        let names = node
            .child_by_field_name("left")
            .map(named_children)
            .unwrap_or_default();
        let values = node
            .child_by_field_name("right")
            .map(named_children)
            .unwrap_or_default();
        self.bind_values(&names, &values, define);
    }

    fn bind_values(&mut self, names: &[Node<'a>], values: &[Node<'a>], define: bool) {
        let results: Vec<Option<Value>> = values.iter().map(|value| self.eval(*value)).collect();
        for (position, name) in names.iter().enumerate() {
            if name.kind() != "identifier" {
                continue;
            }
            let value = if names.len() == results.len() {
                results[position].clone()
            } else if position == 0 && results.len() == 1 {
                results[0].clone()
            } else {
                None
            };
            let text = self.text(*name).to_string();
            self.bind(&text, value, define);
        }
    }

    fn bind(&mut self, name: &str, value: Option<Value>, define: bool) {
        if name.is_empty() || name == "_" {
            return;
        }
        if !define {
            if let Some(scope) = self
                .scopes
                .iter_mut()
                .rev()
                .find(|scope| scope.contains_key(name))
            {
                scope.insert(name.to_string(), value);
            }
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }

    /// The value of a local name. None when no local binding has the name.
    fn lookup(&self, name: &str) -> Option<Option<Value>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn eval(&mut self, node: Node<'a>) -> Option<Value> {
        if self.depth >= EVAL_DEPTH_LIMIT {
            return None;
        }
        self.depth += 1;
        let value = self.eval_inner(node);
        self.depth -= 1;
        value
    }

    fn eval_inner(&mut self, node: Node<'a>) -> Option<Value> {
        match node.kind() {
            "identifier" => self.lookup(self.text(node)).flatten(),
            "parenthesized_expression" | "variadic_argument" => self.eval(node.named_child(0)?),
            "unary_expression" => {
                let operand = node.child_by_field_name("operand")?;
                let value = self.eval(operand);
                let address = node
                    .child_by_field_name("operator")
                    .is_some_and(|operator| self.text(operator) == "&");
                if address {
                    value
                } else {
                    None
                }
            }
            "composite_literal" => {
                if let Some(body) = node.child_by_field_name("body") {
                    self.eval_elements(body);
                }
                let kind = node.child_by_field_name("type")?;
                let key = resolve_type(self.context.file, self.text(kind))?;
                let router_type = *self.tables.by_type.get(&key)?;
                Some(Value::Router(self.graph.add(Origin::Literal, router_type)))
            }
            "call_expression" => self.eval_call(node),
            "func_literal" => {
                let key = self.local_key(node);
                if let Some(body) = node.child_by_field_name("body") {
                    self.deferred.push(Step::Function {
                        key: key.clone(),
                        parameters: node.child_by_field_name("parameters"),
                        body,
                    });
                }
                Some(Value::Function(key))
            }
            "selector_expression" => {
                self.eval(node.child_by_field_name("operand")?);
                None
            }
            _ => {
                for child in named_children(node) {
                    self.eval(child);
                }
                None
            }
        }
    }

    fn eval_elements(&mut self, body: Node<'a>) {
        let mut pending = vec![body];
        while let Some(current) = pending.pop() {
            match current.kind() {
                "literal_value" | "literal_element" | "keyed_element" => {
                    pending.extend(named_children(current));
                }
                _ => {
                    self.eval(current);
                }
            }
        }
    }

    fn eval_call(&mut self, call: Node<'a>) -> Option<Value> {
        let function = call.child_by_field_name("function")?;
        let arguments = call_arguments(call);
        if function.kind() == "selector_expression" {
            let operand = function.child_by_field_name("operand")?;
            let name = self.text(function.child_by_field_name("field")?);
            match self.eval(operand) {
                Some(Value::Router(router)) => {
                    let router_type = self.graph.routers[router].router_type;
                    if let Some(shape) = self.tables.router_types[router_type].methods.get(name) {
                        let shape = shape.clone();
                        return self.registration(router, &shape, call, &arguments);
                    }
                }
                Some(Value::Combo(combo)) => {
                    let verbs = self.tables.combos[combo.combo].methods.get(name).cloned();
                    if let Some(verbs) = verbs {
                        self.combo_route(&combo, &verbs, call, &arguments);
                        return Some(Value::Combo(combo));
                    }
                }
                _ => {}
            }
        }
        let callee = self.callee(function);
        let values: Vec<Option<Value>> = arguments
            .iter()
            .map(|argument| self.eval(*argument))
            .collect();
        let (key, result) = callee?;
        for (position, value) in values.into_iter().enumerate() {
            if let Some(Value::Router(router)) = value {
                let router_type = self.graph.routers[router].router_type;
                let parameter = self.graph.parameter(&key, position, router_type);
                let (prefix, guards) = self.group_of(router);
                self.graph.links.push(Link {
                    child: parameter,
                    parent: router,
                    mount: false,
                    prefix,
                    guards,
                });
            }
        }
        let router_type = result?;
        let router = self.graph.add(Origin::Result, router_type);
        self.graph.calls.push((key, router));
        Some(Value::Router(router))
    }

    /// The function that a call names, and the router type of its result.
    fn callee(&self, function: Node<'a>) -> Option<(String, Option<usize>)> {
        if function.kind() == "identifier" {
            if let Some(local) = self.lookup(self.text(function)) {
                return match local {
                    Some(Value::Function(key)) => Some((key, None)),
                    _ => None,
                };
            }
        }
        if let Some((symbol, file)) = self.context.go_handler_target(function) {
            return Some((
                symbol.key.to_string(),
                self.tables.result_type(symbol, file),
            ));
        }
        if function.kind() != "selector_expression" {
            return None;
        }
        let field = function.child_by_field_name("field")?;
        let reference = &self.context.file.references[*self.references.get(&field.start_byte())?];
        let resolved = matches!(
            reference.resolution.status,
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
        );
        let [key] = reference.resolution.candidates.as_slice() else {
            return None;
        };
        if !resolved {
            return None;
        }
        let (file, position) = *self.tables.symbols.get(&**key)?;
        let file = &self.tables.files[file];
        let symbol = &file.symbols[position];
        Some((key.to_string(), self.tables.result_type(symbol, file)))
    }

    /// The prefix and the guards of the open groups of a router.
    fn group_of(&self, router: usize) -> (Option<String>, Vec<String>) {
        let mut prefix = Some(String::new());
        let mut guards = Vec::new();
        for (part, names) in self.groups.get(&router).into_iter().flatten() {
            prefix = prefix
                .zip(part.as_deref())
                .map(|(known, part)| known + part);
            guards.extend(names.iter().cloned());
        }
        (prefix, guards)
    }

    fn string(&self, node: Option<&Node<'a>>) -> Option<String> {
        self.context.go_string(*node?)
    }

    /// The arguments with a final spread of `append(list, a, b)` in place of `a, b`. The bool
    /// is false when the arguments have a spread that the walk cannot read.
    fn flatten(&self, arguments: Vec<Node<'a>>) -> (Vec<Node<'a>>, bool) {
        let mut flat = arguments;
        let Some(last) = flat.last().copied() else {
            return (flat, true);
        };
        if last.kind() != "variadic_argument" {
            return (flat, true);
        }
        flat.pop();
        let Some(inner) = last.named_child(0) else {
            return (flat, false);
        };
        let is_append = inner.kind() == "call_expression"
            && inner
                .child_by_field_name("function")
                .is_some_and(|function| {
                    function.kind() == "identifier"
                        && self.text(function) == "append"
                        && !self.context.is_go_shadowed(function)
                });
        if !is_append {
            return (flat, false);
        }
        let mut items = call_arguments(inner);
        if items.is_empty()
            || items
                .last()
                .is_some_and(|item| item.kind() == "variadic_argument")
        {
            return (flat, false);
        }
        items.remove(0);
        flat.extend(items);
        (flat, true)
    }

    /// The name of a guard, as `auth`, `context.ReqToken`, or the function of a call.
    fn guard_name(&self, node: Node<'a>) -> Option<String> {
        match node.kind() {
            "identifier" | "selector_expression" => Some(self.text(node).to_string()),
            "call_expression" => self.guard_name(node.child_by_field_name("function")?),
            "index_expression" => self.guard_name(node.child_by_field_name("operand")?),
            "parenthesized_expression" => self.guard_name(node.named_child(0)?),
            _ => None,
        }
    }

    /// The handler and the guards of a list of handler arguments: the last is the handler.
    fn handler(&mut self, handlers: Vec<Node<'a>>) -> Option<(Option<String>, Vec<String>)> {
        for handler in &handlers {
            self.eval(*handler);
        }
        let (flat, complete) = self.flatten(handlers);
        if !complete {
            let guards = flat
                .iter()
                .filter_map(|node| self.guard_name(*node))
                .collect();
            return Some((None, guards));
        }
        let (last, before) = flat.split_last()?;
        let guards = before
            .iter()
            .filter_map(|node| self.guard_name(*node))
            .collect();
        Some((self.context.go_handler_symbol(*last), guards))
    }

    /// The names of the guards of a list of arguments.
    fn guard_names(&mut self, nodes: Vec<Node<'a>>) -> Vec<String> {
        for node in &nodes {
            self.eval(*node);
        }
        let (flat, _) = self.flatten(nodes);
        flat.into_iter()
            .filter_map(|node| self.guard_name(node))
            .collect()
    }

    fn expand(&self, verbs: &[Verb], arguments: &[Node<'a>]) -> Option<Vec<String>> {
        let mut methods = Vec::new();
        for verb in verbs {
            let text = match verb {
                Verb::Any => {
                    methods.push("ANY".to_string());
                    continue;
                }
                Verb::Fixed(text) => text.clone(),
                Verb::Parameter(position) => self.string(arguments.get(*position))?,
            };
            for part in text.split(',') {
                let part = part.trim();
                if !is_http_token(part) {
                    return None;
                }
                methods.push(part.to_string());
            }
        }
        methods.sort();
        methods.dedup();
        Some(methods)
    }

    fn registration(
        &mut self,
        router: usize,
        shape: &Shape,
        call: Node<'a>,
        arguments: &[Node<'a>],
    ) -> Option<Value> {
        match shape {
            Shape::Route { pattern, verbs } => {
                let excluded: HashSet<usize> = std::iter::once(*pattern)
                    .chain(verbs.iter().filter_map(|verb| match verb {
                        Verb::Parameter(position) => Some(*position),
                        _ => None,
                    }))
                    .collect();
                let handlers: Vec<Node<'a>> = arguments
                    .iter()
                    .enumerate()
                    .filter(|(position, _)| !excluded.contains(position))
                    .map(|(_, node)| *node)
                    .collect();
                let path = self.string(arguments.get(*pattern));
                let methods = self.expand(verbs, arguments);
                let found = self.handler(handlers);
                let (Some(path), Some(methods), Some((symbol, guards))) = (path, methods, found)
                else {
                    return None;
                };
                self.record(router, &path, methods, symbol, guards, Span::of(&call));
                None
            }
            Shape::Mount { pattern, mounted } => {
                let mounted_value = arguments.get(*mounted).and_then(|node| self.eval(*node));
                let prefix = self.string(arguments.get(*pattern));
                if let Some(Value::Router(child)) = mounted_value {
                    let (open, guards) = self.group_of(router);
                    self.graph.links.push(Link {
                        child,
                        parent: router,
                        mount: true,
                        prefix: open.zip(prefix).map(|(open, prefix)| open + &prefix),
                        guards,
                    });
                }
                None
            }
            Shape::Group { pattern, body } => {
                let others: Vec<Node<'a>> = arguments
                    .iter()
                    .enumerate()
                    .filter(|(position, _)| position != pattern && position != body)
                    .map(|(_, node)| *node)
                    .collect();
                let guards = self.guard_names(others);
                let prefix = self.string(arguments.get(*pattern));
                let literal = arguments
                    .get(*body)
                    .copied()
                    .filter(|node| node.kind() == "func_literal");
                match literal {
                    Some(literal) => {
                        let body = literal.child_by_field_name("body")?;
                        self.deferred.push(Step::PushGroup {
                            router,
                            prefix,
                            guards,
                        });
                        self.deferred.push(Step::Function {
                            key: self.local_key(literal),
                            parameters: literal.child_by_field_name("parameters"),
                            body,
                        });
                        self.deferred.push(Step::PopGroup(router));
                    }
                    None => {
                        if let Some(node) = arguments.get(*body) {
                            self.eval(*node);
                        }
                    }
                }
                None
            }
            Shape::Combo { pattern, combo } => {
                let others: Vec<Node<'a>> = arguments
                    .iter()
                    .enumerate()
                    .filter(|(position, _)| position != pattern)
                    .map(|(_, node)| *node)
                    .collect();
                let guards = self.guard_names(others);
                let pattern = self.string(arguments.get(*pattern))?;
                Some(Value::Combo(Rc::new(ComboValue {
                    router,
                    combo: *combo,
                    pattern,
                    guards,
                })))
            }
        }
    }

    fn combo_route(
        &mut self,
        combo: &ComboValue,
        verbs: &[Verb],
        call: Node<'a>,
        arguments: &[Node<'a>],
    ) {
        let methods = self.expand(verbs, arguments);
        let found = self.handler(arguments.to_vec());
        let (Some(methods), Some((symbol, own))) = (methods, found) else {
            return;
        };
        let mut guards = combo.guards.clone();
        guards.extend(own);
        let range = call
            .child_by_field_name("function")
            .and_then(|function| function.child_by_field_name("field"))
            .map_or_else(|| Span::of(&call), |field| Span::between(&field, &call));
        self.record(combo.router, &combo.pattern, methods, symbol, guards, range);
    }

    fn record(
        &mut self,
        router: usize,
        path: &str,
        methods: Vec<String>,
        symbol: Option<String>,
        guards: Vec<String>,
        range: Span,
    ) {
        let (prefix, mut all) = self.group_of(router);
        // A group whose prefix the walk cannot read hides the path of the route.
        let Some(prefix) = prefix else {
            return;
        };
        all.extend(guards);
        self.graph.records.push(Record {
            router,
            file: self.file,
            range,
            segment: prefix + path,
            methods,
            symbol,
            guards: all,
        });
    }
}

/// One chain of links from a router up to a root.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Placement {
    mounts: Vec<String>,
    open: String,
    guards: Vec<String>,
}

#[derive(Clone)]
enum Placements {
    /// The router is in a cycle of links, or it has too many chains.
    Refused,
    Known(Rc<Vec<Placement>>),
}

/// The text with a leading slash.
fn rooted(text: &str) -> String {
    if text.starts_with('/') {
        text.to_string()
    } else {
        format!("/{text}")
    }
}

impl WrapperRoutes {
    pub(super) fn build(files: &[FileSymbols], trees: &dyn TreeSource) -> Self {
        // The packages with a method that has the name of a route method.
        let candidate_directories: HashSet<&str> = files
            .iter()
            .filter(|file| is_source(file))
            .filter(|file| {
                file.symbols.iter().any(|symbol| {
                    symbol.kind == SymbolKind::Method
                        && symbol.receiver_type.is_some()
                        && ROUTE_METHODS.contains(&symbol.name.as_str())
                })
            })
            .map(|file| directory_of(&file.file))
            .collect();
        if candidate_directories.is_empty() {
            return Self::default();
        }
        let mut structs: Structs = HashMap::new();
        let mut methods: Vec<MethodFact> = Vec::new();
        for (position, file) in files.iter().enumerate() {
            if !is_source(file) || !candidate_directories.contains(directory_of(&file.file)) {
                continue;
            }
            let Some(tree) = trees.tree(position) else {
                continue;
            };
            let context = Context::new(&tree, file, files, trees, &[]);
            let facts = file_facts(&context, tree.root_node());
            structs.extend(facts.structs);
            methods.extend(facts.methods);
        }
        let (router_types, combos, combo_types) = classify(&structs, &methods);
        if router_types.is_empty() {
            return Self::default();
        }
        let mut keys: Vec<TypeKey> = router_types.keys().cloned().collect();
        keys.sort();
        let mut by_type: HashMap<TypeKey, usize> = HashMap::new();
        let mut ordered: Vec<RouterType> = Vec::new();
        let mut router_types = router_types;
        for key in keys {
            if let Some(router_type) = router_types.remove(&key) {
                by_type.insert(key, ordered.len());
                ordered.push(router_type);
            }
        }
        let mut symbols: HashMap<&str, (usize, usize)> = HashMap::new();
        for (file_position, file) in files.iter().enumerate() {
            if !is_source(file) {
                continue;
            }
            for (position, symbol) in file.symbols.iter().enumerate() {
                if symbol.is_callable() {
                    symbols.insert(&symbol.key, (file_position, position));
                }
            }
        }
        let tables = Tables {
            files,
            router_types: ordered,
            by_type,
            combos,
            skipped_types: combo_types,
            symbols,
        };
        // The packages whose functions take or return a router type, and the packages of the types.
        let mut directories: HashSet<String> = tables
            .by_type
            .keys()
            .map(|(directory, _)| directory.clone())
            .collect();
        for file in files.iter().filter(|file| is_source(file)) {
            let uses_router = file
                .symbols
                .iter()
                .filter(|symbol| symbol.is_callable())
                .any(|symbol| {
                    parse_signature(&symbol.signature, &symbol.name).is_some_and(|signature| {
                        signature
                            .parameters
                            .iter()
                            .chain([&signature.result])
                            .filter_map(|kind| resolve_type(file, kind.trim_start_matches("...")))
                            .any(|key| tables.by_type.contains_key(&key))
                    })
                });
            if uses_router {
                directories.insert(directory_of(&file.file).to_string());
            }
        }
        let mut graph = Graph::default();
        for (position, file) in files.iter().enumerate() {
            if !is_source(file) {
                continue;
            }
            let relevant = directories.contains(directory_of(&file.file))
                || file.imports.iter().any(|binding| {
                    let target = match &binding.target_file {
                        ImportTarget::Resolved(target) => Some(target.as_str()),
                        ImportTarget::Package(targets) => targets.first().map(String::as_str),
                        _ => None,
                    };
                    target.is_some_and(|target| directories.contains(directory_of(target)))
                });
            if !relevant {
                continue;
            }
            let Some(tree) = trees.tree(position) else {
                continue;
            };
            let context = Context::new(&tree, file, files, trees, &[]);
            let references = file
                .references
                .iter()
                .enumerate()
                .map(|(index, reference)| (reference.name_range.start_byte, index))
                .collect();
            let mut walker = Walker {
                context: &context,
                tables: &tables,
                graph: &mut graph,
                file: position,
                references,
                scopes: Vec::new(),
                frames: Vec::new(),
                groups: HashMap::new(),
                deferred: Vec::new(),
                depth: 0,
            };
            walker.run(tree.root_node());
        }
        Self::finish(&tables, graph)
    }

    fn finish(tables: &Tables<'_>, mut graph: Graph) -> Self {
        // The router that a function returns is the router of each call of the function.
        let mut returned_as: HashMap<usize, Vec<usize>> = HashMap::new();
        for (key, call) in std::mem::take(&mut graph.calls) {
            if let Some(Some(returned)) = graph.returns.get(&key) {
                graph.links.push(Link {
                    child: *returned,
                    parent: call,
                    mount: false,
                    prefix: Some(String::new()),
                    guards: Vec::new(),
                });
                returned_as.entry(*returned).or_default().push(call);
            }
        }
        // A literal is a root when no link attaches it. So is the result of a call that returns a
        // literal, and the result of a call that returns such a result.
        let count = graph.routers.len();
        let mut can_be_root = vec![false; count];
        let mut queue: VecDeque<usize> = (0..count)
            .filter(|router| graph.routers[*router].origin == Origin::Literal)
            .collect();
        for router in &queue {
            can_be_root[*router] = true;
        }
        while let Some(router) = queue.pop_front() {
            for call in returned_as.get(&router).into_iter().flatten() {
                if !can_be_root[*call] {
                    can_be_root[*call] = true;
                    queue.push_back(*call);
                }
            }
        }
        let mut parents: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (index, link) in graph.links.iter().enumerate() {
            parents[link.child].push(index);
        }
        let mut network: petgraph::graph::DiGraph<(), ()> =
            petgraph::graph::DiGraph::with_capacity(count, graph.links.len());
        for _ in 0..count {
            network.add_node(());
        }
        for link in &graph.links {
            network.add_edge(
                petgraph::graph::NodeIndex::new(link.child),
                petgraph::graph::NodeIndex::new(link.parent),
                (),
            );
        }
        let mut placements: Vec<Option<Placements>> = vec![None; count];
        // The components come with each parent before its children.
        for component in petgraph::algo::tarjan_scc(&network) {
            let cyclic = component.len() > 1
                || component.first().is_some_and(|node| {
                    parents[node.index()]
                        .iter()
                        .any(|link| graph.links[*link].parent == node.index())
                });
            for node in component {
                let router = node.index();
                if cyclic {
                    placements[router] = Some(Placements::Refused);
                    continue;
                }
                placements[router] = Some(Self::place(
                    &graph,
                    &parents[router],
                    &placements,
                    can_be_root[router],
                ));
            }
        }
        let mut by_file: HashMap<String, Vec<Found>> = HashMap::new();
        for record in &graph.records {
            let Some(Placements::Known(known)) = &placements[record.router] else {
                continue;
            };
            let inner = tables.router_types[graph.routers[record.router].router_type].inner;
            let file = &tables.files[record.file].file;
            for placement in known.iter() {
                let own = rooted(&format!("{}{}", placement.open, record.segment));
                let Some(composed) = inner.compose(&placement.mounts, &own) else {
                    continue;
                };
                let mut guards = placement.guards.clone();
                guards.extend(record.guards.iter().cloned());
                for method in &record.methods {
                    let (method, path, parameters) = match inner {
                        Inner::Framework(framework) => (
                            method.clone(),
                            composed.clone(),
                            path_parameters(framework, &composed),
                        ),
                        Inner::ServeMux => {
                            let Some((pattern_method, path)) = servemux_pattern(&composed) else {
                                continue;
                            };
                            let method = if method == "ANY" {
                                pattern_method
                            } else {
                                method.clone()
                            };
                            let parameters = servemux_parameters(&path);
                            (method, path, parameters)
                        }
                    };
                    by_file.entry(file.clone()).or_default().push(Found {
                        method,
                        path,
                        guards: guards.clone(),
                        symbol: record.symbol.clone(),
                        framework: inner.name(),
                        parameters,
                        range: record.range,
                    });
                }
            }
        }
        for found in by_file.values_mut() {
            found.sort();
            found.dedup();
        }
        Self { by_file }
    }

    /// The chains of a router, from the chains of its parents.
    fn place(
        graph: &Graph,
        links: &[usize],
        placements: &[Option<Placements>],
        can_be_root: bool,
    ) -> Placements {
        if links.is_empty() {
            return if can_be_root {
                Placements::Known(Rc::new(vec![Placement {
                    mounts: Vec::new(),
                    open: String::new(),
                    guards: Vec::new(),
                }]))
            } else {
                Placements::Known(Rc::new(Vec::new()))
            };
        }
        let mut found: BTreeSet<Placement> = BTreeSet::new();
        for index in links {
            let link = &graph.links[*index];
            let (Some(Placements::Known(above)), Some(prefix)) =
                (&placements[link.parent], &link.prefix)
            else {
                return Placements::Refused;
            };
            for placement in above.iter() {
                let mut guards = placement.guards.clone();
                guards.extend(link.guards.iter().cloned());
                let next = if link.mount {
                    let mut mounts = placement.mounts.clone();
                    mounts.push(rooted(&format!("{}{prefix}", placement.open)));
                    Placement {
                        mounts,
                        open: String::new(),
                        guards,
                    }
                } else {
                    Placement {
                        mounts: placement.mounts.clone(),
                        open: format!("{}{prefix}", placement.open),
                        guards,
                    }
                };
                found.insert(next);
                if found.len() > PLACEMENT_LIMIT {
                    return Placements::Refused;
                }
            }
        }
        Placements::Known(Rc::new(found.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{assert_scales_linearly, entries, entries_timed};
    use super::super::EntryKind;
    use crate::core::Language;

    /// Each route as `framework METHOD path [guards] handler`, in order.
    fn routes(files: &[(&str, &str)]) -> Vec<String> {
        let mut found: Vec<String> = entries(Language::Go, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| {
                let route = entry.route.expect("a route");
                format!(
                    "{} {} {} [{}] {}",
                    entry.framework,
                    route.method,
                    route.path,
                    entry.guards.join(","),
                    entry
                        .symbol
                        .as_deref()
                        .map_or("-", |symbol| symbol.rsplit('#').next().unwrap_or(symbol))
                )
            })
            .collect();
        found.sort();
        found
    }

    const PROBE_ROUTER: &str = r#"package web

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

type Router struct{ chiRouter chi.Router }

func NewRouter() *Router {
	return &Router{chiRouter: chi.NewRouter()}
}

func (r *Router) Get(pattern string, h ...any) {
	r.chiRouter.Get(pattern, h[0].(http.HandlerFunc))
}
"#;

    const PROBE_MAIN: &str = r#"package main

import (
	"net/http"

	"example.com/app/web"
	"github.com/go-chi/chi/v5"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func mw(next http.Handler) http.Handler { return next }

func reqSignIn(w http.ResponseWriter, r *http.Request) {}

func main() {
	c := chi.NewRouter()
	c.With(mw).Get("/direct", Home)
	m := web.NewRouter()
	m.Get("/wrapped", reqSignIn, Home)
	http.ListenAndServe(":80", c)
}
"#;

    #[test]
    fn a_route_on_a_router_type_of_the_repository_has_its_handler_and_its_guards() {
        assert_eq!(
            routes(&[("main.go", PROBE_MAIN), ("web/router.go", PROBE_ROUTER)]),
            [
                "chi GET /direct [mw] Home",
                "chi GET /wrapped [reqSignIn] Home",
            ]
        );
    }

    /// A router type in the shape of the router of Gitea: a group keeps its prefix in a field,
    /// `Methods` takes a list of methods, and `Combo` keeps a pattern for a chain of methods.
    const ROUTER: &str = r#"package web

import (
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
)

type Router struct {
	chiRouter      *chi.Mux
	curGroupPrefix string
	curMiddlewares []any
}

func NewRouter() *Router {
	r := chi.NewRouter()
	return &Router{chiRouter: r}
}

func (r *Router) Group(pattern string, fn func(), middlewares ...any) {
	previousGroupPrefix := r.curGroupPrefix
	previousMiddlewares := r.curMiddlewares
	r.curGroupPrefix += pattern
	r.curMiddlewares = append(r.curMiddlewares, middlewares...)
	fn()
	r.curGroupPrefix = previousGroupPrefix
	r.curMiddlewares = previousMiddlewares
}

func (r *Router) getPattern(pattern string) string {
	newPattern := r.curGroupPrefix + pattern
	if !strings.HasPrefix(newPattern, "/") {
		newPattern = "/" + newPattern
	}
	return newPattern
}

func toHandler(h []any) http.HandlerFunc { return nil }

func (r *Router) Methods(methods, pattern string, h ...any) {
	fullPattern := r.getPattern(pattern)
	if strings.Contains(methods, ",") {
		for method := range strings.SplitSeq(methods, ",") {
			r.chiRouter.With().Method(strings.TrimSpace(method), fullPattern, toHandler(h))
		}
	} else {
		r.chiRouter.With().Method(methods, fullPattern, toHandler(h))
	}
}

func (r *Router) Mount(pattern string, subRouter *Router) {
	r.chiRouter.Mount(r.getPattern(pattern), subRouter.chiRouter)
}

func (r *Router) Any(pattern string, h ...any) {
	r.chiRouter.HandleFunc(r.getPattern(pattern), toHandler(h))
}

func (r *Router) Get(pattern string, h ...any) { r.Methods("GET", pattern, h...) }

func (r *Router) Post(pattern string, h ...any) { r.Methods("POST", pattern, h...) }

func (r *Router) Combo(pattern string, h ...any) *Combo {
	return &Combo{r, pattern, h}
}

type Combo struct {
	r       *Router
	pattern string
	h       []any
}

func (c *Combo) Get(h ...any) *Combo {
	c.r.Get(c.pattern, append(c.h, h...)...)
	return c
}

func (c *Combo) Post(h ...any) *Combo {
	c.r.Post(c.pattern, append(c.h, h...)...)
	return c
}
"#;

    const API: &str = r#"package api

import (
	"example.com/app/context"
	"example.com/app/web"
)

func reqToken(ctx *context.Context) {}

func reqAdmin(ctx *context.Context) {}

func loadUser(ctx *context.Context) {}

func ListUsers(ctx *context.Context) {}

func GetUser(ctx *context.Context) {}

func CreateUser(ctx *context.Context) {}

func Version(ctx *context.Context) {}

func addUserRoutes(m *web.Router) {
	m.Get("", ListUsers)
	m.Combo("/{id}", loadUser).
		Get(GetUser).
		Post(reqAdmin, CreateUser)
}

func Routes() *web.Router {
	m := web.NewRouter()
	m.Group("/v1", func() {
		m.Get("/version", Version)
		m.Group("/users", func() {
			addUserRoutes(m)
		}, reqToken)
		m.Methods("GET, HEAD", "/ping", Version)
	})
	return m
}
"#;

    const CONTEXT: &str = "package context\n\ntype Context struct{}\n";

    const MAIN: &str = r#"package main

import (
	"net/http"

	"example.com/app/api"
	"example.com/app/web"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func NormalRoutes() *web.Router {
	r := web.NewRouter()
	r.Mount("/api", api.Routes())
	r.Any("/", Home)
	return r
}

func main() {
	http.ListenAndServe(":80", NormalRoutes())
}
"#;

    fn gitea_shape(extra: &[(&'static str, &'static str)]) -> Vec<String> {
        let mut files = vec![
            ("api/api.go", API),
            ("context/context.go", CONTEXT),
            ("main.go", MAIN),
            ("web/router.go", ROUTER),
        ];
        files.extend_from_slice(extra);
        routes(&files)
    }

    #[test]
    fn groups_combos_methods_and_mounts_of_a_router_type_give_full_paths() {
        assert_eq!(
            gitea_shape(&[]),
            [
                "chi ANY / [] Home",
                "chi GET /api/v1/ping [] Version",
                "chi GET /api/v1/users [reqToken] ListUsers",
                "chi GET /api/v1/users/{id} [reqToken,loadUser] GetUser",
                "chi GET /api/v1/version [] Version",
                "chi HEAD /api/v1/ping [] Version",
                "chi POST /api/v1/users/{id} [reqToken,loadUser,reqAdmin] CreateUser",
            ]
        );
    }

    #[test]
    fn a_type_whose_method_does_not_send_its_pattern_to_a_router_is_not_a_router() {
        let store = r#"package store

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

type Cache struct{ data map[string]any }

func (c *Cache) Get(key string, fallback ...any) any { return c.data[key] }

type Fixed struct{ router chi.Router }

func (s *Fixed) Get(key string, h ...any) { s.router.Get("/fixed", h[0].(http.HandlerFunc)) }
"#;
        let main = r#"package main

import (
	"net/http"

	"example.com/app/store"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func main() {
	c := &store.Cache{}
	c.Get("/cache", Home)
	f := &store.Fixed{}
	f.Get("/fixed", Home)
}
"#;
        assert!(routes(&[("main.go", main), ("store/store.go", store)]).is_empty());
    }

    #[test]
    fn a_router_parameter_gets_the_path_of_each_call_and_none_without_a_call() {
        let main = r#"package main

import (
	"net/http"

	"example.com/app/web"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func addHome(m *web.Router) {
	m.Get("/home", Home)
}

func orphan(m *web.Router) {
	m.Get("/orphan", Home)
}

func main() {
	m := web.NewRouter()
	m.Group("/a", func() {
		addHome(m)
	})
	m.Group("/b", func() {
		addHome(m)
	})
}
"#;
        assert_eq!(
            routes(&[("main.go", main), ("web/router.go", ROUTER)]),
            ["chi GET /a/home [] Home", "chi GET /b/home [] Home"]
        );
    }

    #[test]
    fn each_call_of_a_constructor_makes_its_own_router() {
        let main = r#"package main

import (
	"net/http"

	"example.com/app/web"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func main() {
	a := web.NewRouter()
	b := web.NewRouter()
	a.Get("/a", Home)
	b.Get("/b", Home)
	a.Mount("/x", b)
	http.ListenAndServe(":80", a)
}
"#;
        assert_eq!(
            routes(&[("main.go", main), ("web/router.go", ROUTER)]),
            ["chi GET /a [] Home", "chi GET /x/b [] Home"]
        );
    }

    #[test]
    fn a_test_file_gives_no_route_and_no_mount() {
        let test = r#"package main

import (
	"net/http"
	"testing"

	"example.com/app/api"
	"example.com/app/web"
)

func TestRoutes(t *testing.T) {
	m := web.NewRouter()
	m.Mount("/other", api.Routes())
	m.Get("/test", Home)
}
"#;
        assert_eq!(gitea_shape(&[("main_test.go", test)]), gitea_shape(&[]));
    }

    #[test]
    fn a_router_in_a_cycle_of_links_gives_no_entry() {
        let main = r#"package main

import (
	"net/http"

	"example.com/app/web"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func first(m *web.Router) {
	m.Get("/first", Home)
	m.Group("/again", func() {
		second(m)
	})
}

func second(m *web.Router) {
	first(m)
}

func main() {
	first(web.NewRouter())
}
"#;
        assert!(routes(&[("main.go", main), ("web/router.go", ROUTER)]).is_empty());
    }

    /// The routes and the time of the search for entry points, for files with the router type.
    fn timed(main: String) -> (usize, std::time::Duration) {
        let (found, took) = entries_timed(
            Language::Go,
            &[("main.go", &main), ("web/router.go", ROUTER)],
            true,
        );
        let routes = found
            .iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .count();
        (routes, took)
    }

    const HEADER: &str = "package main\n\nimport (\n\t\"net/http\"\n\n\t\"example.com/app/web\"\n)\n\nfunc Home(w http.ResponseWriter, r *http.Request) {}\n\n";

    fn many_groups_with_a_route(count: usize) -> (usize, std::time::Duration) {
        let mut main = String::from(HEADER);
        main.push_str("func main() {\n\tm := web.NewRouter()\n");
        for number in 0..count {
            main.push_str(&format!(
                "\tm.Group(\"/g{number}\", func() {{\n\t\tm.Get(\"/x\", Home)\n\t}}, guard{number})\n"
            ));
        }
        main.push_str("}\n");
        timed(main)
    }

    fn many_functions_with_a_router_parameter(count: usize) -> (usize, std::time::Duration) {
        let mut main = String::from(HEADER);
        for number in 0..count {
            main.push_str(&format!(
                "func add{number}(m *web.Router) {{\n\tm.Get(\"/p{number}\", Home)\n}}\n\n"
            ));
        }
        main.push_str("func main() {\n\tm := web.NewRouter()\n\tm.Group(\"/api\", func() {\n");
        for number in 0..count {
            main.push_str(&format!("\t\tadd{number}(m)\n"));
        }
        main.push_str("\t})\n}\n");
        timed(main)
    }

    fn many_mounted_factories(count: usize) -> (usize, std::time::Duration) {
        let mut main = String::from(HEADER);
        for number in 0..count {
            main.push_str(&format!(
                "func routes{number}() *web.Router {{\n\tm := web.NewRouter()\n\tm.Get(\"/x\", Home)\n\treturn m\n}}\n\n"
            ));
        }
        main.push_str("func main() {\n\tr := web.NewRouter()\n");
        for number in 0..count {
            main.push_str(&format!("\tr.Mount(\"/m{number}\", routes{number}())\n"));
        }
        main.push_str("}\n");
        timed(main)
    }

    fn a_chain_of_router_parameters(count: usize) -> (usize, std::time::Duration) {
        let mut main = String::from(HEADER);
        for number in 0..count {
            main.push_str(&format!(
                "func step{number}(m *web.Router) {{\n\tm.Get(\"/s{number}\", Home)\n\tstep{next}(m)\n}}\n\n",
                next = number + 1
            ));
        }
        main.push_str(&format!(
            "func step{count}(m *web.Router) {{}}\n\nfunc main() {{\n\tstep0(web.NewRouter())\n}}\n"
        ));
        timed(main)
    }

    #[test]
    fn many_registrations_give_each_route() {
        assert_eq!(many_groups_with_a_route(50).0, 50);
        assert_eq!(many_functions_with_a_router_parameter(50).0, 50);
        assert_eq!(many_mounted_factories(50).0, 50);
        assert_eq!(a_chain_of_router_parameters(50).0, 50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_groups_with_a_route_cost_linear_time() {
        assert_scales_linearly("groups", 10_000, |count| many_groups_with_a_route(count).1);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_functions_with_a_router_parameter_cost_linear_time() {
        assert_scales_linearly("parameters", 10_000, |count| {
            many_functions_with_a_router_parameter(count).1
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_mounted_factories_cost_linear_time() {
        assert_scales_linearly("factories", 10_000, |count| many_mounted_factories(count).1);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_router_parameters_costs_linear_time() {
        assert_scales_linearly("chain", 10_000, |count| {
            a_chain_of_router_parameters(count).1
        });
    }

    fn many_types_with_a_get_method(count: usize) -> (usize, std::time::Duration) {
        let mut main = String::from(HEADER);
        for number in 0..count {
            main.push_str(&format!(
                "type Cache{number} struct{{ data map[string]any }}\n\nfunc (c *Cache{number}) Get(key string) any {{ return c.data[key] }}\n\n"
            ));
        }
        main.push_str("func main() {\n\tm := web.NewRouter()\n\tm.Get(\"/x\", Home)\n}\n");
        timed(main)
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_types_with_a_get_method_cost_linear_time() {
        assert_eq!(many_types_with_a_get_method(10).0, 1);
        assert_scales_linearly("types", 10_000, |count| {
            many_types_with_a_get_method(count).1
        });
    }

    #[test]
    fn a_prefix_that_is_not_a_string_literal_hides_the_paths_below_it() {
        let main = r#"package main

import (
	"net/http"

	"example.com/app/web"
)

const base = "/base"

func Home(w http.ResponseWriter, r *http.Request) {}

func sub() *web.Router {
	m := web.NewRouter()
	m.Get("/sub", Home)
	return m
}

func add(m *web.Router) {
	m.Get("/added", Home)
}

func main() {
	m := web.NewRouter()
	m.Group(base, func() {
		m.Get("/grouped", Home)
		add(m)
	})
	prefix := "/mounted"
	m.Mount(prefix, sub())
	m.Get("/plain", Home)
}
"#;
        assert_eq!(
            routes(&[("main.go", main), ("web/router.go", ROUTER)]),
            ["chi GET /plain [] Home"]
        );
    }
}
