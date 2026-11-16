//! Router registrations of Go web frameworks.
//!
//! A router is a value that a framework constructor makes, or a value that a
//! group, a sub-router, or a typed parameter gives. The walk keeps each router
//! in a scope, in source order. A prefix and a guard come from the router that
//! the receiver of a registration names, never from the name of a variable.
//!
//! When the syntax does not prove a fact, the walk gives no entry. A router
//! that a function receives as a parameter, and that a call of the file does not
//! give to the function, has an unknown prefix. Thus its routes give no entry.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{route, Context, Draft, EntryBasis, EntryKind};
use crate::index::model::{ImportKind, Span, Symbol};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Framework {
    Chi,
    Echo,
    Mux,
    Gin,
}

impl Framework {
    const ALL: [Self; 4] = [Self::Chi, Self::Echo, Self::Mux, Self::Gin];

    fn name(self) -> &'static str {
        match self {
            Self::Chi => "chi",
            Self::Echo => "echo",
            Self::Mux => "gorilla/mux",
            Self::Gin => "gin",
        }
    }

    /// The package name of the module, which an import without an alias binds.
    fn package(self) -> &'static str {
        match self {
            Self::Chi => "chi",
            Self::Echo => "echo",
            Self::Mux => "mux",
            Self::Gin => "gin",
        }
    }

    fn owns(self, module: &str) -> bool {
        let base = match self {
            Self::Chi => "github.com/go-chi/chi",
            Self::Echo => "github.com/labstack/echo",
            Self::Mux => "github.com/gorilla/mux",
            Self::Gin => "github.com/gin-gonic/gin",
        };
        module == base
            || module
                .strip_prefix(base)
                .and_then(|rest| rest.strip_prefix("/v"))
                .is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                })
    }

    fn router_types(self) -> &'static [&'static str] {
        match self {
            Self::Chi => &["Router", "Mux"],
            Self::Echo => &["Echo", "Group"],
            Self::Mux => &["Router"],
            Self::Gin => &["Engine", "RouterGroup", "IRouter", "IRoutes"],
        }
    }

    /// The types that only a root router has. A value of another type can be a group,
    /// a sub-router, or a mounted router, which has a prefix that the type does not give.
    fn is_root_type(self, name: &str) -> bool {
        matches!(
            (self, name),
            (Self::Chi, "Mux") | (Self::Echo, "Echo") | (Self::Gin, "Engine")
        )
    }

    fn handler_kind(self) -> HandlerKind {
        match self {
            Self::Chi | Self::Mux => HandlerKind::Http,
            Self::Echo => HandlerKind::Echo,
            Self::Gin => HandlerKind::Gin,
        }
    }

    /// The path of a route, from the prefixes of its routers and its own path. The rules
    /// are the rules of each library. None means that the library rejects the route.
    fn compose(self, prefixes: &[String], path: &str) -> Option<String> {
        let parts = || prefixes.iter().map(String::as_str).chain([path]);
        match self {
            // chi and gorilla/mux require a leading slash. They trim the slashes at the
            // end of the prefix before they add the path of the sub-router.
            Self::Chi | Self::Mux => {
                let mut joined = String::new();
                for part in parts().filter(|part| !part.is_empty()) {
                    if !part.starts_with('/') {
                        return None;
                    }
                    joined.truncate(joined.trim_end_matches('/').len());
                    joined.push_str(part);
                }
                (!joined.is_empty()).then_some(joined)
            }
            // echo v4 joins with `+`, and its router adds a missing leading slash.
            Self::Echo => {
                let joined: String = parts().collect();
                Some(match joined.as_str() {
                    "" => "/".to_string(),
                    text if text.starts_with('/') => joined,
                    _ => format!("/{joined}"),
                })
            }
            // gin uses `path.Join`, which cleans the result.
            Self::Gin => {
                let mut joined = "/".to_string();
                for part in parts() {
                    joined = gin_join(&joined, part);
                }
                Some(joined)
            }
        }
    }
}

/// `joinPaths` of gin.
fn gin_join(absolute: &str, relative: &str) -> String {
    if relative.is_empty() {
        return absolute.to_string();
    }
    let text = format!("{absolute}/{relative}");
    let mut parts: Vec<&str> = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let mut joined = format!("/{}", parts.join("/"));
    if relative.ends_with('/') && !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

/// The signature that a handler function has, for each family of frameworks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HandlerKind {
    Http,
    Gin,
    Echo,
}

impl HandlerKind {
    fn is_handler(self, signature: &str) -> bool {
        match self {
            Self::Http => {
                signature.contains("http.ResponseWriter") && signature.contains("http.Request")
            }
            Self::Gin => signature.contains("gin.Context"),
            Self::Echo => signature.contains("echo.Context"),
        }
    }

