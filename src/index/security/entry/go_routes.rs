//! Router registrations of Go web frameworks.
//!
//! The walk models the registrations of a valid program. It does not model a
//! framework panic. A program that panics at start, as a chi `Use` after a route
//! does in some versions, is out of scope.
//!
//! The walk reads the body of each function one time, at its declaration. A route
//! call gives its entry at its own place, with the guards that the function knows.
//! A router is a value that a framework constructor makes, or a value that a group,
//! a sub-router, or a typed parameter gives. The walk keeps each router in a scope,
//! in source order. A prefix and a guard come from the router that the receiver of a
//! registration names, never from the name of a variable.
//!
//! A function that makes a router and returns it has a summary when it is pure. A pure
//! function writes no package-level variable, no parameter, and no router that it did
//! not make. The summary has the prefixes and the guards of the returned router. A call
//! of the function gives a new router with these prefixes and guards. The routes that
//! the function registers on its own routers give their entries in the function, with
//! the paths of the function. A call can attach the returned router to a parent, so the
//! paths of these routes can depend on the call. When a call anywhere in the repository
//! attaches the result, or when the use of the result is not known, these routes give
//! no entry. A call that serves the result as the root does not attach it. A function
//! that no call in the repository uses keeps its routes. The analysis reads the calls of
//! the repository one time, with the references of the index, and shares the result.
//!
//! When the syntax does not prove a fact, the walk gives no entry. These values have
//! no proof: the result of a function without a summary, a package-level router that a
//! function assigns, and a router that a function receives as a parameter. Thus the
//! routes on these values give no entry.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{route_parameters, Context, Draft, EntryBasis, EntryKind, Route, TreeSource};
use crate::core::Language;
use crate::index::extract::FileSymbols;
use crate::index::model::{ImportKind, Reference, Span, Symbol};

/// The methods that `Any` registers. gin v1.11.0 `anyMethods`.
const GIN_ANY: [&str; 9] = [
    "GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS", "DELETE", "CONNECT", "TRACE",
];

/// The methods that `Any` registers. echo v4.13.4 `methods`.
const ECHO_ANY: [&str; 11] = [
    "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PROPFIND", "PUT", "TRACE",
    "REPORT",
];

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
            // chi and gorilla/mux need a leading slash. They trim the slashes at the
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

/// The parameter types and the result of a function, as the source writes them.
struct Signature {
    parameters: Vec<String>,
    result: String,
}

/// Splits a text at the commas that are not inside a bracket.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (position, character) in text.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(&text[start..position]);
                start = position + 1;
            }
            _ => {}
        }
    }
    pieces.push(&text[start..]);
    pieces
        .into_iter()
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// The position of the first white space that is not inside a bracket.
fn first_space(text: &str) -> Option<usize> {
    let mut depth = 0_i32;
    for (position, character) in text.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            character if character.is_whitespace() && depth == 0 => return Some(position),
            _ => {}
        }
    }
    None
}

