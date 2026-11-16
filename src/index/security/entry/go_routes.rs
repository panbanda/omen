//! Router registrations of Go web frameworks.
//!
//! A router is a value that a framework constructor makes, or a value that a
//! group, a sub-router, or a typed parameter gives. The walk keeps each router
//! in a scope, in source order. A prefix and a guard come from the router that
//! the receiver of a registration names, never from the name of a variable.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{join_path, route, Context, Draft, EntryBasis, EntryKind};
use crate::index::model::{ImportKind, Span};

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
}

struct Router {
    framework: Framework,
    parent: Option<usize>,
    /// The guards of the parent when the router was made.
    parent_len: usize,
    prefix: String,
    guards: Vec<String>,
    /// A guard applies to a route that the router registers before the guard.
    retroactive: bool,
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

    /// The function that a handler argument names, and each wrapper call around it. The
    /// function is the one argument of a wrapper that resolves to a function of the
    /// repository. A wrapper with more than one such argument gives no function.
    pub(super) fn go_handler(&self, node: Node<'_>) -> Handler {
        match node.kind() {
            "identifier" | "selector_expression" => {
                let symbol = self.handler_symbol(node);
                Handler {
                    function_like: symbol.is_some(),
                    symbol,
                    guards: Vec::new(),
                }
            }
            "func_literal" => Handler {
                function_like: true,
                ..Handler::default()
            },
            "call_expression" => {
                let Some(function) = node.child_by_field_name("function") else {
                    return Handler::default();
                };
                let arguments = Self::arguments_of(node);
                if self.is_go_adapter(function) {
                    return match arguments.as_slice() {
                        [only] => self.go_handler(*only),
                        _ => Handler::default(),
                    };
                }
                let mut inner: Vec<Handler> = arguments
                    .into_iter()
                    .map(|argument| self.go_handler(argument))
                    .filter(|handler| handler.function_like)
                    .collect();
                if inner.is_empty() {
                    return Handler::default();
                }
                let Some(name) = self.go_guard_name(function) else {
                    return Handler {
                        function_like: true,
                        ..Handler::default()
                    };
                };
                let (symbol, nested) = match inner.pop() {
                    Some(only) if inner.is_empty() => (only.symbol, only.guards),
                    _ => (None, Vec::new()),
                };
                let mut guards = vec![name];
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
        done: HashSet::new(),
        returns: HashMap::new(),
        current: Vec::new(),
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
    done: HashSet<String>,
    returns: HashMap<String, Option<usize>>,
    current: Vec<Option<String>>,
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
            "function_declaration" => self.function(node),
            "method_declaration" => self.function(node),
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
        if !matches!(node.kind(), "function_declaration" | "method_declaration") {
            self.visit_children(node);
        }
    }

    fn visit_children(&mut self, node: Node<'t>) {
        for child in Self::named_children(node) {
            self.visit(child);
        }
    }

    fn function(&mut self, node: Node<'t>) {
        let name = (node.kind() == "function_declaration")
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .map(|name| self.text(name).to_string());
        if let Some(name) = &name {
            if !self.done.insert(name.clone()) {
                return;
            }
        }
        let globals = self.scopes[0].clone();
        let saved = std::mem::replace(&mut self.scopes, vec![globals, HashMap::new()]);
        self.current.push(name);
        self.bind_parameters(node.child_by_field_name("parameters"), None);
        if let Some(body) = node.child_by_field_name("body") {
            self.visit(body);
        }
        self.current.pop();
        let globals = self.scopes.swap_remove(0);
        self.scopes = saved;
        self.scopes[0] = globals;
    }

    fn closure(&mut self, literal: Node<'t>, bound: Option<usize>) {
        self.scopes.push(HashMap::new());
        self.current.push(None);
        self.bind_parameters(literal.child_by_field_name("parameters"), bound);
        if let Some(body) = literal.child_by_field_name("body") {
            self.visit(body);
        }
        self.current.pop();
        self.scopes.pop();
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
        framework
            .router_types()
            .contains(&name)
            .then(|| self.new_router(framework, None, String::new(), Vec::new()))
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

    fn new_router(
        &mut self,
        framework: Framework,
        parent: Option<usize>,
        prefix: String,
        guards: Vec<String>,
    ) -> usize {
        let parent_len = parent.map_or(0, |parent| self.routers[parent].guards.len());
        self.routers.push(Router {
            framework,
            parent,
            parent_len,
            prefix,
            guards,
            retroactive: framework == Framework::Mux
                || (framework == Framework::Echo && parent.is_none()),
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
        let Some(Some(name)) = self.current.last().cloned() else {
            return;
        };
        let Some(list) = node.named_child(0) else {
            return;
        };
        let values = Self::named_children(list);
        if let [value] = values.as_slice() {
            if let Some(router) = self.eval(*value) {
                self.returns.insert(name, Some(router));
            }
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
            return self.local_router(self.text(function));
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
                Some(self.new_router(Framework::Chi, Some(parent), String::new(), guards))
            }
            (Framework::Echo | Framework::Gin, "Group") => {
                let prefix = self.string_argument(*arguments.first()?)?;
                let guards = self.guard_names(&arguments[1..]);
                let framework = self.routers[parent].framework;
                Some(self.new_router(framework, Some(parent), prefix, guards))
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
        makes.then(|| self.new_router(framework, None, String::new(), Vec::new()))
    }

    /// The router that a function of the file returns. The walk reads the function once.
    fn local_router(&mut self, name: &str) -> Option<usize> {
        let node = *self.functions.get(name)?;
        if !self.done.contains(name) {
            self.function(node);
        }
        self.returns.get(name).copied().flatten()
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
        if name == "Use" {
            let guards = self.guard_names(&arguments);
            self.routers[router].guards.extend(guards);
            return;
        }
        match self.routers[router].framework {
            Framework::Chi => self.chi_call(router, node, name, &arguments),
            Framework::Echo => self.echo_call(router, node, name, &arguments),
            Framework::Gin => self.gin_call(router, node, name, &arguments),
            Framework::Mux => {}
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
            ("Route", [_, closure], [Some(prefix), _]) if closure.kind() == "func_literal" => {
                let child =
                    self.new_router(Framework::Chi, Some(router), prefix.clone(), Vec::new());
                self.consumed.insert(closure.id());
                self.closure(*closure, Some(child));
            }
            ("Group", [closure], _) if closure.kind() == "func_literal" => {
                let child =
                    self.new_router(Framework::Chi, Some(router), String::new(), Vec::new());
                self.consumed.insert(closure.id());
                self.closure(*closure, Some(child));
            }
            ("Mount", [_, mounted], [Some(prefix), _]) => self.mount(router, prefix, *mounted),
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
        Some(self.new_router(Framework::Mux, Some(parent), prefix, Vec::new()))
    }

    /// `router.HandleFunc("/path", handler).Methods("GET")`. The chain can have more
    /// matchers. Without `Methods`, the route has each method. Returns true when the
    /// call is a registration of a gorilla router, which the call consumes with its chain.
    fn mux_registration(&mut self, outer: Node<'t>) -> bool {
        let mut chain = Vec::new();
        let mut methods: Option<Vec<String>> = None;
        let mut current = outer;
        loop {
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
                "HandleFunc" | "Handle" => {
                    let Some(router) = self.eval(operand) else {
                        return false;
                    };
                    if self.routers[router].framework != Framework::Mux {
                        return false;
                    }
                    self.consumed.extend(chain);
                    let (Some(path), Some(handler)) = (
                        arguments
                            .first()
                            .and_then(|path| self.string_argument(*path)),
                        arguments.get(1),
                    ) else {
                        return true;
                    };
                    let methods = methods.unwrap_or_else(|| vec!["ANY".to_string()]);
                    let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
                    let range = Span::of(&outer);
                    self.register(router, &methods, &path, Some(*handler), &[], range);
                    return true;
                }
                "Methods" => {
                    let names: Vec<Option<String>> = arguments
                        .iter()
                        .map(|argument| self.string_argument(*argument))
                        .collect();
                    methods = Some(names.into_iter().flatten().collect());
                    if methods
                        .as_ref()
                        .is_some_and(|found| found.len() != arguments.len())
                    {
                        methods = Some(Vec::new());
                    }
                }
                "Name" | "Queries" | "Headers" | "Schemes" | "Host" | "MatcherFunc" => {}
                _ => return false,
            }
            if operand.kind() != "call_expression" {
                return false;
            }
            current = operand;
        }
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
    /// arguments before it are guards.
    fn gin_call(&mut self, router: usize, node: Node<'t>, name: &str, arguments: &[Node<'t>]) {
        if !matches!(
            name,
            "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
        ) {
            return;
        }
        let [path, guards @ .., handler] = arguments else {
            return;
        };
        let Some(path) = self.string_argument(*path) else {
            return;
        };
        let range = Span::of(&node);
        self.register(router, &[name], &path, Some(*handler), guards, range);
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

    /// Joins a router that the file makes to the router that mounts it. A router of
    /// another file, or a router that has a parent, is not followed.
    fn mount(&mut self, parent: usize, prefix: &str, mounted: Node<'t>) {
        let Some(child) = self.eval(mounted) else {
            return;
        };
        let mut ancestor = Some(parent);
        while let Some(router) = ancestor {
            if router == child {
                return;
            }
            ancestor = self.routers[router].parent;
        }
        if self.routers[child].parent.is_some() {
            return;
        }
        self.routers[child].parent = Some(parent);
        self.routers[child].parent_len = self.routers[parent].guards.len();
        self.routers[child].prefix = prefix.to_string();
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
        let handler = handler.map(|node| self.context.go_handler(node));
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

    /// The joined prefix and the guards of a router, up to `limit` guards of the router itself.
    fn lineage(&self, id: usize, limit: Option<usize>) -> (String, Vec<String>) {
        let router = &self.routers[id];
        let (prefix, mut guards) = match router.parent {
            Some(parent) => {
                let parent_limit = (!self.routers[parent].retroactive).then_some(router.parent_len);
                let (prefix, guards) = self.lineage(parent, parent_limit);
                (join_path(&prefix, &router.prefix), guards)
            }
            None => (join_path("", &router.prefix), Vec::new()),
        };
        let count = match limit {
            Some(count) if !router.retroactive => count.min(router.guards.len()),
            _ => router.guards.len(),
        };
        guards.extend(router.guards[..count].iter().cloned());
        (prefix, guards)
    }

    fn finish(self, out: &mut Vec<Draft>) {
        for record in &self.records {
            let (prefix, mut guards) = self.lineage(record.router, Some(record.limit));
            guards.extend(record.guards.iter().cloned());
            out.push(Draft {
                range: record.range,
                kind: EntryKind::HttpHandler,
                symbol: record.symbol.clone(),
                framework: self.routers[record.router].framework.name(),
                route: Some(route(&record.method, join_path(&prefix, &record.path))),
                guards,
                basis: EntryBasis::Registered,
            });
        }
    }
}