    fn is_middleware(self, signature: &str) -> bool {
        match self {
            Self::Http => signature.contains("http.Handler"),
            Self::Gin => signature.contains("gin.HandlerFunc"),
            Self::Echo => {
                signature.contains("echo.HandlerFunc") || signature.contains("echo.MiddlewareFunc")
            }
        }
    }
}

/// A router, the prefix of its place in a parent, and the guards of the parent at that time.
struct Link {
    parent: usize,
    parent_len: usize,
    prefix: String,
}

struct Router {
    framework: Framework,
    /// A group or a sub-router has one link. A mounted router has a link for each mount.
    links: Vec<Link>,
    guards: Vec<String>,
    /// The guards that run before the lookup of the route.
    pre: Vec<String>,
    /// A guard applies to a route that the router registers before the guard.
    retroactive: bool,
    /// A chi router that `With` or `Group` made.
    inline: bool,
    /// A chi router with a route. `Use` on it panics.
    frozen: bool,
    /// The router has a call that makes the program panic. The routes after it are not routes.
    invalid: bool,
    /// The prefix of the router is not known.
    partial: bool,
    mountable: bool,
    /// The function whose walk made the router, when no call of the file made the router.
    origin: Option<String>,
}

struct Record {
    router: usize,
    /// The guards of the router when the route was registered.
    limit: usize,
    range: Span,
    symbol: Option<String>,
    method: String,
    path: String,
    guards: Vec<String>,
}

struct Lineage {
    prefixes: Vec<String>,
    guards: Vec<String>,
}

struct Frame {
    function: bool,
    returned: Option<usize>,
}

#[derive(Default)]
pub(super) struct Handler {
    pub symbol: Option<String>,
    pub guards: Vec<String>,
    function_like: bool,
}

impl<'a> Context<'a> {
    /// The names that the imports of the file give to the package of the framework.
    fn go_package_names(&self, framework: Framework) -> Vec<String> {
        self.file
            .imports
            .iter()
            .filter(|binding| binding.kind == ImportKind::Module)
            .filter(|binding| framework.owns(&binding.module_path))
            .filter_map(|binding| {
                let unaliased = self
                    .source
                    .get(binding.range.start_byte)
                    .is_some_and(|byte| matches!(byte, b'"' | b'`'));
                if unaliased {
                    Some(framework.package().to_string())
                } else {
                    binding.local_name.clone()
                }
            })
            .collect()
    }

    /// The name of a guard that a call or an identifier gives, as `auth` or `middleware.Auth`.
    fn go_guard_name(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "identifier" | "selector_expression" => Some(self.text(node).to_string()),
            "call_expression" => self.go_guard_name(node.child_by_field_name("function")?),
            _ => None,
        }
    }

    fn go_symbol(&self, node: Node<'_>) -> Option<&Symbol> {
        let key = self.handler_symbol(node)?;
        self.all
            .iter()
            .flat_map(|file| &file.symbols)
            .find(|symbol| *symbol.key == *key)
    }

    /// The innermost function of a call such as `auth(cfg)(next)`.
    fn go_callee(function: Node<'_>) -> Option<Node<'_>> {
        match function.kind() {
            "identifier" | "selector_expression" => Some(function),
            "call_expression" => Self::go_callee(function.child_by_field_name("function")?),
            _ => None,
        }
    }

    /// The function of the repository that a wrapper call names, when its signature has the
    /// shape of a middleware of the framework.
    fn is_go_middleware(&self, function: Node<'_>, kind: HandlerKind) -> bool {
        Self::go_callee(function)
            .and_then(|callee| self.go_symbol(callee))
            .is_some_and(|symbol| kind.is_middleware(&symbol.signature))
    }

    /// The function that a handler argument names, and each wrapper call around it. A
    /// function inside a wrapper is the handler only when a conversion proves it, as
    /// `http.HandlerFunc(show)` does, or when its signature is the signature of a handler.
    /// A wrapper with more than one such function gives no function. A wrapper is a guard
    /// only when the repository has it, with the signature of a middleware.
    pub(super) fn go_handler(&self, node: Node<'_>, kind: HandlerKind) -> Handler {
        self.go_handler_at(node, kind, true)
    }

    fn go_handler_at(&self, node: Node<'_>, kind: HandlerKind, direct: bool) -> Handler {
        match node.kind() {
            "identifier" | "selector_expression" => {
                let symbol = if direct {
                    self.handler_symbol(node)
                } else {
                    self.go_symbol(node)
                        .filter(|symbol| kind.is_handler(&symbol.signature))
                        .map(|symbol| symbol.key.to_string())
                };
                Handler {
                    function_like: symbol.is_some(),
                    symbol,
                    guards: Vec::new(),
                }
            }
            "func_literal" => Handler {
                function_like: direct
                    || node
                        .child_by_field_name("parameters")
                        .is_some_and(|parameters| kind.is_handler(self.text(parameters))),
                ..Handler::default()
            },
            "call_expression" => {
                let Some(function) = node.child_by_field_name("function") else {
                    return Handler::default();
                };
                let arguments = Self::arguments_of(node);
                if self.is_go_adapter(function) {
                    return match arguments.as_slice() {
                        [only] => {
                            let mut inner = self.go_handler_at(*only, kind, true);
                            inner.function_like = inner.symbol.is_some();
                            inner
                        }
                        _ => Handler::default(),
                    };
                }
                let mut inner: Vec<Handler> = arguments
                    .into_iter()
                    .map(|argument| self.go_handler_at(argument, kind, false))
                    .filter(|handler| handler.function_like)
                    .collect();
                if inner.is_empty() {
                    return Handler::default();
                }
                let (symbol, nested) = match inner.pop() {
                    Some(only) if inner.is_empty() => (only.symbol, only.guards),
                    _ => (None, Vec::new()),
                };
                let mut guards = Vec::new();
                if self.is_go_middleware(function, kind) {
                    guards.extend(self.go_guard_name(function));
                }
                guards.extend(nested);
                Handler {
                    symbol,
                    guards,
                    function_like: true,
                }
            }
            _ => Handler::default(),
        }
    }

    /// A call that converts a function to a handler, as `http.HandlerFunc(show)`. It is not a guard.
    fn is_go_adapter(&self, function: Node<'_>) -> bool {
        if function.kind() != "selector_expression" {
            return false;
        }
        let (Some(operand), Some(field)) = (
            function.child_by_field_name("operand"),
            function.child_by_field_name("field"),
        ) else {
            return false;
        };
        let (operand, field) = (self.text(operand), self.text(field));
        let from_net_http = self
            .file
            .imports
            .iter()
            .filter(|binding| binding.local_name.as_deref() == Some(operand))
            .any(|binding| binding.module_path == "net/http");
        let from = |framework: Framework| {
            self.go_package_names(framework)
                .iter()
                .any(|name| name == operand)
        };
        (from_net_http && field == "HandlerFunc")
            || (from(Framework::Gin) && matches!(field, "WrapF" | "WrapH"))
            || (from(Framework::Echo) && field == "WrapHandler")
    }
}