/// The types of the parameter list that opens at `open`, and the text after its end.
fn parse_parameters(text: &str, open: usize) -> Option<(Vec<String>, &str)> {
    let mut depth = 0_i32;
    let mut close = None;
    for (offset, character) in text[open..].char_indices() {
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
    let close = close?;
    let pieces = split_top_level(&text[open + 1..close]);
    let split: Vec<(Option<&str>, &str)> = pieces
        .iter()
        .map(|piece| {
            if piece.starts_with("func(") || piece.starts_with("func ") {
                return (None, *piece);
            }
            match first_space(piece) {
                Some(space) => (Some(&piece[..space]), piece[space..].trim()),
                None => (None, *piece),
            }
        })
        .collect();
    let named = split.iter().any(|(name, _)| name.is_some());
    let mut types = Vec::new();
    let mut following: Option<&str> = None;
    for (name, kind) in split.iter().rev() {
        if name.is_some() {
            following = Some(kind);
            types.push(kind.to_string());
        } else if named {
            types.push(following.unwrap_or(kind).to_string());
        } else {
            types.push(kind.to_string());
        }
    }
    types.reverse();
    Some((types, text[close + 1..].trim().trim_end_matches('{').trim()))
}

/// The parameters and the result of the function that a symbol declares.
fn parse_signature(signature: &str, name: &str) -> Option<Signature> {
    let open = signature.find(&format!("{name}("))? + name.len();
    let (parameters, result) = parse_parameters(signature, open)?;
    Some(Signature {
        parameters,
        result: result.to_string(),
    })
}

/// The parameters and the result of a function type, as `func(http.Handler) http.Handler`.
fn parse_function_type(text: &str) -> Option<Signature> {
    let text = text.trim();
    let rest = text.strip_prefix("func")?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let (parameters, result) = parse_parameters(text, text.len() - rest.len())?;
    Some(Signature {
        parameters,
        result: result.to_string(),
    })
}

/// The names that the imports of a file give to a package.
fn package_names(file: &FileSymbols, owns: impl Fn(&str) -> bool, package: &str) -> Vec<String> {
    file.imports
        .iter()
        .filter(|binding| binding.kind == ImportKind::Module)
        .filter(|binding| owns(&binding.module_path))
        .filter_map(|binding| {
            let local = binding.local_name.as_deref()?;
            let default = binding.module_path.rsplit('/').next() == Some(local);
            Some(if default { package } else { local }.to_string())
        })
        .collect()
}

/// Tells if a type is `qualifier.member` with a qualifier of the list.
fn is_qualified(kind: &str, member: &str, qualifiers: &[String], pointer: bool) -> bool {
    let kind = match (kind.strip_prefix('*'), pointer) {
        (Some(rest), true) => rest,
        (None, false) => kind,
        _ => return false,
    };
    kind.split_once('.').is_some_and(|(qualifier, name)| {
        name == member && qualifiers.iter().any(|known| known == qualifier)
    })
}

#[derive(Clone)]
struct Link {
    parent: usize,
    /// The guards of the parent when the link was made.
    parent_len: usize,
    prefix: String,
}

#[derive(Clone)]
struct Router {
    framework: Framework,
    /// A group or a sub-router has one link. A mounted router has a link for each mount.
    links: Vec<Link>,
    guards: Vec<String>,
    /// The guards that run before the lookup of the route.
    pre: Vec<String>,
    /// A guard applies to a route that the router registers before the guard.
    retroactive: bool,
    /// The walk does not know the prefix of the router.
    partial: bool,
    mountable: bool,
    /// The function body that made the router. Zero is the package.
    owner: usize,
    /// A parameter gave the router. The function that changes it is not pure.
    external: bool,
    /// The prefixes of the router in the function that made it, from a summary.
    base: Vec<String>,
}

#[derive(Clone)]
struct Record {
    router: usize,
    /// The number of guards that the router had at the time of the registration.
    limit: usize,
    range: Span,
    symbol: Option<String>,
    method: String,
    path: String,
    guards: Vec<String>,
}

/// The router that a pure function returns.
#[derive(Clone)]
struct Summary {
    framework: Framework,
    prefixes: Vec<String>,
    guards: Vec<String>,
}

struct Lineage {
    prefixes: Vec<String>,
    guards: Vec<String>,
}

struct Frame {
    function: bool,
    returned: Option<usize>,
    owner: usize,
    pure: bool,
    /// The routers that the function body made.
    created: Vec<usize>,
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
        package_names(
            self.file,
            |module| framework.owns(module),
            framework.package(),
        )
    }

    /// Tells if a call anywhere in the repository attaches the result of the function, or
    /// uses it in a way that the walk cannot classify. The analysis computes the facts
    /// one time for all files.
    fn go_factory_blocked(&self, key: &str) -> bool {
        self.all_parsed
            .shared()
            .go_call_sites
            .get_or_init(|| CallSites::build(self.all, self.all_parsed))
            .is_blocked(key)
    }

    /// The name of a guard that a call or an identifier gives, as `auth` or `middleware.Auth`.
    fn go_guard_name(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "identifier" | "selector_expression" => Some(self.text(node).to_string()),
            "call_expression" => self.go_guard_name(node.child_by_field_name("function")?),
            _ => None,
        }
    }

    /// The function of the repository that a node names, and the file of the function.
    fn go_symbol(&self, node: Node<'_>) -> Option<(&Symbol, &FileSymbols)> {
        let key = self.handler_symbol(node)?;
        self.all.iter().find_map(|file| {
            file.symbols
                .iter()
                .find(|symbol| *symbol.key == *key)
                .map(|symbol| (symbol, file))
        })
    }

    /// The innermost function of a call such as `auth(cfg)(next)`.
    fn go_callee(function: Node<'_>) -> Option<Node<'_>> {
        match function.kind() {
            "identifier" | "selector_expression" => Some(function),
            "call_expression" => Self::go_callee(function.child_by_field_name("function")?),
            _ => None,
        }
    }

    /// Tells if the parameter list of the function is the parameter list of a handler of
    /// the framework: `(http.ResponseWriter, *http.Request)`, `(*gin.Context)`, or
    /// `(echo.Context) error`. The imports of the file of the function give the qualifiers.
    fn is_go_handler_function(
        &self,
        symbol: &Symbol,
        file: &FileSymbols,
        kind: HandlerKind,
    ) -> bool {
        let Some(signature) = parse_signature(&symbol.signature, &symbol.name) else {
            return false;
        };
        let http = package_names(file, |module| module == "net/http", "http");
        let own = |framework: Framework| {
            package_names(file, |module| framework.owns(module), framework.package())
        };
        match (kind, signature.parameters.as_slice()) {
            (HandlerKind::Http, [writer, request]) => {
                signature.result.is_empty()
                    && is_qualified(writer, "ResponseWriter", &http, false)
                    && is_qualified(request, "Request", &http, true)
            }
            (HandlerKind::Gin, [context]) => {
                signature.result.is_empty()
                    && is_qualified(context, "Context", &own(Framework::Gin), true)
            }
            (HandlerKind::Echo, [context]) => {
                signature.result == "error"
                    && is_qualified(context, "Context", &own(Framework::Echo), false)
            }
            _ => false,
        }
    }

    /// Tells if the function takes a handler and gives a handler, as a middleware does:
    /// `func(http.Handler) http.Handler`, or a function that returns such a function.
    fn is_go_middleware_function(
        &self,
        symbol: &Symbol,
        file: &FileSymbols,
        kind: HandlerKind,
    ) -> bool {
        let Some(signature) = parse_signature(&symbol.signature, &symbol.name) else {
            return false;
        };
        let http = package_names(file, |module| module == "net/http", "http");
        let gin = package_names(
            file,
            |module| Framework::Gin.owns(module),
            Framework::Gin.package(),
        );
        let echo = package_names(
            file,
            |module| Framework::Echo.owns(module),
            Framework::Echo.package(),
        );
        let is_handler_type = |text: &str| match kind {
            HandlerKind::Http => {
                is_qualified(text, "Handler", &http, false)
                    || is_qualified(text, "HandlerFunc", &http, false)
            }
            HandlerKind::Gin => is_qualified(text, "HandlerFunc", &gin, false),
            HandlerKind::Echo => {
                is_qualified(text, "HandlerFunc", &echo, false)
                    || is_qualified(text, "MiddlewareFunc", &echo, false)
            }
        };
        let mut inputs = signature.parameters;
        let mut output = signature.result;
        if let Some(inner) = parse_function_type(&output) {
            inputs.extend(inner.parameters);
            output = inner.result;
        }
        is_handler_type(output.trim()) && inputs.iter().any(|input| is_handler_type(input))
    }

    /// Tells if a wrapper call names a function of the repository with the shape of a middleware.
    fn is_go_middleware(&self, function: Node<'_>, kind: HandlerKind) -> bool {
        Self::go_callee(function)
            .and_then(|callee| self.go_symbol(callee))
            .is_some_and(|(symbol, file)| self.is_go_middleware_function(symbol, file, kind))
    }

    /// The function that a handler argument names, and each wrapper call around it. A
    /// function inside a wrapper is the handler only when a conversion proves it, as
    /// `http.HandlerFunc(show)` does, or when its parameter list is the parameter list of
    /// a handler. A wrapper with more than one such function gives no function. A wrapper
    /// is a guard only when the repository defines it with the shape of a middleware.
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
                        .filter(|(symbol, file)| self.is_go_handler_function(symbol, file, kind))
                        .map(|(symbol, _)| symbol.key.to_string())
                };
                Handler {
                    function_like: symbol.is_some(),
                    symbol,
                    guards: Vec::new(),
                }
            }
            "func_literal" => Handler {
                function_like: direct,
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
        let from_net_http = package_names(self.file, |module| module == "net/http", "http")
            .iter()
            .any(|name| name == operand);
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

/// The factories of the repository with a call that attaches the result, or that the
/// walk cannot classify. A factory is a function or a method that returns a router. A call
/// serves the result as the root, or it attaches it to a parent. One pass over the files
/// that call a factory gives the facts for the whole analysis.
#[derive(Default)]
pub(super) struct CallSites {
    /// The keys of the symbols of the factories.
    blocked: HashSet<String>,
}

impl CallSites {
    pub(super) fn is_blocked(&self, key: &str) -> bool {
        self.blocked.contains(key)
    }

    /// The facts for all Go files. The walk of each file that imports a framework finds the
    /// factories. Then one pass over the references and one pass over the syntax of each file
    /// classify each use of a factory.
    pub(super) fn build(files: &[FileSymbols], trees: &dyn TreeSource) -> Self {
        let mut factories: HashSet<String> = HashSet::new();
        for (position, file) in files.iter().enumerate() {
            if file.language != Language::Go {
                continue;
            }
            let Some(tree) = trees.tree(position) else {
                continue;
            };
            let context = Context::new(&tree, file, files, trees, &[]);
            factories.extend(factory_keys(&context, tree.root_node()));
        }
        let mut blocked = HashSet::new();
        if factories.is_empty() {
            return Self { blocked };
        }
        let mut by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for symbol in files
            .iter()
            .filter(|file| file.language == Language::Go)
            .flat_map(|file| &file.symbols)
            .filter(|symbol| factories.contains(&*symbol.key))
        {
            by_name.entry(&symbol.name).or_default().push(&symbol.key);
        }
        for (position, file) in files.iter().enumerate() {
            if file.language != Language::Go {
                continue;
            }
            let sites: Vec<(&Reference, Vec<&str>)> = file
                .references
                .iter()
                .filter_map(|reference| {
                    let keys: Vec<&str> = reference
                        .resolution
                        .candidates
                        .iter()
                        .map(|key| &**key)
                        .filter(|key| factories.contains(*key))
                        .collect();
                    (!keys.is_empty()).then_some((reference, keys))
                })
                .collect();
            let Some(tree) = trees.tree(position) else {
                blocked.extend(
                    sites
                        .iter()
                        .flat_map(|(_, keys)| keys)
                        .map(|key| key.to_string()),
                );
                continue;
            };
            blocked.extend(Self::mentions(tree.root_node(), &tree.source, &by_name));
            if sites.is_empty() {
                continue;
            }
            let index = FileUses::new(tree.root_node(), &tree.source, file);
            for (reference, keys) in sites {
                if index.is_blocked(
                    reference.name_range.start_byte,
                    reference.name_range.end_byte,
                ) {
                    blocked.extend(keys.into_iter().map(str::to_string));
                }
            }
        }
        Self { blocked }
    }

    /// The keys of the factories that a name in the file mentions without a call. A function
    /// value, a method value, and an argument that is a function are such mentions. A
    /// mention can name another function of the same name, and the result stays safe.
    fn mentions(root: Node<'_>, source: &[u8], by_name: &HashMap<&str, Vec<&str>>) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if matches!(node.kind(), "identifier" | "field_identifier") {
                let name = std::str::from_utf8(&source[node.byte_range()]).unwrap_or("");
                if let Some(keys) = by_name.get(name) {
                    if !Self::is_declaration_or_callee(node) {
                        found.extend(keys.iter().map(|key| key.to_string()));
                    }
                }
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        found
    }

    fn is_declaration_or_callee(node: Node<'_>) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        match parent.kind() {
            "function_declaration" | "method_declaration" => {
                parent.child_by_field_name("name") == Some(node)
            }
            "call_expression" => parent.child_by_field_name("function") == Some(node),
            "selector_expression" => {
                parent.child_by_field_name("field") == Some(node)
                    && parent.parent().is_some_and(|call| {
                        call.kind() == "call_expression"
                            && call.child_by_field_name("function") == Some(parent)
                    })
            }
            _ => false,
        }
    }
}

