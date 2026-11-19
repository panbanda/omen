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
//!
//! The walk does not recognise these forms, so they give no entry. They are coverage gaps.
//! The first is a route that a caller registers on the result of a factory of another file.
//! The others are an echo group that `Host` makes, a gorilla/mux route that `NewRoute()`
//! builds, and the gin methods `Static`, `StaticFile`, `StaticFileFS`, and `StaticFS`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use tree_sitter::Node;

use super::{
    syntax, Context, Draft, EntryBasis, EntryKind, GuardsUnknownReason, Route, TreeSource,
};
use crate::core::Language;
use crate::index::extract::FileSymbols;
use crate::index::model::{ImportKind, ImportTarget, Reference, Span, Symbol, SymbolKind};

/// The methods that `Any` registers. gin v1.11.0 `anyMethods`.
/// The guard text of a middleware argument that is not a name. An entry point with this guard
/// has a guard list that is not complete.
pub(super) const UNREAD_GUARD: &str = "<expression>";

/// The guard text of a middleware that is a function literal. It has no name, so the guard list
/// is not complete.
pub(super) const UNNAMED_FUNCTION_GUARD: &str = "<function literal>";

const GIN_ANY: [&str; 9] = [
    "GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS", "DELETE", "CONNECT", "TRACE",
];

/// The methods that `Any` registers. echo v4.13.4 `methods`.
/// The depth of a chain of calls that the walk reads.
const EVAL_DEPTH_LIMIT: usize = 1000;