pub(super) fn collect(context: &Context<'_>, root: Node<'_>, out: &mut Vec<Draft>) {
    let packages: Vec<(String, Framework)> = Framework::ALL
        .into_iter()
        .flat_map(|framework| {
            context
                .go_package_names(framework)
                .into_iter()
                .map(move |name| (name, framework))
        })
        .collect();
    if packages.is_empty() {
        return;
    }
    let mut routes = Routes {
        context,
        packages,
        routers: Vec::new(),
        records: Vec::new(),
        scopes: vec![HashMap::new()],
        consumed: HashSet::new(),
        functions: HashMap::new(),
        active: HashSet::new(),
        instantiated: HashSet::new(),
        origin: None,
        frames: Vec::new(),
    };
    routes.run(root);
    routes.finish(out);
}

struct Routes<'c, 'a, 't> {
    context: &'c Context<'a>,
    packages: Vec<(String, Framework)>,
    routers: Vec<Router>,
    records: Vec<Record>,
    scopes: Vec<HashMap<String, Option<usize>>>,
    consumed: HashSet<usize>,
    functions: HashMap<String, Node<'t>>,
    /// The functions that a call is reading now. A recursive call gives no router.
    active: HashSet<String>,
    /// The functions that a call of the file read. Their own walk gives no root router.
    instantiated: HashSet<String>,
    origin: Option<String>,
    frames: Vec<Frame>,
}

impl<'c, 'a, 't> Routes<'c, 'a, 't> {
    fn text(&self, node: Node<'t>) -> &'a str {
        self.context.text(node)
    }

    fn named_children(node: Node<'t>) -> Vec<Node<'t>> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).collect()
    }

    fn run(&mut self, root: Node<'t>) {
        let children = Self::named_children(root);
        for child in &children {
            if child.kind() == "function_declaration" {
                if let Some(name) = child.child_by_field_name("name") {
                    self.functions.insert(self.text(name).to_string(), *child);
                }
            }
        }
        for child in children
            .iter()
            .filter(|child| child.kind() == "var_declaration")
        {
            self.visit(*child);
        }
        for child in children
            .iter()
            .filter(|child| child.kind() != "var_declaration")
        {
            self.visit(*child);
        }
    }