/// The uses of the variables of one file that the classification of a call site needs.
struct FileUses<'t> {
    root: Node<'t>,
    source: &'t [u8],
    http: Vec<String>,
    /// The start of each use of a name that is not a use as a router, by name, in order.
    bad: HashMap<String, Vec<usize>>,
}

impl<'t> FileUses<'t> {
    fn new(root: Node<'t>, source: &'t [u8], file: &FileSymbols) -> Self {
        let mut uses = Self {
            root,
            source,
            http: package_names(file, |module| module == "net/http", "http"),
            bad: HashMap::new(),
        };
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.kind() == "identifier" && uses.is_bad_use(node) {
                let name = uses.text(node).to_string();
                uses.bad.entry(name).or_default().push(node.start_byte());
            }
            let mut cursor = node.walk();
            let children: Vec<Node<'t>> = node.named_children(&mut cursor).collect();
            pending.extend(children.into_iter().rev());
        }
        uses
    }

    fn text(&self, node: Node<'t>) -> &'t str {
        std::str::from_utf8(&self.source[node.byte_range()]).unwrap_or("")
    }

    /// Tells if a call serves its argument as the root handler of a server.
    fn is_sink(&self, call: Option<Node<'t>>) -> bool {
        let Some(function) = call
            .filter(|call| call.kind() == "call_expression")
            .and_then(|call| call.child_by_field_name("function"))
            .filter(|function| function.kind() == "selector_expression")
        else {
            return false;
        };
        let (Some(operand), Some(field)) = (
            function.child_by_field_name("operand"),
            function.child_by_field_name("field"),
        ) else {
            return false;
        };
        matches!(
            self.text(field),
            "ListenAndServe" | "ListenAndServeTLS" | "Serve" | "ServeTLS"
        ) && self.http.iter().any(|name| name == self.text(operand))
    }

    /// Tells if a literal is an `http.Server`, which serves its handler as the root.
    fn is_server_literal(&self, literal: Node<'t>) -> bool {
        literal
            .child_by_field_name("type")
            .map(|kind| self.text(kind))
            .and_then(|kind| kind.split_once('.'))
            .is_some_and(|(package, name)| {
                name == "Server" && self.http.iter().any(|known| known == package)
            })
    }

    /// Tells if a use of a name is not a use as a router that the walk understands. A router
    /// that a variable holds is only a receiver of a call, or the root handler of a server.
    fn is_bad_use(&self, node: Node<'t>) -> bool {
        let mut child = node;
        let mut parent = child.parent();
        while let Some(found) = parent.filter(|found| found.kind() == "parenthesized_expression") {
            child = found;
            parent = found.parent();
        }
        let Some(parent) = parent else {
            return false;
        };
        match parent.kind() {
            "selector_expression" => parent.child_by_field_name("operand") != Some(child),
            "argument_list" => !self.is_sink(parent.parent()),
            "var_spec" => parent.child_by_field_name("value") == Some(child),
            "expression_list" => {
                let Some(grand) = parent.parent() else {
                    return false;
                };
                match grand.kind() {
                    "short_var_declaration" | "assignment_statement" => {
                        grand.child_by_field_name("left") != Some(parent)
                    }
                    "var_spec" => grand.child_by_field_name("value") == Some(parent),
                    _ => true,
                }
            }
            "literal_element" | "keyed_element" | "literal_value" => {
                let mut up = parent.parent();
                while let Some(found) = up.filter(|found| found.kind() != "composite_literal") {
                    up = found.parent();
                }
                !up.is_some_and(|literal| self.is_server_literal(literal))
            }
            "block" | "statement_list" | "expression_statement" | "source_file" => false,
            _ => true,
        }
    }

    /// Tells if the call site that has the name at these bytes attaches the result, or has
    /// a use that the walk cannot classify.
    fn is_blocked(&self, start: usize, end: usize) -> bool {
        let Some(mut node) = self.root.descendant_for_byte_range(start, end) else {
            return true;
        };
        let call = loop {
            if node.kind() == "call_expression"
                && node
                    .child_by_field_name("function")
                    .is_some_and(|function| function.end_byte() == end)
            {
                break node;
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => return true,
            }
        };
        let mut value = call;
        let mut parent = value.parent();
        while let Some(found) = parent.filter(|found| found.kind() == "parenthesized_expression") {
            value = found;
            parent = found.parent();
        }
        let Some(parent) = parent else {
            return true;
        };
        match parent.kind() {
            "expression_statement" => false,
            "selector_expression" => parent.child_by_field_name("operand") != Some(value),
            "argument_list" => !self.is_sink(parent.parent()),
            "literal_element" | "keyed_element" => {
                let mut up = parent.parent();
                while let Some(found) = up.filter(|found| found.kind() != "composite_literal") {
                    up = found.parent();
                }
                !up.is_some_and(|literal| self.is_server_literal(literal))
            }
            "expression_list" => self.is_blocked_assignment(value, parent),
            "var_spec" => true,
            _ => true,
        }
    }

    /// A call in a list of values: a local variable that only receives calls, or the blank name.
    fn is_blocked_assignment(&self, value: Node<'t>, list: Node<'t>) -> bool {
        let Some(declaration) = list.parent() else {
            return true;
        };
        let kind = declaration.kind();
        if !matches!(
            kind,
            "short_var_declaration" | "var_spec" | "assignment_statement"
        ) {
            return true;
        }
        let mut cursor = list.walk();
        let values: Vec<Node<'t>> = list.named_children(&mut cursor).collect();
        let Some(position) = values.iter().position(|candidate| *candidate == value) else {
            return true;
        };
        let mut cursor = declaration.walk();
        let names: Vec<Node<'t>> = if kind == "var_spec" {
            declaration
                .children_by_field_name("name", &mut cursor)
                .collect()
        } else {
            let Some(left) = declaration.child_by_field_name("left") else {
                return true;
            };
            let mut inner = left.walk();
            left.named_children(&mut inner).collect()
        };
        let Some(target) = names.get(position).filter(|_| names.len() == values.len()) else {
            return true;
        };
        let name = self.text(*target);
        if name == "_" {
            return false;
        }
        if kind == "assignment_statement" {
            return true;
        }
        let mut block = declaration.parent();
        while let Some(found) = block.filter(|found| found.kind() != "block") {
            block = found.parent();
        }
        let Some(block) = block else {
            return true;
        };
        let (from, to) = (declaration.end_byte(), block.end_byte());
        self.bad.get(name).is_some_and(|starts| {
            let first = starts.partition_point(|start| *start <= from);
            starts.get(first).is_some_and(|start| *start < to)
        })
    }
}