pub(super) const ECHO_ANY: [&str; 11] = [
    "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PROPFIND", "PUT", "TRACE",
    "REPORT",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Framework {
    Chi,
    Echo,
    Mux,
    Gin,
}

impl Framework {
    pub(super) const ALL: [Self; 4] = [Self::Chi, Self::Echo, Self::Mux, Self::Gin];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Chi => "chi",
            Self::Echo => "echo",
            Self::Mux => "gorilla/mux",
            Self::Gin => "gin",
        }
    }

    /// The package name of the module, which an import without an alias binds.
    pub(super) fn package(self) -> &'static str {
        match self {
            Self::Chi => "chi",
            Self::Echo => "echo",
            Self::Mux => "mux",
            Self::Gin => "gin",
        }
    }

    pub(super) fn owns(self, module: &str) -> bool {
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

    pub(super) fn router_types(self) -> &'static [&'static str] {
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

    /// Adds one part to a joined path, with the rules of `compose`. A joined path is a
    /// persistent list, so a router shares the list of its parent and a route costs the
    /// length of its own part. None means that the library rejects the route.
    fn extend(self, state: &PrefixState, part: &str) -> Option<PrefixState> {
        if part.is_empty() && self != Self::Echo {
            return Some(state.clone());
        }
        let mut top = state.top.clone();
        let mut slash = state.slash;
        match self {
            Self::Chi | Self::Mux => {
                if !part.starts_with('/') {
                    return None;
                }
                while let Some(piece) = top.clone() {
                    let trimmed = piece.text.trim_end_matches('/');
                    if trimmed.len() == piece.text.len() {
                        break;
                    }
                    if trimmed.is_empty() {
                        top = piece.parent.clone();
                    } else {
                        top = Some(Rc::new(Piece {
                            text: trimmed.to_string(),
                            parent: piece.parent.clone(),
                        }));
                        break;
                    }
                }
                top = Some(Rc::new(Piece {
                    text: part.to_string(),
                    parent: top,
                }));
            }
            Self::Echo => {
                if !part.is_empty() {
                    top = Some(Rc::new(Piece {
                        text: part.to_string(),
                        parent: top,
                    }));
                }
            }
            Self::Gin => {
                for segment in part.split('/') {
                    match segment {
                        "" | "." => {}
                        ".." => top = top.and_then(|piece| piece.parent.clone()),
                        _ => {
                            top = Some(Rc::new(Piece {
                                text: segment.to_string(),
                                parent: top,
                            }));
                        }
                    }
                }
                slash = part.ends_with('/');
            }
        }
        Some(PrefixState { top, slash })
    }

    /// The path that a joined list gives.
    fn materialize(self, state: &PrefixState) -> Option<String> {
        let mut pieces: Vec<&str> = Vec::new();
        let mut current = state.top.as_ref();
        while let Some(piece) = current {
            pieces.push(&piece.text);
            current = piece.parent.as_ref();
        }
        pieces.reverse();
        match self {
            Self::Chi | Self::Mux => {
                let joined = pieces.concat();
                (!joined.is_empty()).then_some(joined)
            }
            Self::Echo => {
                let joined = pieces.concat();
                Some(match joined.as_str() {
                    "" => "/".to_string(),
                    text if text.starts_with('/') => joined,
                    _ => format!("/{joined}"),
                })
            }
            Self::Gin => {
                let mut joined = format!("/{}", pieces.join("/"));
                if state.slash && !joined.ends_with('/') {
                    joined.push('/');
                }
                Some(joined)
            }
        }
    }

    /// The path of a route, from the prefixes of its routers and its own path. The rules
    /// are the rules of each library. None means that the library rejects the route.
    pub(super) fn compose(self, prefixes: &[String], path: &str) -> Option<String> {
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
                // `Clean` of a join of two paths is the `Clean` of the joined text. Thus the
                // result needs one pass over the parts. Only the last part that is not empty
                // decides the slash at the end.
                let mut stack: Vec<&str> = Vec::new();
                let mut last: Option<&str> = None;
                for part in parts().filter(|part| !part.is_empty()) {
                    last = Some(part);
                    for segment in part.split('/') {
                        match segment {
                            "" | "." => {}
                            ".." => {
                                stack.pop();
                            }
                            _ => stack.push(segment),
                        }
                    }
                }
                let mut joined = format!("/{}", stack.join("/"));
                if last.is_some_and(|part| part.ends_with('/')) && !joined.ends_with('/') {
                    joined.push('/');
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
pub(super) struct Signature {
    pub(super) parameters: Vec<String>,
    pub(super) result: String,
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
pub(super) fn parse_signature(signature: &str, name: &str) -> Option<Signature> {
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
pub(super) fn package_names(
    file: &FileSymbols,
    owns: impl Fn(&str) -> bool,
    package: &str,
) -> Vec<String> {
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
    /// The number of records when the link was made. A later route of the parent can replace
    /// the mount.
    at: usize,
}

/// A step of the walk over the syntax tree.
enum Step<'t> {
    Read(Node<'t>),
    LeaveBlock,
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
    /// A chi router that `With` or `Group` made. It shares the tree of its parent.
    inline: bool,
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
    /// The route matches every path that starts with its path, as `PathPrefix` does.
    prefix: bool,
    /// The registration is one that omen cannot read. It can match any request.
    opaque: bool,
    /// The registration is real and has a path that omen reads, but omen omits its entry.
    omitted: bool,
    /// The record stands for a node that `Mount` of chi registers.
    mount: bool,
    /// The number of guards that the router had at the time of the registration.
    limit: usize,
    range: Span,
    symbol: Option<String>,
    method: String,
    path: String,
    guards: Vec<String>,
    /// A call around the handler can add a guard that omen does not read.
    unread_wrapper: bool,
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
    /// The routers that a return statement can return. Each return statement counts.
    returned: Vec<usize>,
    /// The return statements, and the return statements that have a router.
    returns: usize,
    router_returns: usize,
    /// The names of the named results. A bare return gives their values.
    results: Vec<String>,
    /// A return statement gave a literal that holds the router, not the router.
    wrapped: bool,
    /// The type of the first literal that held a router in a return statement.
    wrapper_type: Option<String>,
    owner: usize,
    pure: bool,
    /// The routers that the function body made.
    created: Vec<usize>,
}

#[derive(Default)]
pub(super) struct Handler {
    pub symbol: Option<String>,
    pub guards: Vec<String>,
    /// A call around the handler is not a conversion and not a middleware that omen reads. It can
    /// add a guard.
    pub unread_wrapper: bool,
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

    /// The patterns of the file with a final `{name...}` that have a sibling `{$}` and a sibling
    /// `{name}` at the same prefix. Together the siblings can take each request of the pattern,
    /// so the pattern gives no entry. Omen ignores the methods and the hosts here.
    pub(super) fn go_covered_patterns(&self) -> &HashSet<String> {
        self.go_covered.get_or_init(|| {
            let mut paths: HashSet<String> = HashSet::new();
            let mut pending = vec![self.parsed.root_node()];
            while let Some(node) = pending.pop() {
                let mut cursor = node.walk();
                pending.extend(node.named_children(&mut cursor));
                if node.kind() != "call_expression" {
                    continue;
                }
                let Some(function) = node
                    .child_by_field_name("function")
                    .filter(|function| function.kind() == "selector_expression")
                else {
                    continue;
                };
                let (Some(field), Some(receiver)) = (
                    function.child_by_field_name("field"),
                    function.child_by_field_name("operand"),
                ) else {
                    continue;
                };
                if !matches!(self.text(field), "HandleFunc" | "Handle")
                    || !self.imports("net.http")
                    || !self.is_go_http_receiver(receiver)
                    || self.go_in_dead_branch(node)
                {
                    continue;
                }
                let Some(first) = Self::arguments_of(node).first().copied() else {
                    continue;
                };
                if let Some((_, path)) = self
                    .go_string(first)
                    .and_then(|pattern| servemux_pattern(&pattern))
                {
                    paths.insert(path);
                }
            }
            let is_single = |segment: &str| {
                segment.starts_with('{')
                    && segment.ends_with('}')
                    && !segment.ends_with("...}")
                    && segment != "{$}"
            };
            let singles: HashSet<&str> = paths
                .iter()
                .filter_map(|other| other.rsplit_once('/'))
                .filter(|(_, tail)| is_single(tail))
                .map(|(head, _)| head)
                .collect();
            paths
                .iter()
                .filter(|path| {
                    let Some((prefix, last)) = path.rsplit_once('/') else {
                        return false;
                    };
                    last.starts_with('{')
                        && last.ends_with("...}")
                        && singles.contains(prefix)
                        && paths.contains(&format!("{prefix}/{{$}}"))
                })
                .cloned()
                .collect()
        })
    }

    /// Tells if the node is in a branch that never runs: the body of `if false`, the `else`
    /// branch of `if true`, or the body of `for false`.
    pub(super) fn go_in_dead_branch(&self, node: Node<'_>) -> bool {
        // One walk over the tree finds the branches that never run. A parent of a node costs
        // a walk down from the root, so the check does not climb to the parents.
        let (starts, ends) = self.go_dead.get_or_init(|| {
            let literal = |condition: Node<'_>| -> Option<bool> {
                if !matches!(condition.kind(), "identifier" | "true" | "false")
                    || self.is_go_shadowed(condition)
                {
                    return None;
                }
                match self.text(condition) {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }
            };
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            let mut pending = vec![self.parsed.root_node()];
            while let Some(current) = pending.pop() {
                let dead = match current.kind() {
                    "if_statement" => {
                        let value = current.child_by_field_name("condition").and_then(literal);
                        match value {
                            Some(true) => current.child_by_field_name("alternative"),
                            Some(false) => current.child_by_field_name("consequence"),
                            None => None,
                        }
                    }
                    "for_statement" => {
                        let never = current.named_child(0).and_then(literal) == Some(false);
                        current.child_by_field_name("body").filter(|_| never)
                    }
                    _ => None,
                };
                if let Some(dead) = dead {
                    ranges.push((dead.start_byte(), dead.end_byte()));
                }
                let mut cursor = current.walk();
                pending.extend(current.named_children(&mut cursor));
            }
            ranges.sort_unstable();
            let starts: Vec<usize> = ranges.iter().map(|(start, _)| *start).collect();
            let mut highest = 0;
            let ends: Vec<usize> = ranges
                .iter()
                .map(|(_, end)| {
                    highest = highest.max(*end);
                    highest
                })
                .collect();
            (starts, ends)
        });
        let before = starts.partition_point(|start| *start <= node.start_byte());
        before > 0 && ends[before - 1] >= node.end_byte()
    }

    /// Tells if a reference anywhere in the repository names the function.
    fn go_function_referenced(&self, key: &str) -> bool {
        self.all_parsed
            .shared()
            .go_call_sites
            .get_or_init(|| CallSites::build(self.all, self.all_parsed))
            .is_referenced(key)
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
        self.go_handler_target(node)
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
                    ..Handler::default()
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
                let middleware = self.is_go_middleware(function, kind);
                let mut inner: Vec<Handler> = arguments
                    .into_iter()
                    .map(|argument| self.go_handler_at(argument, kind, false))
                    .filter(|handler| handler.function_like)
                    .collect();
                if inner.is_empty() {
                    return Handler {
                        unread_wrapper: !middleware,
                        ..Handler::default()
                    };
                }
                let nested_unread = inner.iter().any(|handler| handler.unread_wrapper);
                let (symbol, nested) = match inner.pop() {
                    Some(only) if inner.is_empty() => (only.symbol, only.guards),
                    _ => (None, Vec::new()),
                };
                let mut guards = Vec::new();
                if middleware {
                    guards.extend(self.go_guard_name(function));
                }
                guards.extend(nested);
                Handler {
                    symbol,
                    guards,
                    unread_wrapper: !middleware || nested_unread,
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
        if self.is_go_shadowed(operand) {
            return false;
        }
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

/// The methods that chi v5.3.2 knows before a call of `RegisterMethod`.
const CHI_METHODS: [&str; 10] = [
    "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT", "QUERY", "TRACE",
];

/// The names of the parameters in the path of a route, by the syntax of the framework.
pub(super) fn path_parameters(framework: Framework, path: &str) -> Vec<String> {
    let bytes = path.as_bytes();
    let mut names = Vec::new();
    match framework {
        Framework::Chi | Framework::Mux => {
            let (mut depth, mut start) = (0_usize, 0_usize);
            for (index, byte) in bytes.iter().enumerate() {
                match byte {
                    b'{' => {
                        if depth == 0 {
                            start = index + 1;
                        }
                        depth += 1;
                    }
                    b'}' if depth > 0 => {
                        depth -= 1;
                        if depth == 0 {
                            let name = path[start..index].split(':').next().unwrap_or("");
                            names.push(name.to_string());
                        }
                    }
                    b'*' if depth == 0 && framework == Framework::Chi => {
                        names.push("*".to_string())
                    }
                    _ => {}
                }
            }
        }
        Framework::Echo | Framework::Gin => {
            let mut index = 0;
            while index < bytes.len() {
                let byte = bytes[index];
                let escaped = index > 0 && bytes[index - 1] == b'\\';
                if (byte == b':' && !escaped) || byte == b'*' {
                    let end = path[index + 1..]
                        .find('/')
                        .map_or(bytes.len(), |slash| index + 1 + slash);
                    let name = &path[index + 1..end];
                    if byte == b'*' && framework == Framework::Echo {
                        names.push("*".to_string());
                    } else if !name.is_empty() {
                        names.push(name.to_string());
                    }
                    index = end;
                } else {
                    index += 1;
                }
            }
        }
    }
    names
}

/// The method and the path of a `net/http` pattern, as the standard library of Go 1.24
/// reads it. A pattern that the library rejects or that never matches gives none.
pub(super) fn servemux_pattern(pattern: &str) -> Option<(String, String)> {
    let (method, rest) = match pattern.find([' ', '\t']) {
        Some(space) => (
            &pattern[..space],
            pattern[space + 1..].trim_start_matches([' ', '\t']),
        ),
        None => ("", pattern),
    };
    let token = |byte: u8| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte);
    if !method.is_empty() && !method.bytes().all(token) {
        return None;
    }
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    if host.contains('{') || path != gin_join("/", path) && path != "/" {
        return None;
    }
    let segments: Vec<&str> = path[1..].split('/').collect();
    let mut seen: HashSet<&str> = HashSet::new();
    for (position, segment) in segments.iter().enumerate() {
        let last = position + 1 == segments.len();
        if !segment.contains('{') {
            continue;
        }
        let inner = segment.strip_prefix('{')?.strip_suffix('}')?;
        if inner == "$" {
            if !last {
                return None;
            }
            continue;
        }
        let (name, multi) = match inner.strip_suffix("...") {
            Some(name) => (name, true),
            None => (inner, false),
        };
        let valid = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_');
        if (multi && !last) || !valid || !seen.insert(name) {
            return None;
        }
    }
    let method = if method.is_empty() { "ANY" } else { method };
    Some((method.to_string(), path.to_string()))
}

/// The names of the wildcards of a `net/http` path.
pub(super) fn servemux_parameters(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .filter(|inner| *inner != "$")
        .map(|inner| inner.strip_suffix("...").unwrap_or(inner).to_string())
        .collect()
}

/// The names that the imports of a file give to `net/http`.
pub(super) fn net_http_names(file: &FileSymbols) -> Vec<String> {
    package_names(file, |module| module == "net/http", "http")
}

/// The type aliases of a file: the name, the name of the base type, and whether the base type
/// is not in the repository or cannot be read as a name.
fn collect_aliases(root: Node<'_>, source: &[u8]) -> Vec<(String, Option<(String, bool)>)> {
    let text = |node: Node<'_>| std::str::from_utf8(&source[node.byte_range()]).unwrap_or("");
    let mut found = Vec::new();
    let mut cursor = root.walk();
    for declaration in root
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "type_declaration")
    {
        let mut inner = declaration.walk();
        for spec in declaration.named_children(&mut inner) {
            let is_alias = spec.kind() == "type_alias" || {
                let mut children = spec.walk();
                let alias = spec
                    .children(&mut children)
                    .any(|child| child.kind() == "=");
                alias
            };
            let (Some(name), Some(target)) = (
                spec.child_by_field_name("name"),
                spec.child_by_field_name("type"),
            ) else {
                continue;
            };
            if !is_alias {
                found.push((text(name).to_string(), None));
                continue;
            }
            let target = text(target).trim_start_matches(['*', '&']);
            let base = target.split('[').next().unwrap_or(target);
            let readable =
                !base.is_empty() && base.chars().all(|c| c.is_alphanumeric() || c == '_');
            found.push((text(name).to_string(), Some((base.to_string(), !readable))));
        }
    }
    found
}

/// The declarations of the types by package directory and name. Each value has the number
/// of declarations, and for an alias the base name and a flag for a base that is not a name.
type TypeTable = HashMap<(String, String), (usize, Option<(String, bool)>)>;

/// The base type of a type name after the aliases, or none for a cycle or for an alias to a
/// type that is not read as a name. The walk keeps the result of each name.
fn resolve_alias(
    types: &TypeTable,
    memo: &mut HashMap<(String, String), Option<String>>,
    package: &str,
    name: &str,
) -> Option<String> {
    let start = (package.to_string(), name.to_string());
    if let Some(known) = memo.get(&start) {
        return known.clone();
    }
    let mut path: Vec<(String, String)> = Vec::new();
    let mut in_path: HashSet<(String, String)> = HashSet::new();
    let mut current = start.clone();
    let result = loop {
        if let Some(known) = memo.get(&current) {
            break known.clone();
        }
        match types.get(&current) {
            None => break Some(current.1.clone()),
            Some((count, _)) if *count > 1 => break None,
            Some((_, None)) => break Some(current.1.clone()),
            Some((_, Some((_, true)))) => break None,
            Some((_, Some((target, false)))) => {
                if !in_path.insert(current.clone()) {
                    break None;
                }
                let next = (current.0.clone(), target.clone());
                path.push(current);
                current = next;
            }
        }
    };
    memo.insert(start, result.clone());
    for node in path {
        memo.insert(node, result.clone());
    }
    result
}

/// The name of a type without its pointer, its package, and its type arguments.
fn type_name(text: &str) -> &str {
    let text = text.trim_start_matches(['*', '&']);
    let text = text.split('[').next().unwrap_or(text);
    text.rsplit('.').next().unwrap_or(text)
}

/// Tells if a regular expression is in the grammar that Go and Rust read in the same way:
/// ASCII letters and digits, a backslash and an ASCII punctuation character, a dot, a class of
/// letters, digits, and ranges, the repetitions `*`, `+`, `?`, `{m}`, `{m,}`, and `{m,n}` with
/// numbers up to 1000 and an optional `?`, and `|`. A group, a flag, an anchor, a shorthand
/// class, and a Unicode class are not in the grammar.
fn common_regex(pattern: &str) -> bool {
    if !pattern.is_ascii() {
        return false;
    }
    let bytes = pattern.as_bytes();
    let mut index = 0;
    let mut quantifiable = false;
    let lazy = |index: &mut usize| {
        if bytes.get(*index) == Some(&b'?') {
            *index += 1;
        }
    };
    while index < bytes.len() {
        match bytes[index] {
            b'|' => {
                quantifiable = false;
                index += 1;
            }
            b'*' | b'+' | b'?' => {
                if !quantifiable {
                    return false;
                }
                index += 1;
                lazy(&mut index);
                quantifiable = false;
            }
            b'{' => {
                let Some(close) = pattern[index..].find('}') else {
                    return false;
                };
                if !quantifiable {
                    return false;
                }
                let inner = &pattern[index + 1..index + close];
                let number = |text: &str| {
                    (!text.is_empty()
                        && text.len() <= 4
                        && text.bytes().all(|byte| byte.is_ascii_digit()))
                    .then(|| text.parse::<u32>().ok())
                    .flatten()
                    .filter(|value| *value <= 1000)
                };
                let valid = match inner.split_once(',') {
                    None => number(inner).is_some(),
                    Some((low, "")) => number(low).is_some(),
                    Some((low, high)) => match (number(low), number(high)) {
                        (Some(low), Some(high)) => low <= high,
                        _ => false,
                    },
                };
                if !valid {
                    return false;
                }
                index += close + 1;
                lazy(&mut index);
                quantifiable = false;
            }
            b'\\' => {
                if !bytes.get(index + 1).is_some_and(u8::is_ascii_punctuation) {
                    return false;
                }
                index += 2;
                quantifiable = true;
            }
            b'[' => {
                let Some(end) = common_class(&bytes[index..]) else {
                    return false;
                };
                index += end;
                quantifiable = true;
            }
            b'(' | b')' | b']' | b'}' | b'^' | b'$' => return false,
            byte if byte.is_ascii_graphic() || byte == b' ' => {
                index += 1;
                quantifiable = true;
            }
            _ => return false,
        }
    }
    true
}

/// The length of a class `[...]` at the start of the bytes, when it has only letters, digits,
/// underscores, ranges, and escaped punctuation.
fn common_class(bytes: &[u8]) -> Option<usize> {
    let mut index = 1;
    if bytes.get(index) == Some(&b'^') {
        index += 1;
    }
    let mut items = 0;
    loop {
        let byte = *bytes.get(index)?;
        match byte {
            b']' if items > 0 => return Some(index + 1),
            b'\\' => {
                if !bytes.get(index + 1)?.is_ascii_punctuation() {
                    return None;
                }
                index += 2;
            }
            byte if byte.is_ascii_alphanumeric() || byte == b'_' => {
                let high = bytes.get(index + 2).copied();
                if bytes.get(index + 1) == Some(&b'-')
                    && high.is_some_and(|h| h.is_ascii_alphanumeric())
                {
                    if high? < byte {
                        return None;
                    }
                    index += 3;
                } else {
                    index += 1;
                }
            }
            _ => return None,
        }
        items += 1;
    }
}

/// Tells if a gorilla/mux template for a path has balanced braces, and each variable has a
/// name and a pattern that is a regular expression without a capture group. The library
/// rejects the other templates, and the route then never matches.
fn mux_template_valid(template: &str) -> bool {
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, byte) in template.bytes().enumerate() {
        match byte {
            b'{' => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
                if depth == 0 {
                    let inner = &template[start..index];
                    let valid = match inner.split_once(':') {
                        Some((name, pattern)) => {
                            !name.is_empty() && !pattern.is_empty() && common_regex(pattern)
                        }
                        None => !inner.is_empty(),
                    };
                    if !valid {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// The value of a Go string literal. An interpreted literal has escapes, and a raw literal has
/// none. A bad escape, or a value that is not UTF-8, gives none.
fn decode_go_string(literal: &str) -> Option<String> {
    if let Some(raw) = literal.strip_prefix('`') {
        return Some(raw.strip_suffix('`')?.replace('\r', ""));
    }
    let inner = literal.strip_prefix('"')?.strip_suffix('"')?;
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut index = 0;
    let digits = |from: usize, count: usize, radix: u32| -> Option<u32> {
        let text = inner.get(from..from + count)?;
        if text.len() != count || !text.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        u32::from_str_radix(text, radix).ok()
    };
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let escape = *bytes.get(index)?;
        index += 1;
        match escape {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(10),
            b'r' => out.push(13),
            b't' => out.push(9),
            b'v' => out.push(11),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'0'..=b'7' => {
                let value = digits(index - 1, 3, 8)?;
                index += 2;
                out.push(u8::try_from(value).ok()?);
            }
            b'x' => {
                out.push(u8::try_from(digits(index, 2, 16)?).ok()?);
                index += 2;
            }
            b'u' | b'U' => {
                let count = if escape == b'u' { 4 } else { 8 };
                let character = char::from_u32(digits(index, count, 16)?)?;
                index += count;
                let mut buffer = [0_u8; 4];
                out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

/// The directory of a file. The files of one directory are the files of one Go package.
pub(super) fn directory_of(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// The package name of a Go file, from its package clause.
fn package_name(root: Node<'_>, source: &[u8]) -> String {
    let mut cursor = root.walk();
    let name = root
        .named_children(&mut cursor)
        .find(|node| node.kind() == "package_clause")
        .and_then(|clause| clause.named_child(0))
        .map(|name| syntax::text(name, source).to_string());
    name.unwrap_or_default()
}

/// The top-level names of a Go file.
fn declared_names(root: Node<'_>, source: &[u8], names: &mut HashSet<String>) {
    let mut cursor = root.walk();
    for declaration in root.named_children(&mut cursor) {
        if declaration.kind() == "function_declaration" {
            if let Some(name) = declaration.child_by_field_name("name") {
                names.insert(syntax::text(name, source).to_string());
            }
            continue;
        }
        let mut pending = vec![declaration];
        while let Some(current) = pending.pop() {
            if matches!(
                current.kind(),
                "type_spec" | "type_alias" | "const_spec" | "var_spec"
            ) {
                let mut inner = current.walk();
                for name in current.children_by_field_name("name", &mut inner) {
                    names.insert(syntax::text(name, source).to_string());
                }
            }
            let mut inner = current.walk();
            pending.extend(current.named_children(&mut inner));
        }
    }
}

/// The top-level names of each Go package. A package is the files of one directory that
/// have the same package clause.
#[derive(Default)]
pub(super) struct PackageNames {
    by_package: HashMap<(String, String), Arc<HashSet<String>>>,
}

impl PackageNames {
    pub(super) fn build(files: &[FileSymbols], trees: &dyn TreeSource) -> Self {
        let mut sets: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for (position, file) in files.iter().enumerate() {
            if file.language != Language::Go {
                continue;
            }
            let Some(tree) = trees.tree(position) else {
                continue;
            };
            let root = tree.root_node();
            let key = (
                directory_of(&file.file).to_string(),
                package_name(root, &tree.source),
            );
            declared_names(root, &tree.source, sets.entry(key).or_default());
        }
        Self {
            by_package: sets
                .into_iter()
                .map(|(key, names)| (key, Arc::new(names)))
                .collect(),
        }
    }

    fn names(&self, key: &(String, String)) -> Arc<HashSet<String>> {
        self.by_package.get(key).cloned().unwrap_or_default()
    }
}

/// The functions and the methods of the Go files, by package directory and name. The
/// value has the position of the file and the position of the symbol.
#[derive(Default)]
pub(super) struct PackageCallables {
    by_name: HashMap<(String, String), Vec<(usize, usize)>>,
}

impl PackageCallables {
    pub(super) fn build(files: &[FileSymbols]) -> Self {
        let mut by_name: HashMap<(String, String), Vec<(usize, usize)>> = HashMap::new();
        for (file, symbols) in files.iter().enumerate() {
            if symbols.language != Language::Go {
                continue;
            }
            let directory = directory_of(&symbols.file);
            for (position, symbol) in symbols.symbols.iter().enumerate() {
                if symbol.kind == SymbolKind::Function {
                    by_name
                        .entry((directory.to_string(), symbol.name.clone()))
                        .or_default()
                        .push((file, position));
                }
            }
        }
        Self { by_name }
    }

    fn find(&self, directory: &str, name: &str) -> &[(usize, usize)] {
        self.by_name
            .get(&(directory.to_string(), name.to_string()))
            .map_or(&[], Vec::as_slice)
    }
}

impl<'a> Context<'a> {
    /// The function that a handler of a Go file names. A name with a qualifier names a
    /// function of the package of the import. A name without a qualifier names a function
    /// of the package of the file. A definition in the file wins. A package that the index
    /// does not resolve gives no function.
    pub(super) fn go_handler_symbol(&self, node: Node<'_>) -> Option<String> {
        self.go_handler_target(node)
            .map(|(symbol, _)| symbol.key.to_string())
    }

    /// The function and its file, as `go_handler_symbol` finds them. A name that a local
    /// binding gives, as a parameter or `Show := real`, has no function.
    pub(super) fn go_handler_target(
        &self,
        node: Node<'_>,
    ) -> Option<(&'a Symbol, &'a FileSymbols)> {
        let own = directory_of(&self.file.file);
        let (directory, name) = match node.kind() {
            "identifier" => {
                if self.is_go_local(node) {
                    return None;
                }
                (own, self.text(node))
            }
            "selector_expression" => {
                let operand = node.child_by_field_name("operand")?;
                let name = self.text(node.child_by_field_name("field")?);
                if operand.kind() == "identifier" && self.is_go_local(operand) {
                    return None;
                }
                let import = (operand.kind() == "identifier")
                    .then(|| {
                        self.file.imports.iter().find(|binding| {
                            binding.kind == ImportKind::Module
                                && binding.local_name.as_deref() == Some(self.text(operand))
                        })
                    })
                    .flatten();
                // Another qualifier is a variable or a field. The index has no symbol for it.
                let binding = import?;
                let file = match &binding.target_file {
                    ImportTarget::Resolved(file) => file.as_str(),
                    ImportTarget::Package(files) => files.first()?.as_str(),
                    _ => return None,
                };
                (directory_of(file), name)
            }
            _ => return None,
        };
        if directory == own {
            // A name without a qualifier, and `package.Name`, never name a method.
            let functions: Vec<usize> = self
                .callables_here()
                .get(name)
                .into_iter()
                .flatten()
                .copied()
                .filter(|position| self.file.symbols[*position].kind == SymbolKind::Function)
                .collect();
            if let [position] = functions.as_slice() {
                return Some((&self.file.symbols[*position], self.file));
            }
        }
        self.package_lookup(
            self.all_parsed
                .shared()
                .go_callables
                .get_or_init(|| PackageCallables::build(self.all)),
            directory,
            name,
        )
    }

    fn package_lookup(
        &self,
        callables: &PackageCallables,
        directory: &str,
        name: &str,
    ) -> Option<(&'a Symbol, &'a FileSymbols)> {
        match callables.find(directory, name) {
            [(file, position)] => {
                let file = &self.all[*file];
                Some((&file.symbols[*position], file))
            }
            _ => None,
        }
    }

    /// The value of a Go string literal, as the language reads it. An escape that the
    /// language does not allow, or a value that is not UTF-8, gives none.
    pub(super) fn go_string(&self, node: Node<'_>) -> Option<String> {
        if !self.is_string(node) {
            return None;
        }
        decode_go_string(self.text(node))
    }

    /// The names of the variables that hold a `net/http` multiplexer. A name is in the set
    /// when the file has one binding of the name, and the binding is a call of
    /// `http.NewServeMux()`. The walk builds the set one time for the file.
    pub(super) fn is_go_servemux(&self, name: &str) -> bool {
        self.go_servemuxes
            .get_or_init(|| {
                let http = net_http_names(self.file);
                let is_call = |value: Node<'_>| {
                    value.kind() == "call_expression"
                        && Self::arguments_of(value).is_empty()
                        && value
                            .child_by_field_name("function")
                            .filter(|function| function.kind() == "selector_expression")
                            .is_some_and(|function| {
                                let operand = function.child_by_field_name("operand");
                                let field = function.child_by_field_name("field");
                                operand.is_some_and(|operand| {
                                    http.iter().any(|name| name == self.text(operand))
                                        && !self.is_go_shadowed(operand)
                                }) && field.is_some_and(|field| self.text(field) == "NewServeMux")
                            })
                };
                let mut counts: HashMap<String, (usize, bool)> = HashMap::new();
                let mut note = |binder: Node<'_>, makes_one: bool| {
                    if binder.kind() == "identifier" {
                        let entry = counts.entry(self.text(binder).to_string()).or_default();
                        entry.0 += 1;
                        entry.1 |= makes_one;
                    }
                };
                let mut pending = vec![self.parsed.root_node()];
                while let Some(node) = pending.pop() {
                    let mut cursor = node.walk();
                    match node.kind() {
                        "short_var_declaration" | "assignment_statement" => {
                            let left = node.child_by_field_name("left");
                            let right = node.child_by_field_name("right");
                            let names: Vec<Node<'_>> = left
                                .map(|list| list.named_children(&mut cursor).collect())
                                .unwrap_or_default();
                            let mut inner = node.walk();
                            let values: Vec<Node<'_>> = right
                                .map(|list| list.named_children(&mut inner).collect())
                                .unwrap_or_default();
                            let define = node.kind() == "short_var_declaration";
                            for (position, binder) in names.iter().enumerate() {
                                let makes_one = define
                                    && names.len() == values.len()
                                    && is_call(values[position]);
                                note(*binder, makes_one);
                            }
                        }
                        "var_spec" => {
                            let names: Vec<Node<'_>> =
                                node.children_by_field_name("name", &mut cursor).collect();
                            let mut inner = node.walk();
                            let values: Vec<Node<'_>> = node
                                .child_by_field_name("value")
                                .map(|list| list.named_children(&mut inner).collect())
                                .unwrap_or_default();
                            for (position, binder) in names.iter().enumerate() {
                                let makes_one =
                                    names.len() == values.len() && is_call(values[position]);
                                note(*binder, makes_one);
                            }
                        }
                        "parameter_declaration" | "variadic_parameter_declaration" => {
                            for binder in node.children_by_field_name("name", &mut cursor) {
                                note(binder, false);
                            }
                        }
                        "range_clause" => {
                            if let Some(left) = node.child_by_field_name("left") {
                                for binder in left.named_children(&mut cursor) {
                                    note(binder, false);
                                }
                            }
                        }
                        "type_switch_statement" => {
                            if let Some(alias) = node.child_by_field_name("alias") {
                                for binder in alias.named_children(&mut cursor) {
                                    note(binder, false);
                                }
                            }
                        }
                        _ => {}
                    }
                    let mut cursor = node.walk();
                    pending.extend(node.named_children(&mut cursor));
                }
                counts
                    .into_iter()
                    .filter(|(_, (count, makes_one))| *count == 1 && *makes_one)
                    .map(|(name, _)| name)
                    .collect()
            })
            .contains(name)
    }

    /// Tells if a name at this node has a binding that is in scope at the node. A binding is
    /// a parameter, a short variable declaration, a variable, a range variable, or a type
    /// switch variable. The scope of a parameter is the function. The scope of another
    /// binding starts after its declaration, and ends with its block, its statement, or its
    /// case clause.
    pub(super) fn is_go_local(&self, node: Node<'_>) -> bool {
        let locals = self.go_locals.get_or_init(|| {
            let mut sites: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
            let mut pending = vec![(self.parsed.root_node(), None::<usize>)];
            while let Some((node, function_end)) = pending.pop() {
                let function_end = match node.kind() {
                    "function_declaration" | "method_declaration" | "func_literal" => {
                        Some(node.end_byte())
                    }
                    _ => function_end,
                };
                // (binder, first byte of the scope, end of the scope)
                let mut binders: Vec<(Node<'_>, usize, usize)> = Vec::new();
                let mut cursor = node.walk();
                if let Some(end) = function_end {
                    match node.kind() {
                        "short_var_declaration" => {
                            if let Some(left) = node.child_by_field_name("left") {
                                let scope = Self::scope_end(node).unwrap_or(end);
                                binders.extend(
                                    left.named_children(&mut cursor)
                                        .map(|binder| (binder, node.end_byte(), scope)),
                                );
                            }
                        }
                        "range_clause" => {
                            if let Some(left) = node.child_by_field_name("left") {
                                let scope = Self::scope_end(node).unwrap_or(end);
                                binders.extend(
                                    left.named_children(&mut cursor)
                                        .map(|binder| (binder, node.end_byte(), scope)),
                                );
                            }
                        }
                        "parameter_declaration" | "variadic_parameter_declaration" => {
                            binders.extend(
                                node.children_by_field_name("name", &mut cursor)
                                    .map(|binder| (binder, binder.start_byte(), end)),
                            );
                        }
                        "var_spec" => {
                            let scope = Self::scope_end(node).unwrap_or(end);
                            binders.extend(
                                node.children_by_field_name("name", &mut cursor)
                                    .map(|binder| (binder, node.end_byte(), scope)),
                            );
                        }
                        "type_switch_statement" => {
                            if let Some(alias) = node.child_by_field_name("alias") {
                                binders.extend(
                                    alias
                                        .named_children(&mut cursor)
                                        .map(|binder| (binder, alias.end_byte(), node.end_byte())),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                for (binder, from, to) in binders {
                    if binder.kind() == "identifier" {
                        sites
                            .entry(self.text(binder).to_string())
                            .or_default()
                            .push((from, to));
                    }
                }
                let mut cursor = node.walk();
                pending.extend(
                    node.named_children(&mut cursor)
                        .map(|child| (child, function_end)),
                );
            }
            sites
                .into_iter()
                .map(|(name, mut list)| {
                    list.sort_unstable();
                    let starts = list.iter().map(|(start, _)| *start).collect();
                    let mut highest = 0;
                    let ends = list
                        .iter()
                        .map(|(_, end)| {
                            highest = highest.max(*end);
                            highest
                        })
                        .collect();
                    (name, (starts, ends))
                })
                .collect()
        });
        let Some((starts, ends)) = locals.get(self.text(node)) else {
            return false;
        };
        let before = starts.partition_point(|start| *start <= node.start_byte());
        before > 0 && ends[before - 1] > node.start_byte()
    }

    /// Tells if a local binding or a declaration of the package has the name of this node. The
    /// package name of an import is then not the name of the package at the node.
    pub(super) fn is_go_shadowed(&self, node: Node<'_>) -> bool {
        if self.is_go_local(node) {
            return true;
        }
        self.go_declared
            .get_or_init(|| {
                let key = (
                    directory_of(&self.file.file).to_string(),
                    package_name(self.parsed.root_node(), self.source),
                );
                self.all_parsed
                    .shared()
                    .go_packages
                    .get_or_init(|| PackageNames::build(self.all, self.all_parsed))
                    .names(&key)
            })
            .contains(self.text(node))
    }

    /// The end of the scope of a declaration: its block, its `if`, `for`, or `switch`
    /// statement, or its case clause.
    fn scope_end(node: Node<'_>) -> Option<usize> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if matches!(
                parent.kind(),
                "block"
                    | "if_statement"
                    | "for_statement"
                    | "expression_switch_statement"
                    | "type_switch_statement"
                    | "select_statement"
                    | "expression_case"
                    | "default_case"
                    | "type_case"
                    | "communication_case"
            ) {
                return Some(parent.end_byte());
            }
            current = parent.parent();
        }
        None
    }
}

/// The methods of a router that register a route, make a group, or serve. A method that adds
/// middleware, as `Use` and `Pre`, is not in the list.
const ROUTER_METHODS: [&str; 52] = [
    "Get",
    "Post",
    "Put",
    "Patch",
    "Delete",
    "Head",
    "Options",
    "Connect",
    "Trace",
    "Query",
    "Method",
    "MethodFunc",
    "Handle",
    "HandleFunc",
    "Route",
    "Group",
    "Mount",
    "With",
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH",
    "HEAD",
    "OPTIONS",
    "CONNECT",
    "TRACE",
    "Any",
    "Match",
    "Add",
    "PathPrefix",
    "Path",
    "Methods",
    "Name",
    "Subrouter",
    "NewRoute",
    "Handler",
    "HandlerFunc",
    "Run",
    "RunTLS",
    "Start",
    "StartTLS",
    "ServeHTTP",
    "Routes",
    "Walk",
    "Static",
    "StaticFile",
    "StaticFS",
    "StaticFileFS",
    "Host",
    "Headers",
    "Queries",
];

/// The factories of the repository with a call that attaches the result, or that the
/// walk cannot classify. A factory is a function or a method that returns a router. A call
/// serves the result as the root, or it attaches it to a parent. One pass over the files
/// that call a factory gives the facts for the whole analysis.
#[derive(Default)]
pub(super) struct CallSites {
    /// The keys of the symbols of the factories.
    blocked: HashSet<String>,
    /// The keys of the functions that a reference names anywhere in the repository.
    referenced: HashSet<String>,
}

impl CallSites {
    pub(super) fn is_blocked(&self, key: &str) -> bool {
        self.blocked.contains(key)
    }

    pub(super) fn is_referenced(&self, key: &str) -> bool {
        self.referenced.contains(key)
    }

    /// The facts for all Go files. The walk of each file that imports a framework finds the
    /// factories. Then one pass over the references and one pass over the syntax of each file
    /// classify each use of a factory.
    pub(super) fn build(files: &[FileSymbols], trees: &dyn TreeSource) -> Self {
        let referenced: HashSet<String> = files
            .iter()
            .filter(|file| file.language == Language::Go)
            .flat_map(|file| &file.references)
            .flat_map(|reference| reference.resolution.candidates.iter())
            .map(|key| key.to_string())
            .collect();
        let mut factories: HashSet<String> = HashSet::new();
        let mut wrappers: HashSet<String> = HashSet::new();
        let mut raw_wrapper_types: Vec<(String, String)> = Vec::new();
        // Each type has its package, the directory, and its name. The count is the number of
        // declarations of the key. The alias has the name of its base type, and tells if
        // the base type cannot be read as a name of the same package.
        let mut types: TypeTable = HashMap::new();
        for (position, file) in files.iter().enumerate() {
            if file.language != Language::Go {
                continue;
            }
            let Some(tree) = trees.tree(position) else {
                continue;
            };
            let context = Context::new(position, &tree, file, files, trees, &[]);
            let directory = directory_of(&file.file);
            for (name, alias) in collect_aliases(tree.root_node(), &tree.source) {
                let entry = types
                    .entry((directory.to_string(), name))
                    .or_insert((0, None));
                entry.0 += 1;
                entry.1 = alias;
            }
            for (key, wrapper) in factory_keys(&context, tree.root_node()) {
                if let Some(kind) = wrapper {
                    wrappers.insert(key.clone());
                    raw_wrapper_types.push((key.clone(), kind));
                }
                factories.insert(key);
            }
        }
        let mut blocked = HashSet::new();
        let mut memo: HashMap<(String, String), Option<String>> = HashMap::new();
        let mut wrapper_types: Vec<(String, String, String)> = Vec::new();
        for (key, kind) in raw_wrapper_types {
            let directory = directory_of(key.split('#').next().unwrap_or("")).to_string();
            let bare = kind.trim_start_matches(['*', '&']);
            let bare = bare.split('[').next().unwrap_or(bare);
            // A type of another package is not read, so a value of it gives no entry.
            let resolved = if bare.contains('.') {
                None
            } else {
                resolve_alias(&types, &mut memo, &directory, bare)
            };
            match resolved {
                Some(base) => wrapper_types.push((key, directory, base)),
                None => {
                    blocked.insert(key);
                }
            }
        }
        if factories.is_empty() {
            return Self {
                blocked,
                referenced,
            };
        }
        // A wrapper type can define a method that hides a method of the router, or that
        // serves the requests in another way. Then its values are not routers.
        let mut hiding: HashSet<(String, String)> = HashSet::new();
        for file in files.iter().filter(|file| file.language == Language::Go) {
            let directory = directory_of(&file.file);
            for symbol in file.symbols.iter().filter(|symbol| {
                symbol.is_callable()
                    && (ROUTER_METHODS.contains(&symbol.name.as_str())
                        || matches!(symbol.name.as_str(), "Use" | "Pre" | "ServeHTTP"))
            }) {
                let Some(kind) = symbol
                    .receiver_type
                    .as_deref()
                    .or(symbol.container.as_deref())
                else {
                    continue;
                };
                if let Some(base) = resolve_alias(&types, &mut memo, directory, type_name(kind)) {
                    hiding.insert((directory.to_string(), base));
                }
            }
        }
        for (key, directory, name) in &wrapper_types {
            if hiding.contains(&(directory.clone(), name.clone())) {
                blocked.insert(key.clone());
            }
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
        let mut blocked_names: HashSet<&str> = HashSet::new();
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
            blocked_names.extend(Self::mentions(tree.root_node(), &tree.source, &by_name));
            if sites.is_empty() {
                continue;
            }
            let context = Context::new(position, &tree, file, files, trees, &[]);
            let own_index = FileUses::new(tree.root_node(), &tree.source, file, &context, false);
            let mut other_index: Option<FileUses<'_, '_>> = None;
            for (reference, keys) in sites {
                let strict = keys.iter().any(|key| wrappers.contains(*key));
                // A registration in the file of the factory has an order that the walk reads.
                let same_file = keys
                    .iter()
                    .all(|key| key.split('#').next() == Some(file.file.as_str()));
                let index = if same_file {
                    &own_index
                } else {
                    &*other_index.get_or_insert_with(|| {
                        FileUses::new(tree.root_node(), &tree.source, file, &context, true)
                    })
                };
                if index.is_blocked(
                    reference.name_range.start_byte,
                    reference.name_range.end_byte,
                    strict,
                ) {
                    blocked.extend(keys.into_iter().map(str::to_string));
                }
            }
        }
        for name in blocked_names {
            if let Some(keys) = by_name.get(name) {
                blocked.extend(keys.iter().map(|key| key.to_string()));
            }
        }
        Self {
            blocked,
            referenced,
        }
    }

    /// The keys of the factories that a name in the file mentions without a call. A function
    /// value, a method value, and an argument that is a function are such mentions. A
    /// mention can name another function of the same name, and the result stays safe.
    fn mentions<'m>(
        root: Node<'_>,
        source: &[u8],
        by_name: &HashMap<&'m str, Vec<&'m str>>,
    ) -> HashSet<&'m str> {
        let mut found = HashSet::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if matches!(node.kind(), "identifier" | "field_identifier") {
                let name = std::str::from_utf8(&source[node.byte_range()]).unwrap_or("");
                if let Some((known, _)) = by_name.get_key_value(name) {
                    if !Self::is_declaration_or_callee(node) {
                        found.insert(*known);
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
struct FileUses<'t, 'c> {
    root: Node<'t>,
    source: &'t [u8],
    /// The context of the file. It tells if a local name hides a package name.
    context: &'c Context<'t>,
    http: Vec<String>,
    /// The start of each use of a name that is not a use as a router, by name, in order.
    bad: HashMap<String, Vec<usize>>,
    /// The same, when any method of the router counts as a bad use. A wrapper type can
    /// shadow a method of the router.
    bad_strict: HashMap<String, Vec<usize>>,
    /// A call that registers a route on the result is a use that the walk cannot order. A
    /// file that does not declare the factory uses this mode.
    registrations_block: bool,
}

impl<'t, 'c> FileUses<'t, 'c> {
    fn new(
        root: Node<'t>,
        source: &'t [u8],
        file: &FileSymbols,
        context: &'c Context<'t>,
        registrations_block: bool,
    ) -> Self {
        let mut uses = Self {
            registrations_block,
            root,
            source,
            context,
            http: package_names(file, |module| module == "net/http", "http"),
            bad: HashMap::new(),
            bad_strict: HashMap::new(),
        };
        // The walk keeps the ancestors of each node. `Node::parent` searches down from the root,
        // so a call for each name costs time in proportion to the depth.
        let mut ancestors: Vec<Node<'t>> = Vec::new();
        let mut pending = vec![(root, 0)];
        while let Some((node, depth)) = pending.pop() {
            ancestors.truncate(depth);
            if node.kind() == "identifier" && uses.is_bad_use(node, &ancestors, true) {
                let name = uses.text(node).to_string();
                if uses.is_bad_use(node, &ancestors, false) {
                    uses.bad
                        .entry(name.clone())
                        .or_default()
                        .push(node.start_byte());
                }
                uses.bad_strict
                    .entry(name)
                    .or_default()
                    .push(node.start_byte());
            }
            let mut cursor = node.walk();
            let children: Vec<Node<'t>> = node.named_children(&mut cursor).collect();
            pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
            ancestors.push(node);
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
            && !self.context.is_go_shadowed(operand)
    }

    /// Tells if a selector is a call of a method that registers a route, or that serves the
    /// router, on the node. A call that adds middleware changes the guards of the routes.
    fn is_router_call(
        &self,
        selector: Node<'t>,
        operand: Node<'t>,
        call: Option<Node<'t>>,
    ) -> bool {
        const SERVING: [&str; 7] = [
            "Run",
            "RunTLS",
            "Start",
            "StartTLS",
            "ServeHTTP",
            "Routes",
            "Walk",
        ];
        selector.child_by_field_name("operand") == Some(operand)
            && call.is_some_and(|call| {
                call.kind() == "call_expression"
                    && call.child_by_field_name("function") == Some(selector)
            })
            && selector.child_by_field_name("field").is_some_and(|field| {
                let name = self.text(field);
                SERVING.contains(&name)
                    || (!self.registrations_block && ROUTER_METHODS.contains(&name))
            })
    }

    /// Tells if a literal is an `http.Server`, which serves its handler as the root.
    fn is_server_literal(&self, literal: Node<'t>) -> bool {
        let Some(kind) = literal
            .child_by_field_name("type")
            .filter(|kind| kind.kind() == "qualified_type")
        else {
            return false;
        };
        let (Some(package), Some(name)) = (
            kind.child_by_field_name("package"),
            kind.child_by_field_name("name"),
        ) else {
            return false;
        };
        self.text(name) == "Server"
            && self.http.iter().any(|known| known == self.text(package))
            && !self.context.is_go_shadowed(package)
    }

    /// Tells if a use of a name is not a use as a router that the walk understands. A router
    /// that a variable holds is only a receiver of a call, or the root handler of a server.
    /// `ancestors` holds the ancestors of the node, with its parent last.
    fn is_bad_use(&self, node: Node<'t>, ancestors: &[Node<'t>], strict: bool) -> bool {
        let mut child = node;
        let mut up = ancestors.iter().rev().copied();
        let mut parent = up.next();
        while let Some(found) = parent.filter(|found| found.kind() == "parenthesized_expression") {
            child = found;
            parent = up.next();
        }
        let Some(parent) = parent else {
            return false;
        };
        let grand = up.next();
        match parent.kind() {
            "selector_expression" => strict || !self.is_router_call(parent, child, grand),
            "argument_list" => !self.is_sink(grand),
            "var_spec" => parent.child_by_field_name("value") == Some(child),
            "expression_list" => {
                let Some(grand) = grand else {
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
                let literal = std::iter::once(grand)
                    .flatten()
                    .chain(up)
                    .find(|found| found.kind() == "composite_literal");
                !literal.is_some_and(|literal| self.is_server_literal(literal))
            }
            "block" | "statement_list" | "expression_statement" | "source_file" => false,
            _ => true,
        }
    }

    /// Tells if the call site that has the name at these bytes attaches the result, or has
    /// a use that the walk cannot classify.
    fn is_blocked(&self, start: usize, end: usize, strict: bool) -> bool {
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
            "selector_expression" => strict || !self.is_router_call(parent, value, parent.parent()),
            "argument_list" => !self.is_sink(parent.parent()),
            "literal_element" | "keyed_element" => {
                let mut up = parent.parent();
                while let Some(found) = up.filter(|found| found.kind() != "composite_literal") {
                    up = found.parent();
                }
                !up.is_some_and(|literal| self.is_server_literal(literal))
            }
            "expression_list" => self.is_blocked_assignment(value, parent, strict),
            "var_spec" => true,
            _ => true,
        }
    }

    /// A call in a list of values: a local variable that only receives calls, or the blank name.
    fn is_blocked_assignment(&self, value: Node<'t>, list: Node<'t>, strict: bool) -> bool {
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
        let bad = if strict { &self.bad_strict } else { &self.bad };
        bad.get(name).is_some_and(|starts| {
            let first = starts.partition_point(|start| *start <= from);
            starts.get(first).is_some_and(|start| *start < to)
        })
    }
}

/// The keys of the functions and the methods of a file that return a router that they made.
/// The walk does not read the calls of the repository for this result.
pub(super) fn factory_keys(context: &Context<'_>, root: Node<'_>) -> Vec<(String, Option<String>)> {
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
            routed_owners: HashSet::new(),
            routed_factories: HashSet::new(),
            fresh_calls: HashMap::new(),
            registered_fresh: HashSet::new(),
            returned_of: HashMap::new(),
            components: HashMap::new(),
            attached: HashSet::new(),
            owners: 0,
            frames: Vec::new(),
            mounts: HashSet::new(),
            mounted: HashSet::new(),
            tree: Vec::new(),
            made_cache: HashMap::new(),
            prefix_next: false,
            omit_next: false,
            mount_next: false,
            eval_depth: 0,
            eval_memo: HashMap::new(),
            scanned: HashSet::new(),
            dry,
            function_key: None,
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
    /// The owners of the routers that have a record.
    routed_owners: HashSet<usize>,
    /// The factories that register a route on a router that they make.
    routed_factories: HashSet<String>,
    /// The number of calls of each factory that the walk read, and the factories that got a
    /// registration on the result of a call.
    fresh_calls: HashMap<String, usize>,
    registered_fresh: HashSet<String>,
    /// The router that a factory with a summary returns.
    returned_of: HashMap<String, usize>,
    /// The routers of the returned router of each factory, and the routers that join to it.
    components: HashMap<String, Vec<usize>>,
    /// The factories whose result a call can attach to a parent. Their own routes have
    /// paths that depend on the call.
    attached: HashSet<String>,
    owners: usize,
    frames: Vec<Frame>,
    /// The mounts that the walk made: the child, the parent, and the prefix.
    mounts: HashSet<(usize, usize, String)>,
    /// The routers that a mount attached. Each router is in a tree of the forest.
    mounted: HashSet<usize>,
    tree: Vec<usize>,
    /// Whether the router and each router above it came from the function that made it.
    made_cache: HashMap<usize, bool>,
    prefix_next: bool,
    omit_next: bool,
    mount_next: bool,
    eval_depth: usize,
    /// The router that each call gave, by the id of the node.
    eval_memo: HashMap<usize, Option<usize>>,
    /// The calls of the chains that the walk read from their last call.
    scanned: HashSet<usize>,
    /// The walk only finds the factories. It does not read the calls of the repository.
    dry: bool,
    /// The key of the function that the walk reads.
    function_key: Option<String>,
    factory_keys: Vec<(String, Option<String>)>,
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

    /// Reads a node and its descendants in source order. The walk keeps its own stack, so a
    /// deep expression or a long chain does not overflow the stack of the thread.
    fn visit(&mut self, root: Node<'t>) {
        let mut stack = vec![Step::Read(root)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Read(node) => self.visit_one(node, &mut stack),
                Step::LeaveBlock => {
                    self.scopes.pop();
                }
            }
        }
    }

    fn push_children(node: Node<'t>, skipped: Option<Node<'t>>, stack: &mut Vec<Step<'t>>) {
        let mut cursor = node.walk();
        let children: Vec<Node<'t>> = node.named_children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            if Some(child) != skipped {
                stack.push(Step::Read(child));
            }
        }
    }

    fn visit_one(&mut self, node: Node<'t>, stack: &mut Vec<Step<'t>>) {
        match node.kind() {
            "function_declaration" | "method_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .filter(|_| node.kind() == "function_declaration")
                    .map(|name| self.text(name));
                self.walk_declared(node, name);
                return;
            }
            // A branch with a condition that is the word `false`, or the other branch of the
            // word `true`, never runs.
            "if_statement" => {
                let value = node
                    .child_by_field_name("condition")
                    .and_then(|condition| self.constant_condition(condition));
                let dead = match value {
                    Some(true) => node.child_by_field_name("alternative"),
                    Some(false) => node.child_by_field_name("consequence"),
                    None => None,
                };
                Self::push_children(node, dead, stack);
                return;
            }
            "for_statement" => {
                let body = node.child_by_field_name("body");
                let dead = node
                    .named_child(0)
                    .is_some_and(|first| self.constant_condition(first) == Some(false));
                Self::push_children(node, body.filter(|_| dead), stack);
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
                stack.push(Step::LeaveBlock);
                Self::push_children(node, None, stack);
                return;
            }
            "short_var_declaration" | "assignment_statement" => self.assignment(node),
            "var_spec" => self.var_spec(node),
            "return_statement" => self.return_statement(node),
            "composite_literal" => self.literal_escapes(node),
            "call_expression" if !self.consumed.contains(&node.id()) => self.call(node),
            _ => {}
        }
        Self::push_children(node, None, stack);
    }

    /// The value of a condition that is the word `true` or the word `false`.
    fn constant_condition(&self, node: Node<'t>) -> Option<bool> {
        if !matches!(node.kind(), "identifier" | "true" | "false")
            || self.context.is_go_shadowed(node)
        {
            return None;
        }
        match self.text(node) {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
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
        let saved_key = std::mem::replace(
            &mut self.function_key,
            node.child_by_field_name("name")
                .and_then(|name| self.context.symbol_at(name)),
        );
        // The scope of the package stays in place, and the scopes of the caller wait aside.
        let saved = self.scopes.split_off(1);
        self.scopes.push(HashMap::new());
        self.owners += 1;
        let owner = self.owners;
        self.frames.push(Frame {
            function: true,
            returned: Vec::new(),
            returns: 0,
            router_returns: 0,
            results: Vec::new(),
            wrapped: false,
            wrapper_type: None,
            owner,
            pure: true,
            created: Vec::new(),
        });
        self.bind_parameters(node.child_by_field_name("parameters"), None);
        self.bind_results(node.child_by_field_name("result"));
        if let Some(body) = node.child_by_field_name("body") {
            self.visit(body);
        }
        self.function_key = saved_key;
        let frame = self.frames.pop();
        if let Some(name) = declared {
            self.in_progress.remove(name);
        }
        self.scopes.truncate(1);
        self.scopes.extend(saved);
        let Some(frame) = frame else {
            return;
        };
        let key = node
            .child_by_field_name("name")
            .and_then(|name| self.context.symbol_at(name));
        let mut returned = frame.returned.clone();
        returned.sort_unstable();
        returned.dedup();
        let only_router = match returned.as_slice() {
            [router] if frame.returns == frame.router_returns => Some(*router),
            _ => None,
        };
        let summary = only_router
            .filter(|_| {
                frame.pure
                    && !frame.wrapped
                    && declared.is_some_and(|name| !self.cyclic.contains(name))
            })
            .and_then(|router| self.summarize(router, owner));
        if let (Some(name), Some(router), true) = (declared, only_router, summary.is_some()) {
            self.returned_of.insert(name.to_string(), router);
        }
        let mut made: Vec<usize> = Vec::new();
        for router in returned {
            if self.made_by(router, owner) {
                made.push(router);
            }
        }
        let comp = (!made.is_empty()).then(|| {
            let mut all = self.component(&made, &frame.created);
            all.sort_unstable();
            all
        });
        let id = declared.map(str::to_string).or_else(|| key.clone());
        if let (Some(id), Some(comp)) = (id, comp) {
            let needs_a_summary = declared.is_some()
                && !frame.wrapped
                && summary
                    .as_ref()
                    .is_none_or(|summary| !summary.prefixes.is_empty());
            if let Some(key) = &key {
                self.factory_keys
                    .push((key.clone(), frame.wrapper_type.clone()));
            }
            let attached_in_the_repository = !self.dry
                && key
                    .as_deref()
                    .is_some_and(|key| self.context.go_factory_blocked(key));
            if needs_a_summary || attached_in_the_repository {
                self.attached.insert(id.clone());
            }
            if self.routed_owners.contains(&owner) {
                self.routed_factories.insert(id.clone());
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
        self.note_registration(router);
        let router = &self.routers[router];
        if router.external || router.owner != self.current_owner() {
            self.mark_impure();
        }
    }

    /// A registration on the router that a call of a factory gave, or on a group of it, can
    /// replace a route of the factory or be replaced by it. Then the routes of both give no
    /// entry.
    fn note_registration(&mut self, router: usize) {
        let root = self.find(router);
        let fresh = [root, router].into_iter().find(|id| {
            self.fresh_of
                .get(id)
                .is_some_and(|name| self.routed_factories.contains(name))
        });
        if let Some(fresh) = fresh {
            self.registered_fresh.insert(self.fresh_of[&fresh].clone());
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
        ) && self.is_net_http(operand)
    }

    /// Tells if the node names the package `net/http`, and no local name hides it.
    fn is_net_http(&self, package: Node<'t>) -> bool {
        package_names(self.context.file, |module| module == "net/http", "http")
            .iter()
            .any(|name| name == self.text(package))
            && !self.context.is_go_shadowed(package)
    }

    /// The values of a literal escape, unless the literal is an `http.Server`. A server
    /// serves its handler as the root.
    fn literal_escapes(&mut self, literal: Node<'t>) {
        let is_server = literal
            .child_by_field_name("type")
            .filter(|kind| kind.kind() == "qualified_type")
            .and_then(|kind| {
                Some((
                    kind.child_by_field_name("package")?,
                    kind.child_by_field_name("name")?,
                ))
            })
            .is_some_and(|(package, name)| {
                self.text(name) == "Server" && self.is_net_http(package)
            });
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
    fn component(&self, returned: &[usize], created: &[usize]) -> Vec<usize> {
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
        let mut seen: HashSet<usize> = returned.iter().copied().collect();
        let mut pending: Vec<usize> = returned.to_vec();
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
    fn made_by(&mut self, router: usize, owner: usize) -> bool {
        self.routers[router].owner == owner && self.self_made(router)
    }

    /// Tells if the router is not from a parameter, and each router above it has the owner of
    /// the router. The walk goes up one time for each router, and keeps the result.
    fn self_made(&mut self, router: usize) -> bool {
        let mut path = Vec::new();
        let mut current = router;
        let result = loop {
            if let Some(known) = self.made_cache.get(&current) {
                break *known;
            }
            let node = &self.routers[current];
            if node.external {
                break false;
            }
            match node.links.as_slice() {
                [] => break true,
                [link] if self.routers[link.parent].owner == node.owner => {
                    path.push(current);
                    current = link.parent;
                }
                _ => break false,
            }
        };
        self.made_cache.insert(current, result);
        for node in path {
            self.made_cache.insert(node, result);
        }
        result
    }

    fn summarize(&mut self, router: usize, owner: usize) -> Option<Summary> {
        if !self.made_by(router, owner) {
            return None;
        }
        let place = self.place_now(router, None)?;
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
        *self.fresh_calls.entry(name.to_string()).or_default() += 1;
        Some(id)
    }

    fn closure(&mut self, literal: Node<'t>, bound: Option<usize>) {
        self.scopes.push(HashMap::new());
        let owner = self.current_owner();
        self.frames.push(Frame {
            function: false,
            returned: Vec::new(),
            returns: 0,
            router_returns: 0,
            results: Vec::new(),
            wrapped: false,
            wrapper_type: None,
            owner,
            pure: true,
            created: Vec::new(),
        });
        let before = self.routers.len();
        self.bind_parameters(literal.child_by_field_name("parameters"), bound);
        if bound.is_none() {
            for router in before..self.routers.len() {
                self.routers[router].partial = true;
            }
        }
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
        let package = kind.child_by_field_name("package")?;
        let name = self.text(kind.child_by_field_name("name")?);
        let framework = self.framework_of(package)?;
        if !framework.router_types().contains(&name) {
            return None;
        }
        let router = self.make_root(framework);
        self.routers[router].external = true;
        // A caller can pass its router, and then the function registers on it.
        let called = !self.dry
            && self
                .function_key
                .as_deref()
                .is_some_and(|key| self.context.go_function_referenced(key));
        if !framework.is_root_type(name) || called {
            self.routers[router].partial = true;
            self.routers[router].mountable = false;
        }
        Some(router)
    }

    /// The framework that the node names as a package, when no local name hides it.
    fn framework_of(&self, package: Node<'t>) -> Option<Framework> {
        let package_text = self.text(package);
        if self.lookup(package_text).is_some() || self.context.is_go_shadowed(package) {
            return None;
        }
        self.packages
            .iter()
            .find(|(name, _)| name == package_text)
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
            inline: false,
            owner: self.current_owner(),
            external: false,
            base: Vec::new(),
        });
        let id = self.routers.len() - 1;
        self.tree.push(id);
        if let Some(frame) = self.frames.iter_mut().rev().find(|frame| frame.function) {
            frame.created.push(id);
        }
        id
    }

    fn make_child(&mut self, parent: usize, prefix: String, guards: Vec<String>) -> usize {
        self.note_registration(parent);
        let framework = self.routers[parent].framework;
        let link = Link {
            parent,
            parent_len: self.routers[parent].guards.len(),
            prefix,
            at: self.records.len(),
        };
        self.routers.push(Router {
            framework,
            links: vec![link],
            guards,
            pre: Vec::new(),
            retroactive: matches!(framework, Framework::Chi | Framework::Mux),
            partial: self.routers[parent].partial,
            mountable: false,
            inline: false,
            owner: self.current_owner(),
            external: false,
            base: Vec::new(),
        });
        let id = self.routers.len() - 1;
        self.tree.push(id);
        self.join(parent, id);
        if let Some(frame) = self.frames.iter_mut().rev().find(|frame| frame.function) {
            frame.created.push(id);
        }
        id
    }

    /// A sub-router of gorilla/mux with a `NotFoundHandler` or a `MethodNotAllowedHandler` takes
    /// each request that reaches it. Then it is an unread registration of the router.
    fn note_handler_assignment(&mut self, left: Node<'t>) {
        for target in Self::named_children(left) {
            if target.kind() != "selector_expression" {
                continue;
            }
            let (Some(field), Some(operand)) = (
                target.child_by_field_name("field"),
                target.child_by_field_name("operand"),
            ) else {
                continue;
            };
            if !matches!(
                self.text(field),
                "NotFoundHandler" | "MethodNotAllowedHandler"
            ) {
                continue;
            }
            if let Some(router) = self.eval(operand) {
                if self.routers[router].framework == Framework::Mux {
                    self.push_opaque(router, Span::of(&target));
                }
            }
        }
    }

    fn assignment(&mut self, node: Node<'t>) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        let define = node.kind() == "short_var_declaration";
        self.note_handler_assignment(left);
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
        let values = node
            .named_child(0)
            .map(Self::named_children)
            .unwrap_or_default();
        let mut routers = Vec::new();
        if values.is_empty() {
            let results = self
                .frames
                .last()
                .map(|frame| frame.results.clone())
                .unwrap_or_default();
            for name in results {
                if let Some(Some(router)) = self.lookup(&name) {
                    routers.push(router);
                }
            }
        }
        let mut wrapper_type: Option<String> = None;
        for value in values {
            self.escape(value);
            match self.eval(value) {
                Some(router) => routers.push(router),
                None => {
                    if let Some((router, kind)) = self.wrapped_router(value) {
                        routers.push(router);
                        wrapper_type.get_or_insert(kind);
                    }
                }
            }
        }
        if let Some(frame) = self.frames.last_mut().filter(|frame| frame.function) {
            frame.wrapped |= wrapper_type.is_some();
            if frame.wrapper_type.is_none() {
                frame.wrapper_type = wrapper_type;
            }
            frame.returns += 1;
            frame.router_returns += usize::from(!routers.is_empty());
            frame.returned.extend(routers);
        }
    }

    /// Binds the names of the named results of a function. A bare return gives their values.
    fn bind_results(&mut self, result: Option<Node<'t>>) {
        let Some(result) = result.filter(|result| result.kind() == "parameter_list") else {
            return;
        };
        let mut names = Vec::new();
        for declaration in Self::named_children(result) {
            let mut cursor = declaration.walk();
            for name in declaration.children_by_field_name("name", &mut cursor) {
                names.push(self.text(name).to_string());
            }
        }
        for name in &names {
            self.bind(name, None, true);
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.results = names;
        }
    }

    /// The router inside a literal that a function returns, as `&Server{Mux: router}` has.
    fn wrapped_router(&mut self, value: Node<'t>) -> Option<(usize, String)> {
        let literal = if value.kind() == "unary_expression" {
            value.named_child(0)?
        } else {
            value
        };
        if literal.kind() != "composite_literal" {
            return None;
        }
        let kind = self.text(literal.child_by_field_name("type")?).to_string();
        let mut values = Vec::new();
        Self::literal_values(literal.child_by_field_name("body")?, &mut values);
        values
            .into_iter()
            .find_map(|inner| self.eval(inner))
            .map(|router| (router, kind))
    }

    /// The router that an expression gives, when the syntax proves it.
    fn eval(&mut self, expression: Node<'t>) -> Option<usize> {
        // A call has one result. A chain that is deeper than the limit gives no router, so each
        // route on it gives no entry, and each call of it is read one time.
        let is_call = expression.kind() == "call_expression";
        if is_call {
            if let Some(known) = self.eval_memo.get(&expression.id()) {
                return *known;
            }
        }
        if self.eval_depth >= EVAL_DEPTH_LIMIT {
            if is_call {
                self.eval_memo.insert(expression.id(), None);
            }
            return None;
        }
        self.eval_depth += 1;
        let router = match expression.kind() {
            "parenthesized_expression" => {
                expression.named_child(0).and_then(|inner| self.eval(inner))
            }
            "identifier" => self.lookup(self.text(expression)).flatten(),
            "call_expression" => self.eval_call(expression),
            _ => None,
        };
        self.eval_depth -= 1;
        if is_call {
            self.eval_memo.insert(expression.id(), router);
        }
        router
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
            if let Some(framework) = self.framework_of(operand) {
                return self.construct(framework, name);
            }
        }
        if name == "Subrouter" {
            let child = self.subrouter(operand);
            if child.is_none() {
                self.push_unread_subrouter(operand, Span::of(&call));
            }
            return child;
        }
        let parent = self.eval(operand)?;
        match (self.routers[parent].framework, name) {
            (Framework::Chi, "With") => {
                let guards = self.guard_names(&arguments);
                let child = self.make_child(parent, String::new(), guards);
                self.routers[child].inline = true;
                self.routers[child].retroactive = false;
                self.routers[child].partial |= self.has_spread(&arguments);
                Some(child)
            }
            (Framework::Chi, "Route") => self.chi_route(parent, call, &arguments),
            (Framework::Chi, "Group") => self.chi_group(parent, call, &arguments),
            (Framework::Echo | Framework::Gin, "Group") => {
                let prefix = arguments
                    .first()
                    .and_then(|prefix| self.string_argument(*prefix));
                let Some(prefix) = prefix else {
                    // The routes of the group can have any path, so they replace routes.
                    self.push_opaque(parent, Span::of(&call));
                    return None;
                };
                let guards = self.guard_names(&arguments[1..]);
                let child = self.make_child(parent, prefix, guards);
                self.routers[child].partial |= self.has_spread(&arguments[1..]);
                Some(child)
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

    /// The names of the middleware in a list of arguments. A function literal or another
    /// expression has a label, so the list does not lose a middleware.
    fn guard_names(&self, arguments: &[Node<'t>]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| {
                self.context.go_guard_name(*argument).unwrap_or_else(|| {
                    match argument.kind() {
                        "func_literal" => UNNAMED_FUNCTION_GUARD,
                        _ => UNREAD_GUARD,
                    }
                    .to_string()
                })
            })
            .collect()
    }

    /// Adds the middleware of a call of `Use` or `Pre`. A list with `...` is not known, so
    /// the routes of the router give no entry.
    fn add_guards(&mut self, router: usize, arguments: &[Node<'t>], before_routing: bool) {
        if self.has_spread(arguments) {
            self.touch(router);
            self.routers[router].partial = true;
            return;
        }
        let guards = self.guard_names(arguments);
        self.touch(router);
        if before_routing {
            self.routers[router].pre.extend(guards);
        } else {
            self.routers[router].guards.extend(guards);
        }
    }

    /// Tells if an argument is a list with `...`. The walk cannot read its middleware.
    fn has_spread(&self, arguments: &[Node<'t>]) -> bool {
        arguments.iter().any(|argument| {
            argument.kind().contains("variadic")
                || (!self.context.is_string(*argument) && self.text(*argument).ends_with("..."))
        })
    }

    fn string_argument(&self, node: Node<'t>) -> Option<String> {
        self.context.go_string(node)
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

    /// Tells if the call is `Router().Add` of an echo value. It registers a route that omen
    /// cannot read, and the call is then an unread registration of the echo router.
    fn is_router_add(&mut self, function: Node<'t>) -> bool {
        let (Some(field), Some(operand)) = (
            function.child_by_field_name("field"),
            function.child_by_field_name("operand"),
        ) else {
            return false;
        };
        if self.text(field) != "Add" || operand.kind() != "call_expression" {
            return false;
        }
        let Some(inner) = operand
            .child_by_field_name("function")
            .filter(|inner| inner.kind() == "selector_expression")
        else {
            return false;
        };
        let (Some(name), Some(base)) = (
            inner.child_by_field_name("field"),
            inner.child_by_field_name("operand"),
        ) else {
            return false;
        };
        if !matches!(self.text(name), "Router" | "Routers") {
            return false;
        }
        match self.eval(base) {
            Some(router) if self.routers[router].framework == Framework::Echo => {
                self.push_opaque(router, Span::of(&operand));
                true
            }
            _ => false,
        }
    }

    /// A router that a call that is not a framework call or a server gets can get routes
    /// that omen does not read. Such a call is a registration that omen cannot read, at
    /// this place.
    fn escape_to_call(&mut self, node: Node<'t>, function: Node<'t>) {
        if self.is_root_sink(node) {
            return;
        }
        if self.is_router_add(function) {
            return;
        }
        if function.kind() == "selector_expression" {
            let Some(field) = function.child_by_field_name("field") else {
                return;
            };
            if matches!(self.text(field), "Handle" | "HandleFunc") {
                return;
            }
        }
        for argument in Context::arguments_of(node) {
            if !matches!(
                argument.kind(),
                "identifier" | "call_expression" | "parenthesized_expression"
            ) {
                continue;
            }
            if let Some(router) = self.eval(argument) {
                self.push_opaque(router, Span::of(&node));
            }
        }
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
            self.escape_to_call(node, function);
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
            self.escape_to_call(node, function);
            return;
        };
        let arguments = Context::arguments_of(node);
        let framework = self.routers[router].framework;
        match (framework, name) {
            (Framework::Gin, _) => self.gin_call(router, node, name, &arguments),
            (_, "Use") => self.add_guards(router, &arguments, false),
            (Framework::Echo, "Pre") => self.add_guards(router, &arguments, true),
            (Framework::Chi, _) => self.chi_call(router, node, name, &arguments),
            (Framework::Echo, _) => self.echo_call(router, node, name, &arguments),
            // With strict slash, a template matches the path with and without a final slash.
            (Framework::Mux, "StrictSlash") => {
                if arguments
                    .first()
                    .is_none_or(|value| self.text(*value) != "false")
                {
                    self.routers[router].partial = true;
                }
            }
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
        self.chi_mount_records(router, &prefix, Span::of(&call));
        let child = self.make_child(router, prefix, Vec::new());
        self.route_calls.insert(call.id(), child);
        self.callback(*callback, child);
        Some(child)
    }

    /// `Mount` of chi v5.3.2 registers its handler for all methods on the pattern, on the
    /// pattern and a slash, and on the pattern and `/*`. Each registration replaces a route
    /// of the same key, whether or not omen reads the mounted handler.
    fn chi_mount_records(&mut self, router: usize, prefix: &str, range: Span) {
        let keys = if prefix.ends_with('/') {
            vec![format!("{prefix}*")]
        } else {
            vec![
                prefix.to_string(),
                format!("{prefix}/"),
                format!("{prefix}/*"),
            ]
        };
        for key in keys {
            self.omit_next = true;
            self.mount_next = true;
            self.register(router, &["ANY"], &key, None, &[], range);
        }
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
        self.routers[child].inline = true;
        self.routers[child].retroactive = false;
        self.route_calls.insert(call.id(), child);
        self.callback(*callback, child);
        // A function that a name gives can register routes that omen does not read.
        if callback.kind() != "func_literal" {
            self.push_opaque(router, Span::of(&call));
        }
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
            // chi v5.3.2 `Handle` reads a method before a space or a tab in the pattern.
            ("HandleFunc" | "Handle", [_, handler], [Some(path), _]) => {
                match path.find([' ', '\t']) {
                    Some(space) => {
                        let method = path[..space].to_uppercase();
                        let rest = path[space + 1..].trim_start_matches([' ', '\t']);
                        if CHI_METHODS.contains(&method.as_str()) {
                            self.register(
                                router,
                                &[method.as_str()],
                                rest,
                                Some(*handler),
                                &[],
                                range,
                            );
                        }
                    }
                    None => self.register(router, &["ANY"], path, Some(*handler), &[], range),
                }
            }
            ("Method" | "MethodFunc", [_, _, handler], [Some(method), Some(path), _]) => {
                let method = method.to_uppercase();
                if CHI_METHODS.contains(&method.as_str()) {
                    self.register(router, &[method.as_str()], path, Some(*handler), &[], range);
                }
            }
            ("Route", _, _) => {
                if self.chi_route(router, node, arguments).is_none() {
                    self.push_opaque(router, range);
                }
            }
            ("Group", _, _) => {
                self.chi_group(router, node, arguments);
            }
            ("Mount", [_, mounted], [Some(prefix), _]) => {
                self.chi_mount_records(router, prefix, range);
                self.mount(router, prefix, *mounted)
            }
            (
                "Get" | "Post" | "Put" | "Patch" | "Delete" | "Head" | "Options" | "Connect"
                | "Trace" | "Query" | "HandleFunc" | "Handle" | "Method" | "MethodFunc" | "Mount",
                _,
                _,
            ) => self.push_opaque(router, range),
            _ => {}
        }
    }

    /// A sub-router that omen cannot read can have routes that match first, so it is an unread
    /// registration of the router that the chain starts from.
    fn push_unread_subrouter(&mut self, operand: Node<'t>, range: Span) {
        let mut current = operand;
        for _ in 0..EVAL_DEPTH_LIMIT {
            if let Some(router) = self.eval(current) {
                self.push_opaque(router, range);
                return;
            }
            let Some(next) = current
                .child_by_field_name("function")
                .filter(|function| function.kind() == "selector_expression")
                .and_then(|function| function.child_by_field_name("operand"))
            else {
                return;
            };
            current = next;
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
        let valid = mux_template_valid(&prefix);
        let child = self.make_child(parent, prefix, Vec::new());
        self.routers[child].partial |= !valid;
        Some(child)
    }

    /// Tells if a call is a part of a chain that ends in `Subrouter`.
    fn is_subrouter_operand(&self, call: Node<'t>) -> bool {
        let mut current = call;
        loop {
            // `Node::parent` searches down from the root. Thus the check of the source text
            // comes first: only a `.` or a comment can follow the operand of a selector.
            let next = self.context.source[current.end_byte()..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace());
            if !matches!(next, Some(b'.' | b'/')) {
                return false;
            }
            let Some(selector) = current.parent().filter(|parent| {
                parent.kind() == "selector_expression"
                    && parent.child_by_field_name("operand") == Some(current)
            }) else {
                return false;
            };
            let Some(outer) = selector
                .parent()
                .filter(|parent| parent.kind() == "call_expression")
            else {
                return false;
            };
            let name = selector
                .child_by_field_name("field")
                .map_or("", |field| self.text(field));
            if name == "Subrouter" {
                return true;
            }
            if !self.is_mux_matcher_call(outer) {
                return false;
            }
            current = outer;
        }
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
                        | "BuildVarsFunc"
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
        if self.scanned.contains(&outer.id()) {
            return false;
        }
        if self.is_subrouter_operand(outer) {
            return false;
        }
        let outer_name = outer
            .child_by_field_name("function")
            .and_then(|function| function.child_by_field_name("field"));
        let mut chain = Vec::new();
        let mut method_lists: Vec<Option<Vec<String>>> = Vec::new();
        let mut paths: Vec<Option<String>> = Vec::new();
        let mut handler: Option<Node<'t>> = None;
        let mut current = outer;
        let mut unreadable = false;
        let mut broken = false;
        let mut name_calls: Vec<Option<String>> = Vec::new();
        let mut terminal_prefix: Option<bool> = None;
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
                    terminal_prefix.get_or_insert(false);
                    let template = arguments
                        .first()
                        .and_then(|path| self.string_argument(*path));
                    broken |= template
                        .as_deref()
                        .is_some_and(|text| !mux_template_valid(text));
                    paths.push(template);
                    handler = arguments.get(1).copied();
                    break operand;
                }
                "HandlerFunc" | "Handler" if handler.is_none() => {
                    handler = arguments.first().copied();
                }
                "Methods" => {
                    unreadable |= arguments.is_empty();
                    let names: Vec<String> = arguments
                        .iter()
                        .filter_map(|argument| self.string_argument(*argument))
                        .collect();
                    unreadable |= names.iter().any(|name| !is_http_token(name));
                    method_lists.push(
                        (names.len() == arguments.len())
                            .then(|| names.iter().map(|name| name.to_uppercase()).collect()),
                    );
                }
                "Path" | "PathPrefix" => {
                    terminal_prefix.get_or_insert(self.text(field) == "PathPrefix");
                    let template = arguments
                        .first()
                        .and_then(|path| self.string_argument(*path));
                    broken |= template
                        .as_deref()
                        .is_some_and(|text| !mux_template_valid(text));
                    paths.push(template);
                }
                // gorilla/mux v1.8.1 sets `Route.err` when the pairs are not even, and the
                // route never matches.
                "Headers" => {
                    if arguments
                        .iter()
                        .any(|argument| self.string_argument(*argument).is_none())
                    {
                        unreadable = true;
                    } else {
                        broken |= !arguments.len().is_multiple_of(2);
                    }
                }
                "Name" => name_calls.push(
                    arguments
                        .first()
                        .and_then(|name| self.string_argument(*name)),
                ),
                // A matcher of schemes or methods that has no value rejects each request.
                // A handler that runs before the router can set `URL.Scheme`, so omen accepts only
                // `http` and `https`.
                "Schemes" => {
                    unreadable |= arguments.is_empty()
                        || arguments.iter().any(|argument| {
                            self.string_argument(*argument).is_none_or(|scheme| {
                                !matches!(scheme.to_lowercase().as_str(), "http" | "https")
                            })
                        });
                }
                "MatcherFunc" => {}
                "BuildVarsFunc" => {}
                // `BuildOnly` makes a route that never matches.
                "BuildOnly" => broken = true,
                // A matcher with a template or a regular expression can set an error that
                // omen does not check, and another call is not known.
                _ => unreadable = true,
            }
            if operand.kind() == "call_expression"
                && (unreadable || broken || self.is_mux_matcher_call(operand))
            {
                current = operand;
            } else {
                break operand;
            }
        };
        self.scanned.extend(chain.iter().copied());
        let Some(router) = self.eval(base) else {
            return false;
        };
        if self.routers[router].framework != Framework::Mux {
            return false;
        }
        let Some(handler) = handler else {
            if matches!(
                outer_name.map(|name| self.text(name)).unwrap_or(""),
                "Path"
                    | "PathPrefix"
                    | "Methods"
                    | "Host"
                    | "Headers"
                    | "Queries"
                    | "Schemes"
                    | "MatcherFunc"
                    | "NewRoute"
                    | "Name"
                    | "BuildVarsFunc"
            ) {
                self.push_opaque(router, Span::of(&outer));
            }
            return false;
        };
        self.consumed.extend(chain);
        // gorilla/mux v1.8.1 sets an error when a route has a name and gets another name.
        // The error is set only when the current name is not empty.
        name_calls.reverse();
        let mut current_name = String::new();
        for call in name_calls {
            match call {
                Some(name) if current_name.is_empty() => current_name = name,
                Some(_) => broken = true,
                None => unreadable = true,
            }
        }
        if unreadable {
            return self.opaque_registration(router, outer);
        }
        if broken {
            return true;
        }
        paths.reverse();
        let Some(paths) = paths.into_iter().collect::<Option<Vec<String>>>() else {
            return self.opaque_registration(router, outer);
        };
        // Each matcher of a route must match, so a second path matcher is not read.
        let [last] = paths.as_slice() else {
            return self.opaque_registration(router, outer);
        };
        // The router cleans the path of a request, so a template that is not clean never matches
        // unless the router skips the cleaning.
        if !mux_template_is_clean(last) {
            return self.opaque_registration(router, outer);
        }
        let path = if last.is_empty() {
            // The empty prefix matches each path.
            if terminal_prefix != Some(true) {
                return self.opaque_registration(router, outer);
            }
            String::new()
        } else {
            let Some(path) = Framework::Mux.compose(&[], last) else {
                return true;
            };
            path
        };
        let mut methods: Option<Vec<String>> = None;
        for list in method_lists {
            let Some(list) = list else {
                return self.opaque_registration(router, outer);
            };
            methods = Some(match methods {
                None => list,
                Some(known) => {
                    let allowed: HashSet<&String> = list.iter().collect();
                    known
                        .into_iter()
                        .filter(|method| allowed.contains(method))
                        .collect()
                }
            });
        }
        let methods = methods.unwrap_or_else(|| vec!["ANY".to_string()]);
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
        self.prefix_next = terminal_prefix.unwrap_or(false);
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
                self.add_guards(router, arguments, false);
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
        // These calls register a GET route for a file, or for the files below a prefix. Omen
        // does not read their handler, but they replace a route of the same path.
        if matches!(name, "File" | "FileFS" | "Static" | "StaticFS") {
            let suffix = if name.starts_with("Static") { "*" } else { "" };
            let Some(path) = arguments
                .first()
                .and_then(|path| self.string_argument(*path))
            else {
                return self.push_opaque(router, range);
            };
            self.omit_next = true;
            self.register(
                router,
                &["GET"],
                &format!("{path}{suffix}"),
                None,
                &[],
                range,
            );
            return;
        }
        // A method that omen cannot read can be any method, a custom method too, so the route
        // replaces the routes of its path for each method. Omen omits its entry.
        let (methods, rest, unread): (Vec<String>, &[Node<'t>], bool) = match (name, arguments) {
            (_, [_, ..]) if verb => (vec![name.to_string()], arguments, false),
            ("Any", [_, ..]) => (ECHO_ANY.map(String::from).to_vec(), arguments, false),
            ("Match", [methods, rest @ ..]) => match self.string_list(*methods) {
                Some(methods) => (methods, rest, false),
                None => (vec![UNREAD_METHOD.to_string()], rest, true),
            },
            ("Add", [method, rest @ ..]) => match self.string_argument(*method) {
                // echo keeps this method as the handler for a path that has no route.
                Some(method) if method == "echo_route_not_found" => return,
                Some(method) => (vec![method], rest, false),
                None => (vec![UNREAD_METHOD.to_string()], rest, true),
            },
            _ => return,
        };
        let (Some(path), Some(handler)) = (rest.first(), rest.get(1)) else {
            return self.push_opaque(router, range);
        };
        let Some(path) = self.string_argument(*path) else {
            return self.push_opaque(router, range);
        };
        // echo stores a route that has `*` in the middle of its path on the node of the `*`.
        let (path, cut) = echo_cut_at_star(&path);
        let methods: Vec<&str> = methods.iter().map(String::as_str).collect();
        self.omit_next = unread || cut;
        self.register(router, &methods, path, Some(*handler), &rest[2..], range);
    }

    /// Joins a router that the file makes to the router that mounts it. A router can have
    /// more than one mount. The walk does not follow a router of another file.
    fn mount(&mut self, parent: usize, prefix: &str, mounted: Node<'t>) {
        let Some(child) = self.eval(mounted) else {
            return;
        };
        if !self.routers[child].mountable
            || self.mounts.contains(&(child, parent, prefix.to_string()))
        {
            return;
        }
        // A router that two mounts attach has more than one place. The walk keeps the
        // mounts as a forest, so the router and its routes give no entry.
        if !self.mounted.insert(child) {
            self.routers[child].partial = true;
            return;
        }
        if self.find(parent) == self.find(child) {
            return;
        }
        self.mounts.insert((child, parent, prefix.to_string()));
        let link = Link {
            parent,
            parent_len: self.routers[parent].guards.len(),
            prefix: prefix.to_string(),
            at: self.records.len(),
        };
        self.touch(parent);
        self.touch(child);
        self.routers[child].links.push(link);
        self.join(parent, child);
    }

    /// The root of the tree of routers that holds this router.
    fn find(&mut self, mut router: usize) -> usize {
        while self.tree[router] != router {
            self.tree[router] = self.tree[self.tree[router]];
            router = self.tree[router];
        }
        router
    }

    fn join(&mut self, parent: usize, child: usize) {
        let (parent, child) = (self.find(parent), self.find(child));
        self.tree[child] = parent;
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
        let prefix = std::mem::take(&mut self.prefix_next);
        let mount = std::mem::take(&mut self.mount_next);
        let omitted = std::mem::take(&mut self.omit_next)
            || self.has_spread(guard_arguments)
            || handler.is_some_and(|node| self.has_spread(&[node]));
        if omitted && self.routers[router].framework == Framework::Mux {
            self.push_opaque(router, range);
            return;
        }
        self.touch(router);
        let kind = self.routers[router].framework.handler_kind();
        let handler = handler
            .filter(|_| !omitted)
            .map(|node| self.context.go_handler(node, kind));
        let mut guards = self.guard_names(guard_arguments);
        let unread_wrapper = handler
            .as_ref()
            .is_some_and(|handler| handler.unread_wrapper);
        let symbol = handler.map(|handler| {
            guards.extend(handler.guards);
            handler.symbol
        });
        let owner = self.routers[router].owner;
        self.routed_owners.insert(owner);
        let limit = self.routers[router].guards.len();
        for method in methods {
            self.records.push(Record {
                router,
                prefix,
                opaque: false,
                omitted,
                mount,
                limit,
                range,
                symbol: symbol.clone().flatten(),
                method: method.to_string(),
                path: path.to_string(),
                guards: guards.clone(),
                unread_wrapper,
            });
        }
    }

    /// Keeps the place of a registration that omen cannot read. A gorilla/mux route that it
    /// can match is not known, so the routes after it give no entry.
    fn push_opaque(&mut self, router: usize, range: Span) {
        if self.routers[router].framework == Framework::Gin {
            return;
        }
        self.records.push(Record {
            router,
            prefix: true,
            opaque: true,
            omitted: false,
            mount: false,
            limit: 0,
            range,
            symbol: None,
            method: "ANY".to_string(),
            path: String::new(),
            guards: Vec::new(),
            unread_wrapper: false,
        });
    }

    fn opaque_registration(&mut self, router: usize, outer: Node<'t>) -> bool {
        self.push_opaque(router, Span::of(&outer));
        true
    }

    /// The number of guards of a router that a route can have. `limit` is the number of
    /// guards that the router had at the time of the route. A router with retroactive
    /// guards has all of them.
    fn visible_guards(router: &Router, limit: Option<usize>) -> usize {
        match limit {
            Some(count) if !router.retroactive => count.min(router.guards.len()),
            _ => router.guards.len(),
        }
    }

    /// The prefixes and the guards of a router at this time. The walk goes up from the
    /// router one time. A router that has no place that the file proves gives none.
    fn place_now(&self, id: usize, limit: Option<usize>) -> Option<Lineage> {
        let mut chain = vec![id];
        let mut current = id;
        loop {
            let router = &self.routers[current];
            if router.partial {
                return None;
            }
            match router.links.as_slice() {
                [] => break,
                [link] => {
                    current = link.parent;
                    chain.push(current);
                }
                _ => return None,
            }
        }
        let root = *chain.last()?;
        let mut prefixes = self.routers[root].base.clone();
        let mut guards: Vec<String> = Vec::new();
        for pair in chain.windows(2).rev() {
            let (child, parent) = (pair[0], pair[1]);
            let link = &self.routers[child].links[0];
            let above = &self.routers[parent];
            let above_limit = (!above.retroactive).then_some(link.parent_len);
            guards.extend(above.pre.iter().cloned());
            let count = Self::visible_guards(above, above_limit);
            guards.extend(above.guards[..count].iter().cloned());
            if !link.prefix.is_empty() {
                prefixes.push(link.prefix.clone());
            }
        }
        let router = &self.routers[id];
        guards.extend(router.pre.iter().cloned());
        let count = Self::visible_guards(router, limit);
        guards.extend(router.guards[..count].iter().cloned());
        Some(Lineage { prefixes, guards })
    }

    /// The prefixes and the guards that the routers above a router give. Each router has
    /// one result, which shares its list with the result of its parent.
    fn up_of(&self, id: usize, memo: &mut [Option<Up>]) {
        let mut path = Vec::new();
        let mut current = id;
        while memo[current].is_none() {
            path.push(current);
            match self.routers[current].links.as_slice() {
                [link] => current = link.parent,
                _ => break,
            }
        }
        for node in path.into_iter().rev() {
            let up = self.compute_up(node, memo);
            memo[node] = Some(up);
        }
    }

    fn compute_up(&self, node: usize, memo: &[Option<Up>]) -> Up {
        let router = &self.routers[node];
        match router.links.as_slice() {
            [] => Up {
                state: router
                    .base
                    .iter()
                    .try_fold(PrefixState::default(), |state, part| {
                        router.framework.extend(&state, part)
                    }),
                guards: None,
                dead: false,
            },
            [link] => {
                let above = &self.routers[link.parent];
                let Some(parent) = memo[link.parent].as_ref() else {
                    return Up::dead();
                };
                let above_limit = (!above.retroactive).then_some(link.parent_len);
                let count = Self::visible_guards(above, above_limit);
                let mut own = above.pre.clone();
                own.extend(above.guards[..count].iter().cloned());
                Up {
                    state: parent
                        .state
                        .as_ref()
                        .and_then(|state| router.framework.extend(state, &link.prefix)),
                    guards: push_chain(&parent.guards, own),
                    dead: parent.dead || above.partial,
                }
            }
            _ => Up::dead(),
        }
    }

    fn finish(self, out: &mut Vec<Draft>) {
        let mut memo: Vec<Option<Up>> = (0..self.routers.len()).map(|_| None).collect();
        // The tree of a chi router is the tree of the router that `With` or `Group` came
        // from. A parent has a lower number than its child.
        let mut tree_of: Vec<usize> = Vec::with_capacity(self.routers.len());
        let mut root_of: Vec<usize> = Vec::with_capacity(self.routers.len());
        for (id, router) in self.routers.iter().enumerate() {
            let parent = match router.links.as_slice() {
                [link] if link.parent < id => Some(link.parent),
                _ => None,
            };
            root_of.push(parent.map_or(id, |parent| root_of[parent]));
            tree_of.push(match parent {
                Some(parent) if router.inline => tree_of[parent],
                _ => id,
            });
        }
        let mut blocked: HashSet<usize> = self
            .attached
            .iter()
            .filter_map(|name| self.components.get(name))
            .flatten()
            .copied()
            .collect();
        // A caller that registers on the result of a factory adds routes to the router of the
        // factory. When the factory has one call, the routes of both are in one order, the order
        // of the registrations, and the rules of the framework apply to them. Otherwise omen
        // cannot order them, so the routes of both give no entry.
        let mut mapping: Vec<usize> = (0..self.routers.len()).collect();
        let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
        if !self.registered_fresh.is_empty() {
            for (router, root) in root_of.iter().enumerate() {
                if self.fresh_of.contains_key(root) {
                    members.entry(*root).or_default().push(router);
                }
            }
        }
        for (fresh, name) in &self.fresh_of {
            if !self.registered_fresh.contains(name) {
                continue;
            }
            let own: Vec<usize> = members.get(fresh).cloned().unwrap_or_default();
            let returned = self.returned_of.get(name).copied();
            let exact = self.fresh_calls.get(name) == Some(&1)
                && !self.attached.contains(name)
                && self.routers[*fresh].base.is_empty()
                && !self.routers[*fresh].partial;
            match returned.filter(|_| exact) {
                Some(returned) => {
                    mapping[*fresh] = returned;
                    for router in own {
                        root_of[router] = root_of[returned];
                        if tree_of[router] == *fresh {
                            tree_of[router] = tree_of[returned];
                        }
                    }
                }
                None => {
                    blocked.extend(self.components.get(name).into_iter().flatten().copied());
                    blocked.extend(own);
                }
            }
        }
        // chi's `Mount` registers the pattern, the pattern and a slash, and the pattern and `/*`
        // for all methods. A route that has one of these keys on the same tree replaces the
        // mount, or the mount panics. Then the routers below the mount give no entry.
        let mut chi_keys: HashMap<usize, Vec<String>> = HashMap::new();
        let mut chi_latest: HashMap<usize, HashMap<String, usize>> = HashMap::new();
        let mut chi_opaque: HashSet<usize> = HashSet::new();
        let mut echo_opaque: HashSet<usize> = HashSet::new();
        for (index, record) in self.records.iter().enumerate() {
            let framework = self.routers[record.router].framework;
            if record.opaque {
                match framework {
                    Framework::Chi => chi_opaque.insert(tree_of[record.router]),
                    Framework::Echo => echo_opaque.insert(root_of[record.router]),
                    _ => false,
                };
            } else if framework == Framework::Chi && !record.mount {
                let key = chi_tree_key(&record.path);
                chi_latest
                    .entry(tree_of[record.router])
                    .or_default()
                    .insert(key.clone(), index);
                chi_keys
                    .entry(tree_of[record.router])
                    .or_default()
                    .push(key);
            }
        }
        for keys in chi_keys.values_mut() {
            keys.sort_unstable();
            keys.dedup();
        }
        let own_replaced: Vec<bool> = self
            .routers
            .iter()
            .map(|router| match router.links.as_slice() {
                [link] => {
                    router.framework == Framework::Chi
                        && !router.inline
                        && tree_of.get(link.parent).is_some_and(|tree| {
                            chi_opaque.contains(tree)
                                || chi_keys
                                    .get(tree)
                                    .is_some_and(|keys| has_key_under(keys, &link.prefix))
                                || chi_latest.get(tree).is_some_and(|latest| {
                                    chi_mount_keys(&link.prefix)
                                        .iter()
                                        .any(|key| latest.get(key).is_some_and(|at| *at >= link.at))
                                })
                        })
                }
                _ => false,
            })
            .collect();
        // The mounts form a forest, so each router has one walk up to a router that is known.
        let mut mount_replaced: Vec<Option<bool>> = vec![None; self.routers.len()];
        for start in 0..self.routers.len() {
            let mut path = Vec::new();
            let mut current = start;
            let result = loop {
                if let Some(known) = mount_replaced[current] {
                    break known;
                }
                path.push(current);
                if own_replaced[current] {
                    break true;
                }
                match self.routers[current].links.as_slice() {
                    [link] => current = link.parent,
                    _ => break false,
                }
            };
            for node in path {
                mount_replaced[node] = Some(result);
            }
        }
        let mount_replaced: Vec<bool> = mount_replaced
            .into_iter()
            .map(|known| known == Some(true))
            .collect();
        // The draft of each record, and the key that decides which records replace each other.
        let mut drafts: Vec<Option<(Draft, Duplicate)>> = Vec::new();
        // The records of a router that omen marks partial still replace routes, and omen omits
        // their entries.
        let mut forced: HashSet<usize> = HashSet::new();
        for record in &self.records {
            drafts.push(None);
            if record.opaque || blocked.contains(&record.router) || mount_replaced[record.router] {
                continue;
            }
            let router = &self.routers[record.router];
            self.up_of(record.router, &mut memo);
            let Some(up) = memo[record.router].as_ref() else {
                continue;
            };
            let unreadable = router.partial || up.dead;
            let framework = router.framework;
            if unreadable && framework == Framework::Mux {
                continue;
            }
            let composed = up
                .state
                .as_ref()
                .filter(|_| !up.dead)
                .and_then(|state| framework.extend(state, &record.path))
                .and_then(|state| framework.materialize(&state));
            let path = match composed {
                Some(path) => path,
                None if unreadable => {
                    if framework == Framework::Echo {
                        echo_opaque.insert(root_of[record.router]);
                    }
                    record.path.clone()
                }
                None => continue,
            };
            if unreadable {
                forced.insert(drafts.len() - 1);
            }
            let mut guards = flatten_chain(&up.guards);
            guards.extend(router.pre.iter().cloned());
            let count = Self::visible_guards(router, Some(record.limit));
            guards.extend(router.guards[..count].iter().cloned());
            guards.extend(record.guards.iter().cloned());
            let duplicate = match framework {
                Framework::Chi => {
                    Duplicate::Chi(tree_of[record.router], chi_tree_key(&record.path))
                }
                Framework::Echo => Duplicate::Echo(root_of[record.router], echo_shape(&path)),
                Framework::Mux | Framework::Gin => Duplicate::None,
            };
            let draft = Draft {
                range: record.range,
                kind: EntryKind::HttpHandler,
                symbol: record.symbol.clone(),
                framework: framework.name(),
                route: Some(Route {
                    method: record.method.clone(),
                    parameters: path_parameters(framework, &path),
                    path: Some(path),
                    path_template: None,
                }),
                guards,
                basis: EntryBasis::Registered,
                variant: None,
                inline_handler: None,
                unknown: record
                    .unread_wrapper
                    .then_some(GuardsUnknownReason::UnreadMiddleware),
            };
            let last = drafts.len() - 1;
            drafts[last] = Some((draft, duplicate));
        }
        let mut kept = Self::survivors(&self.records, &drafts);
        let mux_roots: Vec<Option<usize>> = self
            .records
            .iter()
            .map(|record| {
                (self.routers[record.router].framework == Framework::Mux)
                    .then_some(root_of[record.router])
            })
            .collect();
        mux_order(&self.records, &drafts, &mux_roots, &mapping, &mut kept);
        let chi_trees: Vec<Option<usize>> = self
            .records
            .iter()
            .map(|record| {
                (self.routers[record.router].framework == Framework::Chi)
                    .then_some(tree_of[record.router])
            })
            .collect();
        chi_precedence(&self.records, &chi_trees, &mut kept);
        let echo_roots: Vec<Option<usize>> = self
            .records
            .iter()
            .map(|record| {
                (self.routers[record.router].framework == Framework::Echo)
                    .then_some(root_of[record.router])
            })
            .collect();
        echo_precedence(&self.records, &drafts, &echo_roots, &mut kept);
        for (index, record) in self.records.iter().enumerate() {
            let router = record.router;
            let hidden = match self.routers[router].framework {
                Framework::Chi => chi_opaque.contains(&tree_of[router]),
                Framework::Echo => echo_opaque.contains(&root_of[router]),
                _ => false,
            };
            if hidden || record.omitted || forced.contains(&index) {
                kept[index] = false;
            }
        }
        for (index, draft) in drafts.into_iter().enumerate() {
            if let (Some((draft, _)), true) = (draft, kept[index]) {
                out.push(draft);
            }
        }
    }

    /// Tells for each record if a later registration replaces it. chi and echo keep the last
    /// registration of a method and a path. gin and `net/http` panic on a second registration,
    /// so the walk does not read such a program. `mux_order` handles gorilla/mux.
    fn survivors(records: &[Record], drafts: &[Option<(Draft, Duplicate)>]) -> Vec<bool> {
        let mut kept = vec![true; drafts.len()];
        let mut owners: HashMap<(usize, String), HashMap<String, usize>> = HashMap::new();
        let mut last_echo: HashMap<(usize, String, String), usize> = HashMap::new();
        let mut echo_seen: HashMap<(usize, String), Vec<usize>> = HashMap::new();
        for (index, draft) in drafts.iter().enumerate() {
            let Some((_, duplicate)) = draft else {
                continue;
            };
            let method = records[index].method.clone();
            match duplicate {
                Duplicate::None => {}
                Duplicate::Chi(tree, pattern) => {
                    let owned = owners.entry((*tree, pattern.clone())).or_default();
                    if method == "ANY" {
                        for known in CHI_METHODS {
                            owned.insert(known.to_string(), index);
                        }
                    } else {
                        owned.insert(method, index);
                    }
                }
                Duplicate::Echo(root, shape) => {
                    let seen = echo_seen.entry((*root, shape.clone())).or_default();
                    if method == UNREAD_METHOD {
                        for earlier in seen.drain(..) {
                            kept[earlier] = false;
                        }
                    }
                    seen.push(index);
                    if let Some(earlier) = last_echo.insert((*root, method, shape.clone()), index) {
                        kept[earlier] = false;
                    }
                }
            }
        }
        for (index, draft) in drafts.iter().enumerate() {
            if let Some((_, Duplicate::Chi(tree, pattern))) = draft {
                let Some(owned) = owners.get(&(*tree, pattern.clone())) else {
                    continue;
                };
                let method = &records[index].method;
                kept[index] = if method == "ANY" {
                    CHI_METHODS
                        .iter()
                        .all(|known| owned.get(*known) == Some(&index))
                } else {
                    owned.get(method) == Some(&index)
                };
            }
        }
        kept
    }
}

/// The method of a registration whose method omen cannot read. It stands for each method.
const UNREAD_METHOD: &str = "*";

/// How a record can replace or hide another record.
enum Duplicate {
    None,
    /// The tree of the chi router, and the pattern.
    Chi(usize, String),
    /// The router of echo, and the path with the names of the parameters removed.
    Echo(usize, String),
}

/// Tells if a route of the tree has a key below a mount prefix. chi tries the static route
/// or the parameter route before the catch-all of the mount.
fn has_key_under(keys: &[String], prefix: &str) -> bool {
    let start = if prefix.ends_with('/') {
        chi_tree_key(prefix)
    } else {
        chi_tree_key(&format!("{prefix}/"))
    };
    let first = keys.partition_point(|key| *key < start);
    keys.get(first).is_some_and(|key| key.starts_with(&start))
}

/// The keys of the nodes that `Mount` of chi v5.3.2 registers for a pattern.
fn chi_mount_keys(pattern: &str) -> Vec<String> {
    if pattern.ends_with('/') {
        return vec![chi_tree_key(&format!("{pattern}*"))];
    }
    vec![
        chi_tree_key(pattern),
        chi_tree_key(&format!("{pattern}/")),
        chi_tree_key(&format!("{pattern}/*")),
    ]
}

/// The key that the tree of chi gives to a pattern. Two patterns that differ only by the names
/// of their parameters have one node. This follows `patNextSegment` of chi v5.3.2. A pattern
/// that chi does not accept keeps its text.
fn chi_tree_key(pattern: &str) -> String {
    let mut key = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        if rest[..start].contains('*') {
            return pattern.to_string();
        }
        key.push_str(&rest[..start]);
        let mut depth = 0_usize;
        let mut end = None;
        for (offset, character) in rest[start..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return pattern.to_string();
        };
        match rest[start + 1..end].split_once(':') {
            None => key.push_str("{}"),
            Some((_, expression)) => {
                key.push_str("{:");
                if !expression.is_empty() {
                    if !expression.starts_with('^') {
                        key.push('^');
                    }
                    key.push_str(expression);
                    if !expression.ends_with('$') {
                        key.push('$');
                    }
                }
                key.push('}');
            }
        }
        rest = &rest[end + 1..];
    }
    key.push_str(rest);
    key
}

/// The path of an echo route with the names of the parameters removed. echo uses one node
/// for two paths that differ only by these names.
fn echo_shape(path: &str) -> String {
    let mut shape = String::with_capacity(path.len());
    let mut skipping = false;
    let mut previous = ' ';
    for character in path.chars() {
        if skipping {
            if character == '/' {
                skipping = false;
                shape.push(character);
            }
        } else if character == ':' && previous != '\\' {
            skipping = true;
            shape.push(':');
        } else {
            shape.push(character);
        }
        previous = character;
    }
    shape
}

/// A part of a joined path, with the part before it.
struct Piece {
    text: String,
    parent: Option<Rc<Piece>>,
}

/// A joined path of the routers above a router. `slash` tells if the last part that is
/// not empty ends in a slash, which gin keeps.
#[derive(Clone, Default)]
struct PrefixState {
    top: Option<Rc<Piece>>,
    slash: bool,
}

impl Drop for Piece {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(mut inner) => next = inner.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// A list of strings that shares its start with the list of the router above.
struct Chain {
    items: Vec<String>,
    parent: Option<Rc<Chain>>,
}

/// The prefixes and the guards that the routers above a router give.
struct Up {
    state: Option<PrefixState>,
    guards: Option<Rc<Chain>>,
    /// A router above is not known, or a router has more than one place.
    dead: bool,
}

impl Up {
    fn dead() -> Self {
        Self {
            state: None,
            guards: None,
            dead: true,
        }
    }
}

fn push_chain(parent: &Option<Rc<Chain>>, items: Vec<String>) -> Option<Rc<Chain>> {
    if items.is_empty() {
        parent.clone()
    } else {
        Some(Rc::new(Chain {
            items,
            parent: parent.clone(),
        }))
    }
}

fn flatten_chain(chain: &Option<Rc<Chain>>) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = chain.as_ref();
    while let Some(node) = current {
        segments.push(&node.items);
        current = node.parent.as_ref();
    }
    segments.into_iter().rev().flatten().cloned().collect()
}

/// A long list drops one node at a time, so a deep chain does not overflow the stack.
impl Drop for Chain {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(mut inner) => next = inner.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// Tells if a text is an HTTP token, as a method name must be.
pub(super) fn is_http_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Tells if a template has no empty segment, no `.` and no `..` segment. The router redirects a
/// request path that is not clean.
fn mux_template_is_clean(template: &str) -> bool {
    let mut segments = template.split('/').skip(1).peekable();
    while let Some(segment) = segments.next() {
        let last = segments.peek().is_none();
        if segment == "." || segment == ".." || (segment.is_empty() && !last) {
            return false;
        }
    }
    true
}

/// What a route path can match. A variable that has no constraint matches one segment, so a
/// finite set of paths cannot cover it. A constraint can limit a variable to a few values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Exact,
    Prefix,
    Variable,
    Constrained,
}

/// A trie of the literal start of path templates. A template is open when it matches more
/// than one path: it has a variable, or it is a prefix. Otherwise it matches one path.
#[derive(Default)]
struct PathTrie {
    edges: HashMap<(usize, u8), usize>,
    nodes: Vec<TrieNode>,
}

#[derive(Default, Clone)]
struct TrieNode {
    open: usize,
    exact: usize,
    below: usize,
}

impl PathTrie {
    fn insert(&mut self, literal: &str, kind: PathKind) {
        let open = kind != PathKind::Exact;
        if self.nodes.is_empty() {
            self.nodes.push(TrieNode::default());
        }
        let mut node = 0;
        for byte in literal.bytes() {
            self.nodes[node].below += 1;
            let next = match self.edges.get(&(node, byte)) {
                Some(next) => *next,
                None => {
                    self.nodes.push(TrieNode::default());
                    let next = self.nodes.len() - 1;
                    self.edges.insert((node, byte), next);
                    next
                }
            };
            node = next;
        }
        if open {
            self.nodes[node].open += 1;
        } else {
            self.nodes[node].exact += 1;
        }
    }

    /// The number of inserted templates that can match a path that the template can match.
    /// The count is not less than the true number.
    fn overlaps(&self, literal: &str, kind: PathKind) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        let mut node = 0;
        let mut count = self.nodes[0].open;
        for byte in literal.bytes() {
            let Some(next) = self.edges.get(&(node, byte)) else {
                return count;
            };
            node = *next;
            count += self.nodes[node].open;
        }
        if matches!(kind, PathKind::Exact | PathKind::Constrained) {
            count += self.nodes[node].exact;
        }
        if kind == PathKind::Constrained {
            count += self.nodes[node].below;
        }
        count
    }
}

/// The tries of one set of records, one for all methods and one for each method.
#[derive(Default)]
struct MethodTries {
    all: PathTrie,
    by_method: HashMap<String, PathTrie>,
}

impl MethodTries {
    fn insert(&mut self, method: &str, literal: &str, kind: PathKind) {
        self.all.insert(literal, kind);
        self.by_method
            .entry(method.to_string())
            .or_default()
            .insert(literal, kind);
    }

    /// Counts the templates that have a method in common with `method`.
    fn overlaps(&self, method: &str, literal: &str, kind: PathKind) -> usize {
        if method == "ANY" {
            return self.all.overlaps(literal, kind);
        }
        [method, "ANY"]
            .iter()
            .filter_map(|name| self.by_method.get(*name))
            .map(|trie| trie.overlaps(literal, kind))
            .sum()
    }
}

/// The literal start of a route path, and if the path matches more than one value.
fn mux_literal(path: &str, prefix: bool) -> (&str, PathKind) {
    match path.find('{') {
        Some(start) if path[start..].contains(':') => (&path[..start], PathKind::Constrained),
        Some(start) => (&path[..start], PathKind::Variable),
        None if prefix => (path, PathKind::Prefix),
        None => (path, PathKind::Exact),
    }
}

/// gorilla/mux gives a request to the first route that matches. This clears `kept` for a
/// route that an earlier route can hide. A route is kept only when no earlier route of its
/// router, and no route of another router of the same tree, can match a path that it
/// matches. A registration that omen cannot read can match any path. The order of the
/// routers of one tree is not known, so a route of another router counts as earlier. The
/// check does not compare a variable or a matcher, so it can drop a route that is reachable.
fn mux_order(
    records: &[Record],
    drafts: &[Option<(Draft, Duplicate)>],
    roots: &[Option<usize>],
    mapping: &[usize],
    kept: &mut [bool],
) {
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, root) in roots.iter().enumerate() {
        if let Some(root) = root {
            members.entry(*root).or_default().push(index);
        }
    }
    for indexes in members.values() {
        let readable = |index: usize| -> Option<(&str, PathKind, &str)> {
            let record = &records[index];
            let (draft, _) = drafts[index].as_ref()?;
            if record.opaque {
                return None;
            }
            let path = draft.route.as_ref()?.path.as_deref()?;
            let (literal, kind) = mux_literal(path, record.prefix);
            Some((literal, kind, record.method.as_str()))
        };
        let mut everything = MethodTries::default();
        let mut by_router: HashMap<usize, MethodTries> = HashMap::new();
        let mut unreadable: HashMap<usize, usize> = HashMap::new();
        for &index in indexes {
            let router = mapping[records[index].router];
            match readable(index) {
                Some((literal, kind, method)) => {
                    everything.insert(method, literal, kind);
                    by_router
                        .entry(router)
                        .or_default()
                        .insert(method, literal, kind);
                }
                None => {
                    unreadable.entry(router).or_insert(index);
                }
            }
        }
        let mut earlier: HashMap<usize, MethodTries> = HashMap::new();
        for &index in indexes {
            let router = mapping[records[index].router];
            let others_unreadable =
                unreadable.len() - usize::from(unreadable.contains_key(&router));
            let own_unreadable = unreadable.get(&router).is_some_and(|first| *first < index);
            let Some((literal, kind, method)) = readable(index) else {
                kept[index] = false;
                continue;
            };
            let own = by_router
                .get(&router)
                .map_or(0, |tries| tries.overlaps(method, literal, kind));
            let cross = everything.overlaps(method, literal, kind) - own;
            let before = earlier
                .get(&router)
                .map_or(0, |tries| tries.overlaps(method, literal, kind));
            if others_unreadable > 0 || own_unreadable || cross > 0 || before > 0 {
                kept[index] = false;
            }
            earlier
                .entry(router)
                .or_default()
                .insert(method, literal, kind);
        }
    }
}

/// The elements of a chi pattern, as the tree reads them: the static text, a parameter, a
/// regular expression, and a catch-all. A pattern that chi does not accept gives none.
fn chi_tokens(pattern: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut rest = pattern;
    loop {
        let parameter = rest.find('{');
        let wildcard = rest.find('*');
        match (parameter, wildcard) {
            (None, None) => {
                if !rest.is_empty() {
                    tokens.push(format!("s{rest}"));
                }
                return Some(tokens);
            }
            (Some(start), other) if other.is_none_or(|wild| start < wild) => {
                if start > 0 {
                    tokens.push(format!("s{}", &rest[..start]));
                }
                let mut depth = 0_usize;
                let mut end = None;
                for (offset, character) in rest[start..].char_indices() {
                    match character {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(start + offset);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let end = end?;
                match rest[start + 1..end].split_once(':') {
                    None => tokens.push("p".to_string()),
                    Some((_, expression)) => {
                        let mut token = String::from("r");
                        if !expression.is_empty() && !expression.starts_with('^') {
                            token.push('^');
                        }
                        token.push_str(expression);
                        if !expression.is_empty() && !expression.ends_with('$') {
                            token.push('$');
                        }
                        tokens.push(token);
                    }
                }
                rest = &rest[end + 1..];
            }
            (Some(_), _) => return None,
            (None, Some(wild)) => {
                if wild + 1 != rest.len() {
                    return None;
                }
                if wild > 0 {
                    tokens.push(format!("s{}", &rest[..wild]));
                }
                tokens.push("c".to_string());
                return Some(tokens);
            }
        }
    }
}

/// chi tries the children of a node in the order static, regular expression, parameter,
/// and catch-all, and goes back when a child fails. A parameter or a catch-all can have
/// a sibling of higher order that takes the request, so its routes give no entry. Two
/// different regular expressions have an order that omen does not read. Each tree has one
/// walk over the elements of its patterns.
fn chi_precedence(records: &[Record], trees: &[Option<usize>], kept: &mut [bool]) {
    let entries: Vec<Option<(usize, Vec<String>)>> = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let tree = trees[index]?;
            if record.opaque || record.mount {
                return None;
            }
            Some((tree, chi_tokens(&record.path)?))
        })
        .collect();
    precedence(&entries, false, kept);
}

/// The path of an echo route up to its first wildcard, and if text follows the wildcard. The
/// router stores such a route on the node of the wildcard. A colon after a backslash is text.
fn echo_cut_at_star(path: &str) -> (&str, bool) {
    let mut previous = ' ';
    let mut in_parameter = false;
    for (index, character) in path.char_indices() {
        if in_parameter {
            in_parameter = character != '/';
        } else if character == ':' && previous != '\\' {
            in_parameter = true;
        } else if character == '*' {
            return (&path[..=index], index + 1 != path.len());
        }
        previous = character;
    }
    (path, false)
}

/// The elements of an echo path: the static text, a parameter, and a wildcard. echo reads
/// a colon after a backslash as text.
fn echo_tokens(path: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    let mut previous = ' ';
    let mut characters = path.chars().peekable();
    while let Some(character) = characters.next() {
        if character == ':' && previous != '\\' {
            if !text.is_empty() {
                tokens.push(format!("s{}", std::mem::take(&mut text)));
            }
            tokens.push("p".to_string());
            while characters.next_if(|next| *next != '/').is_some() {}
            previous = ' ';
        } else if character == '*' {
            if !text.is_empty() {
                tokens.push(format!("s{}", std::mem::take(&mut text)));
            }
            tokens.push("c".to_string());
            return tokens;
        } else {
            text.push(character);
            previous = character;
        }
    }
    if !text.is_empty() {
        tokens.push(format!("s{text}"));
    }
    tokens
}

/// echo tries the children of a node in the order static, parameter, and wildcard, and
/// goes back when a child fails. A parameter at the end of a path takes the rest of the
/// path. So a wildcard that has a parameter sibling gives no entry.
fn echo_precedence(
    records: &[Record],
    drafts: &[Option<(Draft, Duplicate)>],
    roots: &[Option<usize>],
    kept: &mut [bool],
) {
    let entries: Vec<Option<(usize, Vec<String>)>> = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let root = roots[index]?;
            if record.opaque {
                return None;
            }
            let (draft, _) = drafts[index].as_ref()?;
            let path = draft.route.as_ref()?.path.as_deref()?;
            Some((root, echo_tokens(path)))
        })
        .collect();
    precedence(&entries, true, kept);
}

/// One walk over the elements of the patterns of each tree. A pattern is a list of
/// elements. Each element of kind 0, 1, or 2 is a regular expression, a parameter, or a
/// catch-all.
fn precedence(entries: &[Option<(usize, Vec<String>)>], echo: bool, kept: &mut [bool]) {
    let mut edges: HashMap<(usize, String), usize> = HashMap::new();
    let mut counts: Vec<[usize; 3]> = Vec::new();
    let mut roots: HashMap<usize, usize> = HashMap::new();
    let mut walks: Vec<Vec<(usize, Option<usize>)>> = vec![Vec::new(); entries.len()];
    for (index, entry) in entries.iter().enumerate() {
        let Some((tree, tokens)) = entry else {
            continue;
        };
        let tree = *tree;
        let tokens = tokens.clone();
        let root = *roots.entry(tree).or_insert_with(|| {
            counts.push([0; 3]);
            counts.len() - 1
        });
        let mut node = root;
        for token in tokens {
            let kind = match token.as_bytes()[0] {
                b'r' => Some(0),
                b'p' => Some(1),
                b'c' => Some(2),
                _ => None,
            };
            let next = match edges.get(&(node, token.clone())) {
                Some(next) => *next,
                None => {
                    counts.push([0; 3]);
                    let next = counts.len() - 1;
                    edges.insert((node, token), next);
                    if let Some(kind) = kind {
                        counts[node][kind] += 1;
                    }
                    next
                }
            };
            walks[index].push((node, kind));
            node = next;
        }
    }
    for (index, walk) in walks.iter().enumerate() {
        for (node, kind) in walk {
            let siblings = counts[*node];
            let hidden = match kind {
                Some(2) if echo => siblings[1] > 0,
                _ if echo => false,
                Some(0) => siblings[0] > 1,
                Some(1) => siblings[0] > 0,
                Some(2) => siblings[0] > 0 || siblings[1] > 0,
                _ => false,
            };
            if hidden {
                kept[index] = false;
            }
        }
    }
}