    fn visit(&mut self, node: Node<'t>) {
        match node.kind() {
            "function_declaration" | "method_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .filter(|_| node.kind() == "function_declaration")
                    .map(|name| self.text(name));
                self.walk_function(node, name, None);
                return;
            }
            "func_literal" => {
                if !self.consumed.contains(&node.id()) {
                    self.closure(node, None);
                }
                return;
            }
            "block" => {
                self.scopes.push(HashMap::new());
                self.visit_children(node);
                self.scopes.pop();
                return;
            }
            "short_var_declaration" | "assignment_statement" => self.assignment(node),
            "var_spec" => self.var_spec(node),
            "return_statement" => self.return_statement(node),
            "call_expression" if !self.consumed.contains(&node.id()) => self.call(node),
            _ => {}
        }
        self.visit_children(node);
    }

    fn visit_children(&mut self, node: Node<'t>) {
        for child in Self::named_children(node) {
            self.visit(child);
        }
    }

    /// Reads the body of a function in a scope of its own. `declared` is the name of the
    /// function when the walk is the walk of its declaration. The result is the router that the
    /// function returns.
    fn walk_function(
        &mut self,
        node: Node<'t>,
        declared: Option<&str>,
        bound: Option<usize>,
    ) -> Option<usize> {
        let globals = self.scopes[0].clone();
        let saved = std::mem::replace(&mut self.scopes, vec![globals, HashMap::new()]);
        let saved_origin = std::mem::replace(&mut self.origin, declared.map(str::to_string));
        self.frames.push(Frame {
            function: true,
            returned: None,
        });
        self.bind_parameters(node.child_by_field_name("parameters"), bound);
        if let Some(body) = node.child_by_field_name("body") {
            self.visit(body);
        }
        let returned = self.frames.pop().and_then(|frame| frame.returned);
        self.origin = saved_origin;
        let globals = self.scopes.swap_remove(0);
        self.scopes = saved;
        self.scopes[0] = globals;
        returned
    }

    /// Reads a function of the file for a call. Each call makes new routers.
    fn instantiate(&mut self, name: &str, bound: Option<usize>) -> Option<usize> {
        let node = *self.functions.get(name)?;
        if bound.is_none() {
            node.child_by_field_name("result")?;
        }
        if !self.active.insert(name.to_string()) {
            return None;
        }
        self.instantiated.insert(name.to_string());
        let router = self.walk_function(node, None, bound);
        self.active.remove(name);
        router
    }

    fn closure(&mut self, literal: Node<'t>, bound: Option<usize>) {
        self.scopes.push(HashMap::new());
        self.frames.push(Frame {
            function: false,
            returned: None,
        });
        self.bind_parameters(literal.child_by_field_name("parameters"), bound);
        if let Some(body) = literal.child_by_field_name("body") {
            self.visit(body);
        }
        self.frames.pop();
        self.scopes.pop();
    }

    /// The function that a framework calls with a new router: a literal, or a name that the
    /// file defines. Another function gives no route, because its prefix is not known.
    fn callback(&mut self, function: Node<'t>, router: usize) {
        match function.kind() {
            "func_literal" => {
                self.consumed.insert(function.id());
                self.closure(function, Some(router));
            }
            "identifier" if self.lookup(self.text(function)).is_none() => {
                self.instantiate(self.text(function), Some(router));
            }
            _ => {}
        }
    }

    /// Binds each parameter name. `bound` is the router of the first parameter.
    fn bind_parameters(&mut self, parameters: Option<Node<'t>>, mut bound: Option<usize>) {
        let Some(parameters) = parameters else {
            return;
        };
        for declaration in Self::named_children(parameters) {
            let mut cursor = declaration.walk();
            let names: Vec<Node<'t>> = declaration
                .children_by_field_name("name", &mut cursor)
                .collect();
            let typed = declaration
                .child_by_field_name("type")
                .and_then(|kind| self.router_of_type(kind));
            for name in names {
                let router = bound.take().or(typed);
                self.bind(self.text(name), router, true);
            }
        }
    }

    fn router_of_type(&mut self, kind: Node<'t>) -> Option<usize> {
        let kind = if kind.kind() == "pointer_type" {
            kind.named_child(0)?
        } else {
            kind
        };
        if kind.kind() != "qualified_type" {
            return None;
        }
        let package = self.text(kind.child_by_field_name("package")?);
        let name = self.text(kind.child_by_field_name("name")?);
        let framework = self.framework_of(package)?;
        if !framework.router_types().contains(&name) {
            return None;
        }
        let router = self.make_root(framework);
        if !framework.is_root_type(name) {
            self.routers[router].partial = true;
            self.routers[router].mountable = false;
        }
        Some(router)
    }

    fn framework_of(&self, package: &str) -> Option<Framework> {
        if self.lookup(package).is_some() {
            return None;
        }
        self.packages
            .iter()
            .find(|(name, _)| name == package)
            .map(|(_, framework)| *framework)
    }

    fn lookup(&self, name: &str) -> Option<Option<usize>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn bind(&mut self, name: &str, router: Option<usize>, define: bool) {
        if name == "_" {
            return;
        }
        let index = if define {
            self.scopes.len() - 1
        } else {
            self.scopes
                .iter()
                .rposition(|scope| scope.contains_key(name))
                .unwrap_or(self.scopes.len() - 1)
        };
        self.scopes[index].insert(name.to_string(), router);
    }

    fn make_root(&mut self, framework: Framework) -> usize {
        self.routers.push(Router {
            framework,
            links: Vec::new(),
            guards: Vec::new(),
            pre: Vec::new(),
            retroactive: matches!(framework, Framework::Mux | Framework::Echo),
            inline: false,
            frozen: false,
            invalid: false,
            partial: false,
            mountable: true,
            origin: self.origin.clone(),
        });
        self.routers.len() - 1
    }

    fn make_child(
        &mut self,
        parent: usize,
        prefix: String,
        guards: Vec<String>,
        inline: bool,
    ) -> usize {
        let framework = self.routers[parent].framework;
        let link = Link {
            parent,
            parent_len: self.routers[parent].guards.len(),
            prefix,
        };
        self.routers.push(Router {
            framework,
            links: vec![link],
            guards,
            pre: Vec::new(),
            retroactive: framework == Framework::Mux,
            inline,
            frozen: false,
            invalid: self.routers[parent].invalid,
            partial: self.routers[parent].partial,
            mountable: false,
            origin: None,
        });
        self.routers.len() - 1
    }

    fn assignment(&mut self, node: Node<'t>) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        let define = node.kind() == "short_var_declaration";
        self.bind_all(
            &Self::named_children(left),
            &Self::named_children(right),
            define,
        );
    }

    fn var_spec(&mut self, node: Node<'t>) {
        let mut cursor = node.walk();
        let names: Vec<Node<'t>> = node.children_by_field_name("name", &mut cursor).collect();
        match node.child_by_field_name("value") {
            Some(value) => self.bind_all(&names, &Self::named_children(value), true),
            None => {
                let typed = node
                    .child_by_field_name("type")
                    .and_then(|kind| self.router_of_type(kind));
                if let [name] = names.as_slice() {
                    self.bind(self.text(*name), typed, true);
                }
            }
        }
    }

    fn bind_all(&mut self, names: &[Node<'t>], values: &[Node<'t>], define: bool) {
        let routers: Vec<Option<usize>> = if names.len() == values.len() {
            values.iter().map(|value| self.eval(*value)).collect()
        } else {
            vec![None; names.len()]
        };
        for (name, router) in names.iter().zip(routers) {
            if name.kind() == "identifier" {
                self.bind(self.text(*name), router, define);
            }
        }
    }

    fn return_statement(&mut self, node: Node<'t>) {
        let Some(list) = node.named_child(0) else {
            return;
        };
        let values = Self::named_children(list);
        let [value] = values.as_slice() else {
            return;
        };
        let router = self.eval(*value);
        if let Some(frame) = self.frames.last_mut().filter(|frame| frame.function) {
            frame.returned = frame.returned.or(router);
        }
    }

    /// The router that an expression gives, when the syntax proves it.
    fn eval(&mut self, expression: Node<'t>) -> Option<usize> {
        match expression.kind() {
            "parenthesized_expression" => self.eval(expression.named_child(0)?),
            "identifier" => self.lookup(self.text(expression))?,
            "call_expression" => self.eval_call(expression),
            _ => None,
        }
    }

    fn eval_call(&mut self, call: Node<'t>) -> Option<usize> {
        let function = call.child_by_field_name("function")?;
        let arguments = Context::arguments_of(call);
        if function.kind() == "identifier" {
            let name = self.text(function);
            return match self.lookup(name) {
                Some(_) => None,
                None => self.instantiate(name, None),
            };
        }
        if function.kind() != "selector_expression" {
            return None;
        }
        let name = self.text(function.child_by_field_name("field")?);
        let operand = function.child_by_field_name("operand")?;
        if operand.kind() == "identifier" {
            if let Some(framework) = self.framework_of(self.text(operand)) {
                return self.construct(framework, name);
            }
        }
        if name == "Subrouter" {
            return self.subrouter(operand);
        }
        let parent = self.eval(operand)?;
        match (self.routers[parent].framework, name) {
            (Framework::Chi, "With") => {
                let guards = self.guard_names(&arguments);
                self.freeze(parent);
                Some(self.make_child(parent, String::new(), guards, true))
            }
            (Framework::Echo | Framework::Gin, "Group") => {
                let prefix = self.string_argument(*arguments.first()?)?;
                let guards = self.guard_names(&arguments[1..]);
                Some(self.make_child(parent, prefix, guards, false))
            }
            // `Use` of gin gives the router, as the interface `IRoutes` says.
            (Framework::Gin, "Use") => {
                if self.consumed.insert(call.id()) {
                    let guards = self.guard_names(&arguments);
                    self.routers[parent].guards.extend(guards);
                }
                Some(parent)
            }
            _ => None,
        }
    }

    fn construct(&mut self, framework: Framework, name: &str) -> Option<usize> {
        let makes = match framework {
            Framework::Chi => matches!(name, "NewRouter" | "NewMux"),
            Framework::Echo => name == "New",
            Framework::Mux => name == "NewRouter",
            Framework::Gin => matches!(name, "Default" | "New"),
        };
        makes.then(|| self.make_root(framework))
    }

    /// A chi router with a handler. A call of `With` or `Group` on a router that is not
    /// inline makes the handler too, as chi v5 does in `Mux.With`.
    fn freeze(&mut self, router: usize) {
        self.routers[router].frozen = true;
    }

    fn guard_names(&self, arguments: &[Node<'t>]) -> Vec<String> {
        arguments
            .iter()
            .filter_map(|argument| self.context.go_guard_name(*argument))
            .collect()
    }

    fn string_argument(&self, node: Node<'t>) -> Option<String> {
        self.context
            .is_string(node)
            .then(|| self.context.string_of(node))
    }

    fn call(&mut self, node: Node<'t>) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        if function.kind() != "selector_expression" {
            return;
        }
        let (Some(field), Some(operand)) = (
            function.child_by_field_name("field"),
            function.child_by_field_name("operand"),
        ) else {
            return;
        };
        let name = self.text(field);
        if self.mux_registration(node) {
            return;
        }
        let Some(router) = self.eval(operand) else {
            return;
        };
        let arguments = Context::arguments_of(node);
        let framework = self.routers[router].framework;
        match (framework, name) {
            // chi v5 panics when `Use` follows a route of the same router.
            (Framework::Chi, "Use") if self.routers[router].frozen => {
                self.routers[router].invalid = true;
            }
            (_, "Use") => {
                let guards = self.guard_names(&arguments);
                self.routers[router].guards.extend(guards);
            }
            (Framework::Echo, "Pre") => {
                let guards = self.guard_names(&arguments);
                self.routers[router].pre.extend(guards);
            }
            (Framework::Chi, _) => self.chi_call(router, node, name, &arguments),
            (Framework::Echo, _) => self.echo_call(router, node, name, &arguments),
            (Framework::Gin, _) => self.gin_call(router, node, name, &arguments),
            (Framework::Mux, _) => {}
        }
    }

    fn chi_call(&mut self, router: usize, node: Node<'t>, name: &str, arguments: &[Node<'t>]) {
        let range = Span::of(&node);
        let strings: Vec<Option<String>> = arguments
            .iter()
            .map(|argument| self.string_argument(*argument))
            .collect();
        match (name, arguments, strings.as_slice()) {
            (
                "Get" | "Post" | "Put" | "Patch" | "Delete" | "Head" | "Options" | "Connect"
                | "Trace",
                [_, handler],
                [Some(path), _],
            ) => self.register(router, &[name], path, Some(*handler), &[], range),
            ("HandleFunc" | "Handle", [_, handler], [Some(path), _]) => {
                self.register(router, &["ANY"], path, Some(*handler), &[], range)
            }
            ("Method" | "MethodFunc", [_, _, handler], [Some(method), Some(path), _]) => {
                self.register(router, &[method.as_str()], path, Some(*handler), &[], range)
            }
            ("Route", [_, callback], [Some(prefix), _]) => {
                self.freeze(router);
                let child = self.make_child(router, prefix.clone(), Vec::new(), false);
                self.callback(*callback, child);
            }
            ("Group", [callback], _) => {
                if !self.routers[router].inline {
                    self.freeze(router);
                }
                let child = self.make_child(router, String::new(), Vec::new(), true);
                self.callback(*callback, child);
            }
            ("Mount", [_, mounted], [Some(prefix), _]) => {
                self.freeze(router);
                self.mount(router, prefix, *mounted)
            }
            _ => {}
        }
    }

    /// `router.PathPrefix("/api").Subrouter()`: `operand` is the call of `PathPrefix`.
    fn subrouter(&mut self, operand: Node<'t>) -> Option<usize> {
        let function = operand.child_by_field_name("function")?;
        if operand.kind() != "call_expression"
            || function.kind() != "selector_expression"
            || self.text(function.child_by_field_name("field")?) != "PathPrefix"
        {
            return None;
        }
        let parent = self.eval(function.child_by_field_name("operand")?)?;
        if self.routers[parent].framework != Framework::Mux {
            return None;
        }
        let prefix = self.string_argument(*Context::arguments_of(operand).first()?)?;
        Some(self.make_child(parent, prefix, Vec::new(), false))
    }

    fn is_mux_matcher_call(&self, call: Node<'t>) -> bool {
        call.child_by_field_name("function")
            .filter(|function| function.kind() == "selector_expression")
            .and_then(|function| function.child_by_field_name("field"))
            .is_some_and(|field| {
                matches!(
                    self.text(field),
                    "Methods"
                        | "Path"
                        | "PathPrefix"
                        | "Name"
                        | "Queries"
                        | "Headers"
                        | "Schemes"
                        | "Host"
                        | "MatcherFunc"
                        | "HandlerFunc"
                        | "HandleFunc"
                        | "Handle"
                        | "Handler"
                )
            })
    }

    /// A route of a gorilla router: `router.HandleFunc("/path", handler)`, or a chain of
    /// matchers that ends in `HandlerFunc` or `Handler`. Without `Methods`, the route has
    /// each method. A chain without one path, or with more than one path, gives no entry.
    /// Returns true when the call is a registration of a gorilla router. The call consumes its chain.
    fn mux_registration(&mut self, outer: Node<'t>) -> bool {
        let mut chain = Vec::new();
        let mut methods: Option<Vec<String>> = None;
        let mut paths: Vec<Option<String>> = Vec::new();
        let mut handler: Option<Node<'t>> = None;
        let mut current = outer;
        let base = loop {
            let Some(function) = current
                .child_by_field_name("function")
                .filter(|function| function.kind() == "selector_expression")
            else {
                return false;
            };
            let (Some(field), Some(operand)) = (
                function.child_by_field_name("field"),
                function.child_by_field_name("operand"),
            ) else {
                return false;
            };
            let arguments = Context::arguments_of(current);
            chain.push(current.id());
            match self.text(field) {
                "HandleFunc" | "Handle" if handler.is_none() => {
                    paths.push(
                        arguments
                            .first()
                            .and_then(|path| self.string_argument(*path)),
                    );
                    handler = arguments.get(1).copied();
                    break operand;
                }
                "HandlerFunc" | "Handler" if handler.is_none() => {
                    handler = arguments.first().copied();
                }
                "Methods" => {
                    let names: Vec<String> = arguments
                        .iter()
                        .filter_map(|argument| self.string_argument(*argument))
                        .collect();
                    methods = Some(if names.len() == arguments.len() {
                        names
                    } else {
                        Vec::new()
                    });
                }
                "Path" | "PathPrefix" => paths.push(
                    arguments
                        .first()
                        .and_then(|path| self.string_argument(*path)),
                ),
                "Name" | "Queries" | "Headers" | "Schemes" | "Host" | "MatcherFunc" => {}
                _ => return false,
            }
            if operand.kind() == "call_expression" && self.is_mux_matcher_call(operand) {
                current = operand;
            } else {
                break operand;
            }
        };
        let Some(handler) = handler else {
            return false;
        };
        let Some(router) = self.eval(base) else {
            return false;
        };
        if self.routers[router].framework != Framework::Mux {
            return false;
        }
        self.consumed.extend(chain);
        let [Some(path)] = paths.as_slice() else {
            return true;
        };
        let methods = methods.unwrap_or_else(|| vec!["ANY".to_string()]);
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
        self.register(router, &methods, path, Some(handler), &[], Span::of(&outer));
        true
    }

    /// The string literals in an expression, as the methods of `[]string{"GET", "POST"}`.
    fn strings_in(&self, node: Node<'t>) -> Vec<String> {
        if self.context.is_string(node) {
            return vec![self.context.string_of(node)];
        }
        Self::named_children(node)
            .into_iter()
            .flat_map(|child| self.strings_in(child))
            .collect()
    }

    /// `router.GET("/path", guard, handler)`. The last argument is the handler, and the
    /// arguments before it are guards. gin accepts only a method of capital letters.
    fn gin_call(&mut self, router: usize, node: Node<'t>, name: &str, arguments: &[Node<'t>]) {
        let (methods, rest): (Vec<String>, &[Node<'t>]) = match (name, arguments) {
            ("GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS", _) => {
                (vec![name.to_string()], arguments)
            }
            ("Any", _) => (vec!["ANY".to_string()], arguments),
            ("Handle", [method, rest @ ..]) => match self.string_argument(*method) {
                Some(method) => (vec![method], rest),
                None => return,
            },
            ("Match", [methods, rest @ ..]) => (self.strings_in(*methods), rest),
            _ => return,
        };
        let [path, guards @ .., handler] = rest else {
            return;
        };
        let Some(path) = self.string_argument(*path) else {
            return;
        };
        let methods: Vec<&str> = methods
            .iter()
            .map(String::as_str)
            .filter(|method| {
                *method == "ANY" || method.bytes().all(|byte| byte.is_ascii_uppercase())
            })
            .collect();
        let range = Span::of(&node);
        self.register(router, &methods, &path, Some(*handler), guards, range);
    }

    fn echo_call(&mut self, router: usize, node: Node<'t>, name: &str, arguments: &[Node<'t>]) {
        let range = Span::of(&node);
        let verb = matches!(
            name,
            "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "CONNECT" | "TRACE"
        );
        let (methods, rest): (Vec<String>, &[Node<'t>]) = match (name, arguments) {
            (_, [_, ..]) if verb => (vec![name.to_string()], arguments),
            ("Any", [_, ..]) => (vec!["ANY".to_string()], arguments),
            ("Match", [methods, rest @ ..]) => (self.strings_in(*methods), rest),
            ("Add", [method, rest @ ..]) => match self.string_argument(*method) {
                Some(method) => (vec![method], rest),
                None => return,
            },
            _ => return,
        };
        let (Some(path), Some(handler)) = (rest.first(), rest.get(1)) else {
            return;
        };
        let Some(path) = self.string_argument(*path) else {
            return;
        };
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
        self.register(router, &methods, &path, Some(*handler), &rest[2..], range);
    }

    /// Joins a router that the file makes to the router that mounts it. A router can have
    /// more than one mount. A router of another file is not followed.
    fn mount(&mut self, parent: usize, prefix: &str, mounted: Node<'t>) {
        let Some(child) = self.eval(mounted) else {
            return;
        };
        if !self.routers[child].mountable || self.reaches(parent, child) {
            return;
        }
        let link = Link {
            parent,
            parent_len: self.routers[parent].guards.len(),
            prefix: prefix.to_string(),
        };
        self.routers[child].links.push(link);
    }

    fn reaches(&self, from: usize, target: usize) -> bool {
        from == target
            || self.routers[from]
                .links
                .iter()
                .any(|link| self.reaches(link.parent, target))
    }

    fn register(
        &mut self,
        router: usize,
        methods: &[&str],
        path: &str,
        handler: Option<Node<'t>>,
        guard_arguments: &[Node<'t>],
        range: Span,
    ) {
        self.routers[router].frozen = true;
        if self.routers[router].invalid {
            return;
        }
        let kind = self.routers[router].framework.handler_kind();
        let handler = handler.map(|node| self.context.go_handler(node, kind));
        let mut guards = self.guard_names(guard_arguments);
        let symbol = handler.map(|handler| {
            guards.extend(handler.guards);
            handler.symbol
        });
        let limit = self.routers[router].guards.len();
        for method in methods {
            self.records.push(Record {
                router,
                limit,
                range,
                symbol: symbol.clone().flatten(),
                method: method.to_uppercase(),
                path: path.to_string(),
                guards: guards.clone(),
            });
        }
    }

    /// The prefixes and the guards of each place of a router, up to `limit` guards of the
    /// router itself. A router that has no place that the file proves gives none.
    fn lineage(&self, id: usize, limit: Option<usize>) -> Vec<Lineage> {
        let router = &self.routers[id];
        if router.partial {
            return Vec::new();
        }
        let count = match limit {
            Some(count) if !router.retroactive => count.min(router.guards.len()),
            _ => router.guards.len(),
        };
        let mut own = router.pre.clone();
        own.extend(router.guards[..count].iter().cloned());
        if router.links.is_empty() {
            let read_by_a_call = router
                .origin
                .as_ref()
                .is_some_and(|origin| self.instantiated.contains(origin));
            return if read_by_a_call {
                Vec::new()
            } else {
                vec![Lineage {
                    prefixes: Vec::new(),
                    guards: own,
                }]
            };
        }
        let mut places = Vec::new();
        for link in &router.links {
            let parent_limit = (!self.routers[link.parent].retroactive).then_some(link.parent_len);
            for mut place in self.lineage(link.parent, parent_limit) {
                if !link.prefix.is_empty() {
                    place.prefixes.push(link.prefix.clone());
                }
                place.guards.extend(own.iter().cloned());
                places.push(place);
            }
        }
        places
    }

    fn finish(self, out: &mut Vec<Draft>) {
        for record in &self.records {
            let framework = self.routers[record.router].framework;
            for place in self.lineage(record.router, Some(record.limit)) {
                let Some(path) = framework.compose(&place.prefixes, &record.path) else {
                    continue;
                };
                let mut guards = place.guards;
                guards.extend(record.guards.iter().cloned());
                out.push(Draft {
                    range: record.range,
                    kind: EntryKind::HttpHandler,
                    symbol: record.symbol.clone(),
                    framework: framework.name(),
                    route: Some(route(&record.method, path)),
                    guards,
                    basis: EntryBasis::Registered,
                });
            }
        }
    }
}