/// The keys of the functions and the methods of a file that return a router that they made.
/// The walk does not read the calls of the repository for this result.
pub(super) fn factory_keys(context: &Context<'_>, root: Node<'_>) -> Vec<String> {
    match Routes::new(context, true) {
        Some(mut routes) => {
            routes.run(root);
            routes.factory_keys
        }
        None => Vec::new(),
    }
}

pub(super) fn collect(context: &Context<'_>, root: Node<'_>, out: &mut Vec<Draft>) {
    if let Some(mut routes) = Routes::new(context, false) {
        routes.run(root);
        routes.finish(out);
    }
}

impl<'c, 'a, 't> Routes<'c, 'a, 't> {
    fn new(context: &'c Context<'a>, dry: bool) -> Option<Self> {
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
            return None;
        }
        Some(Self {
            context,
            packages,
            routers: Vec::new(),
            records: Vec::new(),
            scopes: vec![HashMap::new()],
            consumed: HashSet::new(),
            route_calls: HashMap::new(),
            functions: HashMap::new(),
            walked: HashSet::new(),
            in_progress: HashSet::new(),
            cyclic: HashSet::new(),
            summaries: HashMap::new(),
            fresh_of: HashMap::new(),
            components: HashMap::new(),
            attached: HashSet::new(),
            owners: 0,
            frames: Vec::new(),
            dry,
            factory_keys: Vec::new(),
        })
    }
}

struct Routes<'c, 'a, 't> {
    context: &'c Context<'a>,
    packages: Vec<(String, Framework)>,
    routers: Vec<Router>,
    records: Vec<Record>,
    scopes: Vec<HashMap<String, Option<usize>>>,
    consumed: HashSet<usize>,
    /// The router that a call of `Route` made, by the node of the call.
    route_calls: HashMap<usize, usize>,
    functions: HashMap<String, Node<'t>>,
    /// The functions that the walk read. A call can read a function before its declaration.
    walked: HashSet<String>,
    /// The functions that the walk reads at this time. A recursive call has no summary.
    in_progress: HashSet<String>,
    /// The functions that a call of themselves reaches. They have no summary.
    cyclic: HashSet<String>,
    /// The summary of each function that the walk read, or none.
    summaries: HashMap<String, Option<Summary>>,
    /// The factory that made a router for a call, by router.
    fresh_of: HashMap<usize, String>,
    /// The routers of the returned router of each factory, and the routers that join to it.
    components: HashMap<String, Vec<usize>>,
    /// The factories whose result a call can attach to a parent. Their own routes have
    /// paths that depend on the call.
    attached: HashSet<String>,
    owners: usize,
    frames: Vec<Frame>,
    /// The walk only finds the factories. It does not read the calls of the repository.
    dry: bool,
    factory_keys: Vec<String>,
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
        for name in self.assigned_globals(root) {
            if self.scopes[0].contains_key(&name) {
                self.scopes[0].insert(name, None);
            }
        }
        for child in children
            .iter()
            .filter(|child| child.kind() != "var_declaration")
        {
            self.visit(*child);
        }
    }

    /// The package-level variables that an assignment inside a function writes. The walk
    /// does not know when the function runs, so it does not trust their value.
    fn assigned_globals(&self, root: Node<'t>) -> HashSet<String> {
        let mut globals = HashSet::new();
        for declaration in Self::named_children(root)
            .into_iter()
            .filter(|child| child.kind() == "var_declaration")
        {
            for spec in Self::named_children(declaration) {
                let mut cursor = spec.walk();
                for name in spec.children_by_field_name("name", &mut cursor) {
                    globals.insert(self.text(name).to_string());
                }
            }
        }
        let mut written = HashSet::new();
        let mut pending: Vec<Node<'t>> = Self::named_children(root)
            .into_iter()
            .filter(|child| child.kind() != "var_declaration")
            .collect();
        while let Some(node) = pending.pop() {
            if node.kind() == "assignment_statement" {
                if let Some(left) = node.child_by_field_name("left") {
                    for target in Self::named_children(left) {
                        let name = self.text(target);
                        if target.kind() == "identifier" && globals.contains(name) {
                            written.insert(name.to_string());
                        }
                    }
                }
            }
            pending.extend(Self::named_children(node));
        }
        written
    }

    fn visit(&mut self, node: Node<'t>) {
        match node.kind() {
            "function_declaration" | "method_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .filter(|_| node.kind() == "function_declaration")
                    .map(|name| self.text(name));
                self.walk_declared(node, name);
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
            "composite_literal" => self.literal_escapes(node),
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

    /// Reads the body of a function in a scope of its own, one time. `declared` is the name
    /// of the function. A pure function that returns a router it made gets a summary.
    fn walk_declared(&mut self, node: Node<'t>, declared: Option<&str>) {
        if let Some(name) = declared {
            if !self.walked.insert(name.to_string()) {
                return;
            }
            self.in_progress.insert(name.to_string());
        }
        let globals = self.scopes[0].clone();
        let saved = std::mem::replace(&mut self.scopes, vec![globals, HashMap::new()]);
        self.owners += 1;
        let owner = self.owners;
        self.frames.push(Frame {
            function: true,
            returned: None,
            owner,
            pure: true,
            created: Vec::new(),
        });
        self.bind_parameters(node.child_by_field_name("parameters"), None);
        if let Some(body) = node.child_by_field_name("body") {
            self.visit(body);
        }
        let frame = self.frames.pop();
        if let Some(name) = declared {
            self.in_progress.remove(name);
        }
        let globals = self.scopes.swap_remove(0);
        self.scopes = saved;
        self.scopes[0] = globals;
        let Some(frame) = frame else {
            return;
        };
        let key = node
            .child_by_field_name("name")
            .and_then(|name| self.context.symbol_at(name));
        let summary = frame
            .returned
            .filter(|_| frame.pure && declared.is_some_and(|name| !self.cyclic.contains(name)))
            .and_then(|router| self.summarize(router, owner));
        let comp = frame
            .returned
            .filter(|router| self.made_by(*router, owner))
            .map(|router| self.component(router, &frame.created));
        let id = declared.map(str::to_string).or_else(|| key.clone());
        if let (Some(id), Some(comp)) = (id, comp) {
            let needs_a_summary = declared.is_some()
                && summary
                    .as_ref()
                    .is_none_or(|summary| !summary.prefixes.is_empty());
            if let Some(key) = &key {
                self.factory_keys.push(key.clone());
            }
            let attached_in_the_repository = !self.dry
                && key
                    .as_deref()
                    .is_some_and(|key| self.context.go_factory_blocked(key));
            if needs_a_summary || attached_in_the_repository {
                self.attached.insert(id.clone());
            }
            self.components.insert(id, comp);
        }
        if let Some(name) = declared {
            self.summaries.insert(name.to_string(), summary);
        }
    }

    /// The owner of the function body that the walk reads at this time.
    fn current_owner(&self) -> usize {
        self.frames.last().map_or(0, |frame| frame.owner)
    }

    /// The function that the walk reads at this time changes a value that it did not make.
    fn mark_impure(&mut self) {
        if let Some(frame) = self.frames.iter_mut().rev().find(|frame| frame.function) {
            frame.pure = false;
        }
    }

    /// Notes that the walk changes a router. A function that changes a router that it did
    /// not make has no summary.
    fn touch(&mut self, router: usize) {
        let router = &self.routers[router];
        if router.external || router.owner != self.current_owner() {
            self.mark_impure();
        }
    }

    /// Notes that a router that a factory made leaves the proof of the walk. A call can
    /// then attach it to a parent, so the paths of the routes of the factory depend on the call.
    fn escape_router(&mut self, router: usize) {
        if let Some(name) = self.fresh_of.get(&router) {
            self.attached.insert(name.clone());
        }
    }

    /// Notes the use of an expression as a value, as an argument, a returned value, or a
    /// stored value. A router that a factory gives and a call of a factory escape here.
    fn escape(&mut self, expression: Node<'t>) {
        match expression.kind() {
            "identifier" => {
                if let Some(Some(router)) = self.lookup(self.text(expression)) {
                    self.escape_router(router);
                }
            }
            "call_expression" => {
                let name = expression
                    .child_by_field_name("function")
                    .filter(|function| function.kind() == "identifier")
                    .map(|function| self.text(function));
                if let Some(name) = name.filter(|name| self.functions.contains_key(*name)) {
                    self.attached.insert(name.to_string());
                }
            }
            "parenthesized_expression" | "unary_expression" => {
                if let Some(inner) = expression.named_child(0) {
                    self.escape(inner);
                }
            }
            _ => {}
        }
    }

    /// Tells if a call serves its argument as the root handler of a server, as
    /// `http.ListenAndServe(":80", router)` does.
    fn is_root_sink(&self, call: Node<'t>) -> bool {
        let Some(function) = call
            .child_by_field_name("function")
            .filter(|function| function.kind() == "selector_expression")
        else {
            return false;
        };
        let (Some(operand), Some(field)) = (
            function.child_by_field_name("operand"),
            function.child_by_field_name("field"),
        ) else {
            return false;
        };
        matches!(
            self.text(field),
            "ListenAndServe" | "ListenAndServeTLS" | "Serve" | "ServeTLS"
        ) && self.is_net_http(self.text(operand))
    }

    fn is_net_http(&self, package: &str) -> bool {
        package_names(self.context.file, |module| module == "net/http", "http")
            .iter()
            .any(|name| name == package)
    }

    /// The values of a literal escape, unless the literal is an `http.Server`. A server
    /// serves its handler as the root.
    fn literal_escapes(&mut self, literal: Node<'t>) {
        let is_server = literal
            .child_by_field_name("type")
            .map(|kind| self.text(kind))
            .and_then(|kind| kind.split_once('.'))
            .is_some_and(|(package, name)| name == "Server" && self.is_net_http(package));
        if is_server {
            return;
        }
        let Some(body) = literal.child_by_field_name("body") else {
            return;
        };
        let mut values = Vec::new();
        Self::literal_values(body, &mut values);
        for value in values {
            self.escape(value);
        }
    }

    fn literal_values(node: Node<'t>, out: &mut Vec<Node<'t>>) {
        match node.kind() {
            "identifier" | "call_expression" => out.push(node),
            "literal_value"
            | "literal_element"
            | "parenthesized_expression"
            | "unary_expression" => {
                for child in Self::named_children(node) {
                    Self::literal_values(child, out);
                }
            }
            "keyed_element" => {
                if let Some(value) = Self::named_children(node).pop() {
                    Self::literal_values(value, out);
                }
            }
            _ => {}
        }
    }

    /// The routers that a function body made, and that join to the returned router.
    fn component(&self, returned: usize, created: &[usize]) -> Vec<usize> {
        let mut neighbors: HashMap<usize, Vec<usize>> = HashMap::new();
        let made: HashSet<usize> = created.iter().copied().collect();
        for router in created {
            for link in &self.routers[*router].links {
                if made.contains(&link.parent) {
                    neighbors.entry(*router).or_default().push(link.parent);
                    neighbors.entry(link.parent).or_default().push(*router);
                }
            }
        }
        let mut seen: HashSet<usize> = HashSet::from([returned]);
        let mut pending = vec![returned];
        while let Some(router) = pending.pop() {
            for next in neighbors.get(&router).into_iter().flatten() {
                if seen.insert(*next) {
                    pending.push(*next);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Tells if a function body made the router, and each router that it joins to.
    fn made_by(&self, router: usize, owner: usize) -> bool {
        let router = &self.routers[router];
        router.owner == owner
            && !router.external
            && router
                .links
                .iter()
                .all(|link| self.made_by(link.parent, owner))
    }

    fn summarize(&self, router: usize, owner: usize) -> Option<Summary> {
        if !self.made_by(router, owner) {
            return None;
        }
        let mut places = self.lineage(router, None);
        let place = places.pop().filter(|_| places.is_empty())?;
        Some(Summary {
            framework: self.routers[router].framework,
            prefixes: place.prefixes,
            guards: place.guards,
        })
    }

    /// The router that a call of a function of the file gives. A function with a summary
    /// gives a new router. Another function gives no router, because the walk has no proof.
    fn call_function(&mut self, name: &str) -> Option<usize> {
        let node = *self.functions.get(name)?;
        node.child_by_field_name("result")?;
        if self.in_progress.contains(name) {
            self.cyclic.insert(name.to_string());
            return None;
        }
        self.walk_declared(node, Some(name));
        let summary = self.summaries.get(name)?.clone()?;
        let id = self.make_root(summary.framework);
        let router = &mut self.routers[id];
        router.guards = summary.guards;
        router.mountable = summary.prefixes.is_empty();
        router.base = summary.prefixes;
        self.fresh_of.insert(id, name.to_string());
        Some(id)
    }

    fn closure(&mut self, literal: Node<'t>, bound: Option<usize>) {
        self.scopes.push(HashMap::new());
        let owner = self.current_owner();
        self.frames.push(Frame {
            function: false,
            returned: None,
            owner,
            pure: true,
            created: Vec::new(),
        });
        self.bind_parameters(literal.child_by_field_name("parameters"), bound);
        if let Some(body) = literal.child_by_field_name("body") {
            self.visit(body);
        }
        self.frames.pop();
        self.scopes.pop();
    }

    /// The function that a framework calls with a new router. Only a literal gives routes.
    /// A function that a name gives has an unknown prefix, because the walk reads each
    /// function one time.
    fn callback(&mut self, function: Node<'t>, router: usize) {
        if function.kind() == "func_literal" {
            self.consumed.insert(function.id());
            self.closure(function, Some(router));
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
        self.routers[router].external = true;
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
        if index == 0 && !self.frames.is_empty() {
            self.mark_impure();
            if let Some(router) = router {
                self.escape_router(router);
            }
            return;
        }
        self.scopes[index].insert(name.to_string(), router);
    }

    fn make_root(&mut self, framework: Framework) -> usize {
        self.routers.push(Router {
            framework,
            links: Vec::new(),
            guards: Vec::new(),
            pre: Vec::new(),
            retroactive: matches!(framework, Framework::Chi | Framework::Mux | Framework::Echo),
            partial: false,
            mountable: true,
            owner: self.current_owner(),
            external: false,
            base: Vec::new(),
        });
        let id = self.routers.len() - 1;
        if let Some(frame) = self.frames.iter_mut().rev().find(|frame| frame.function) {
            frame.created.push(id);
        }
        id
    }

    fn make_child(&mut self, parent: usize, prefix: String, guards: Vec<String>) -> usize {
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
            retroactive: matches!(framework, Framework::Chi | Framework::Mux),
            partial: self.routers[parent].partial,
            mountable: false,
            owner: self.current_owner(),
            external: false,
            base: Vec::new(),
        });
        let id = self.routers.len() - 1;
        if let Some(frame) = self.frames.iter_mut().rev().find(|frame| frame.function) {
            frame.created.push(id);
        }
        id
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
            } else if let Some(router) = router {
                self.escape_router(router);
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
        self.escape(*value);
        let router = self.eval(*value);
        let router = router.or_else(|| self.wrapped_router(*value));
        if let Some(frame) = self.frames.last_mut().filter(|frame| frame.function) {
            frame.returned = frame.returned.or(router);
        }
    }

    /// The router inside a literal that a function returns, as `&Server{Mux: router}` has.
    fn wrapped_router(&mut self, value: Node<'t>) -> Option<usize> {
        let literal = if value.kind() == "unary_expression" {
            value.named_child(0)?
        } else {
            value
        };
        if literal.kind() != "composite_literal" {
            return None;
        }
        let mut values = Vec::new();
        Self::literal_values(literal.child_by_field_name("body")?, &mut values);
        values.into_iter().find_map(|inner| self.eval(inner))
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
                None => self.call_function(name),
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
                Some(self.make_child(parent, String::new(), guards))
            }
            (Framework::Chi, "Route") => self.chi_route(parent, call, &arguments),
            (Framework::Chi, "Group") => self.chi_group(parent, call, &arguments),
            (Framework::Echo | Framework::Gin, "Group") => {
                let prefix = self.string_argument(*arguments.first()?)?;
                let guards = self.guard_names(&arguments[1..]);
                Some(self.make_child(parent, prefix, guards))
            }
            // The methods of gin that add a route or a guard return the router, as the
            // interface `IRoutes` says.
            (
                Framework::Gin,
                "Use" | "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "Any"
                | "Handle" | "Match",
            ) => {
                if self.consumed.insert(call.id()) {
                    self.gin_call(parent, call, name, &arguments);
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

    /// The strings of `[]string{"GET", "POST"}`. Another expression gives none, because
    /// only a literal list of strings proves the values.
    fn string_list(&self, node: Node<'t>) -> Option<Vec<String>> {
        if node.kind() != "composite_literal"
            || self.text(node.child_by_field_name("type")?) != "[]string"
        {
            return None;
        }
        Self::named_children(node.child_by_field_name("body")?)
            .into_iter()
            .map(|element| {
                let element = if element.kind() == "literal_element" {
                    element.named_child(0)?
                } else {
                    element
                };
                self.string_argument(element)
            })
            .collect()
    }

    fn call(&mut self, node: Node<'t>) {
        if !self.is_root_sink(node) {
            for argument in Context::arguments_of(node) {
                self.escape(argument);
            }
        }
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
            (Framework::Gin, _) => self.gin_call(router, node, name, &arguments),
            (_, "Use") => {
                let guards = self.guard_names(&arguments);
                self.touch(router);
                self.routers[router].guards.extend(guards);
            }
            (Framework::Echo, "Pre") => {
                let guards = self.guard_names(&arguments);
                self.touch(router);
                self.routers[router].pre.extend(guards);
            }
            (Framework::Chi, _) => self.chi_call(router, node, name, &arguments),
            (Framework::Echo, _) => self.echo_call(router, node, name, &arguments),
            (Framework::Mux, _) => {}
        }
    }

    /// `Route` calls the function with a new router and gives that router. The function
    /// runs before chi mounts the router.
    fn chi_route(
        &mut self,
        router: usize,
        call: Node<'t>,
        arguments: &[Node<'t>],
    ) -> Option<usize> {
        if let Some(child) = self.route_calls.get(&call.id()) {
            return Some(*child);
        }
        let [prefix, callback] = arguments else {
            return None;
        };
        let prefix = self.string_argument(*prefix)?;
        let child = self.make_child(router, prefix, Vec::new());
        self.route_calls.insert(call.id(), child);
        self.callback(*callback, child);
        Some(child)
    }

    fn chi_group(
        &mut self,
        router: usize,
        call: Node<'t>,
        arguments: &[Node<'t>],
    ) -> Option<usize> {
        if let Some(child) = self.route_calls.get(&call.id()) {
            return Some(*child);
        }
        let [callback] = arguments else {
            return None;
        };
        let child = self.make_child(router, String::new(), Vec::new());
        self.route_calls.insert(call.id(), child);
        self.callback(*callback, child);
        Some(child)
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
                | "Trace" | "Query",
                [_, handler],
                [Some(path), _],
            ) => {
                let method = name.to_uppercase();
                self.register(router, &[method.as_str()], path, Some(*handler), &[], range)
            }
            ("HandleFunc" | "Handle", [_, handler], [Some(path), _]) => {
                self.register(router, &["ANY"], path, Some(*handler), &[], range)
            }
            ("Method" | "MethodFunc", [_, _, handler], [Some(method), Some(path), _]) => self
                .register(
                    router,
                    &[method.to_uppercase().as_str()],
                    path,
                    Some(*handler),
                    &[],
                    range,
                ),
            ("Route", _, _) => {
                self.chi_route(router, node, arguments);
            }
            ("Group", _, _) => {
                self.chi_group(router, node, arguments);
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
        Some(self.make_child(parent, prefix, Vec::new()))
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
                        | "Handler"
                        | "Handle"
                )
            })
    }

    /// A route of a gorilla router: `router.HandleFunc("/path", handler)`, or a chain of
    /// matchers that ends in `HandlerFunc` or `Handler`. The library applies each matcher.
    /// Thus the path matchers join in the order of the calls, and a request must match each
    /// `Methods` list. Without `Methods`, the route has each method. A chain with no path,
    /// a path that is not a literal, or a `Methods` argument that is not a literal gives no
    /// entry. Returns true when the call is a registration of a gorilla router. The call
    /// consumes its chain.
    fn mux_registration(&mut self, outer: Node<'t>) -> bool {
        let mut chain = Vec::new();
        let mut method_lists: Vec<Option<Vec<String>>> = Vec::new();
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
                    method_lists.push(
                        (names.len() == arguments.len())
                            .then(|| names.iter().map(|name| name.to_uppercase()).collect()),
                    );
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
        paths.reverse();
        let Some(paths) = paths.into_iter().collect::<Option<Vec<String>>>() else {
            return true;
        };
        let Some((last, earlier)) = paths.split_last() else {
            return true;
        };
        let Some(path) = Framework::Mux.compose(earlier, last) else {
            return true;
        };
        let mut methods: Option<Vec<String>> = None;
        for list in method_lists {
            let Some(list) = list else {
                return true;
            };
            methods = Some(match methods {
                None => list,
                Some(known) => known
                    .into_iter()
                    .filter(|method| list.contains(method))
                    .collect(),
            });
        }
        let methods = methods.unwrap_or_else(|| vec!["ANY".to_string()]);
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
        self.register(
            router,
            &methods,
            &path,
            Some(handler),
            &[],
            Span::of(&outer),
        );
        true
    }

    /// `router.GET("/path", guard, handler)`. The last argument is the handler, and the
    /// arguments before it are guards. `Use` adds guards for the routes that follow.
    fn gin_call(&mut self, router: usize, node: Node<'t>, name: &str, arguments: &[Node<'t>]) {
        let (methods, rest): (Vec<String>, &[Node<'t>]) = match (name, arguments) {
            ("Use", _) => {
                let guards = self.guard_names(arguments);
                self.touch(router);
                self.routers[router].guards.extend(guards);
                return;
            }
            ("GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS", _) => {
                (vec![name.to_string()], arguments)
            }
            ("Any", _) => (GIN_ANY.map(String::from).to_vec(), arguments),
            ("Handle", [method, rest @ ..]) => match self.string_argument(*method) {
                Some(method) => (vec![method], rest),
                None => return,
            },
            ("Match", [methods, rest @ ..]) => match self.string_list(*methods) {
                Some(methods) => (methods, rest),
                None => return,
            },
            _ => return,
        };
        let [path, guards @ .., handler] = rest else {
            return;
        };
        let Some(path) = self.string_argument(*path) else {
            return;
        };
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
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
            ("Any", [_, ..]) => (ECHO_ANY.map(String::from).to_vec(), arguments),
            ("Match", [methods, rest @ ..]) => match self.string_list(*methods) {
                Some(methods) => (methods, rest),
                None => return,
            },
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
    /// more than one mount. The walk does not follow a router of another file.
    fn mount(&mut self, parent: usize, prefix: &str, mounted: Node<'t>) {
        let Some(child) = self.eval(mounted) else {
            return;
        };
        let known = self.routers[child]
            .links
            .iter()
            .any(|link| link.parent == parent && link.prefix == prefix);
        if !self.routers[child].mountable || known || self.reaches(parent, child) {
            return;
        }
        let link = Link {
            parent,
            parent_len: self.routers[parent].guards.len(),
            prefix: prefix.to_string(),
        };
        self.touch(parent);
        self.touch(child);
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
        self.touch(router);
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
                method: method.to_string(),
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
            return vec![Lineage {
                prefixes: router.base.clone(),
                guards: own,
            }];
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
        let blocked: HashSet<usize> = self
            .attached
            .iter()
            .filter_map(|name| self.components.get(name))
            .flatten()
            .copied()
            .collect();
        for record in self
            .records
            .iter()
            .filter(|record| !blocked.contains(&record.router))
        {
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
                    route: Some(Route {
                        method: record.method.clone(),
                        parameters: route_parameters(&path),
                        path,
                    }),
                    guards,
                    basis: EntryBasis::Registered,
                });
            }
        }
    }
}
