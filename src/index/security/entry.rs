//! Entry points: the places where a program receives input from outside.
//!
//! The table in this module names each framework by the syntax that declares a
//! handler: a decorator, an annotation, a registration call, a file convention,
//! or the name `main`. A guard is a decorator, an annotation, a middleware name, or a
//! declared dependency that the syntax attaches to the handler. A guard is a declared fact. It
//! never means that the handler is authenticated.
//!
//! The public API is [`find_entry_points`]. A different analyzer can call it
//! with the parsed files and the index of the same files.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::spec::syntax_of;
use super::syntax;
use crate::core::Language;
use crate::index::extract::FileSymbols;
use crate::index::model::{ResolutionBasis, ResolutionStatus, Span, Symbol, SymbolKind};
use crate::parser::{ParseResult, Walk};

mod ecma;
mod fastapi;
mod go_routes;
mod go_wrappers;
mod persistent;
mod rails;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    HttpHandler,
    CliMain,
    MessageConsumer,
    ScheduledJob,
    ServerlessHandler,
    /// A function that a foreign program calls, as a Rust `#[no_mangle]` function.
    FfiExport,
}

impl EntryKind {
    pub const ALL: [Self; 6] = [
        Self::HttpHandler,
        Self::CliMain,
        Self::MessageConsumer,
        Self::ScheduledJob,
        Self::ServerlessHandler,
        Self::FfiExport,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::HttpHandler => "http_handler",
            Self::CliMain => "cli_main",
            Self::MessageConsumer => "message_consumer",
            Self::ScheduledJob => "scheduled_job",
            Self::ServerlessHandler => "serverless_handler",
            Self::FfiExport => "ffi_export",
        }
    }
}

/// How the syntax declares the entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryBasis {
    /// A decorator, an annotation, or an attribute on the handler.
    Declared,
    /// A call registers the handler.
    Registered,
    /// A file location or a name gives the handler, as `main` does.
    Convention,
}

impl EntryBasis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Registered => "registered",
            Self::Convention => "convention",
        }
    }
}

/// Why the guard list of an entry point is not complete: code that omen did not read can add a
/// guard to the entry point, or remove one. The order of the variants is the order in which omen
/// reports one reason for an entry point that has more than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardsUnknownReason {
    /// A loop around the route has more work than omen expands. Omen read the block of the loop
    /// one time, and the values of the loop parameters are not known.
    UnexpandedLoop,
    /// Omen did not find the controller or the action of the route.
    UnresolvedController,
    /// The method of the action is not public, or omen cannot read its visibility.
    UnreadableAction,
    /// The handler of the route is an application that is not a controller of the repository.
    RackEndpoint,
    /// A class of the chain has a superclass that omen cannot find in the repository.
    UnresolvedSuperclass,
    /// A class of the chain includes, extends, or prepends a module. A module can add callbacks.
    IncludedModule,
    /// Code outside the body of a class of the chain can change the callbacks of the class.
    CallbackOutsideClass,
    /// A class of the chain calls a class method that omen cannot resolve to a definition
    /// that it can read.
    UnresolvedMacro,
    /// A callback call has a form or a place that omen cannot read.
    UnreadableCallback,
    /// A callback has a condition, so omen does not know if it runs.
    ConditionalCallback,
    /// A class of the chain has an `around_action` callback. Omen reads only `before_action`.
    AroundCallback,
    /// The router of the route comes from an import.
    ImportedRouter,
    /// Omen cannot find the one router that the route belongs to.
    UnresolvedRouter,
    /// The file does not include the router in an application. Another file can include it
    /// with more dependencies.
    IncludedElsewhere,
    /// The file includes the router two times, in a condition, or in a loop of routers.
    UnreadableInclude,
    /// A dependency is not a name that omen can read, or the `dependencies` value is not a
    /// literal list, or the file uses the `dependencies` attribute of an object.
    UnreadableDependency,
    /// The file has a wildcard import, which can bind a name again.
    WildcardImport,
    /// A middleware layer of a router is added in a condition, a loop, or a function literal, so
    /// omen does not know if it runs.
    ConditionalMiddleware,
    /// A middleware argument is not a name that omen can read, or a call around a Go handler is
    /// not a conversion and not a middleware that omen reads.
    UnreadMiddleware,
    /// The framework has a place for guards that omen does not read, such as the middleware of
    /// the application.
    LayerNotRead,
}

impl GuardsUnknownReason {
    pub fn name(self) -> &'static str {
        match self {
            Self::UnexpandedLoop => "unexpanded_loop",
            Self::UnresolvedController => "unresolved_controller",
            Self::UnreadableAction => "unreadable_action",
            Self::RackEndpoint => "rack_endpoint",
            Self::UnresolvedSuperclass => "unresolved_superclass",
            Self::IncludedModule => "included_module",
            Self::CallbackOutsideClass => "callback_outside_class",
            Self::UnresolvedMacro => "unresolved_macro",
            Self::UnreadableCallback => "unreadable_callback",
            Self::ConditionalCallback => "conditional_callback",
            Self::AroundCallback => "around_callback",
            Self::ImportedRouter => "imported_router",
            Self::UnresolvedRouter => "unresolved_router",
            Self::IncludedElsewhere => "included_elsewhere",
            Self::UnreadableInclude => "unreadable_include",
            Self::UnreadableDependency => "unreadable_dependency",
            Self::WildcardImport => "wildcard_import",
            Self::ConditionalMiddleware => "conditional_middleware",
            Self::UnreadMiddleware => "unread_middleware",
            Self::LayerNotRead => "layer_not_read",
        }
    }
}

/// The first of two reasons in the order of [`GuardsUnknownReason`].
fn first_reason(
    left: Option<GuardsUnknownReason>,
    right: Option<GuardsUnknownReason>,
) -> Option<GuardsUnknownReason> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

/// The HTTP frameworks for which omen reads only the guards that the route itself declares. Each
/// has a place for guards that omen does not read: `app.use` of Express, `before_request` of
/// Flask, the middleware and the class-based views of Django, the security configuration of
/// Spring, the filters and the policies of ASP.NET, the layers and the extractors of axum, the
/// wrappers and the guards of actix, the fairings and the request guards of rocket, the route
/// groups of Laravel, and the middleware file of Next.
const FRAMEWORKS_WITH_UNREAD_LAYERS: [&str; 16] = [
    "express",
    "fastify",
    "koa",
    "nestjs",
    "routing-controllers",
    "tsoa",
    "repository_decorators",
    "flask",
    "django",
    "spring",
    "aspnet",
    "axum",
    "actix",
    "rocket",
    "laravel",
    "next",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// The method that the framework registers, or `ANY`. gin and echo keep a custom method as the source writes it.
    pub method: String,
    /// The path as the source writes it, with a leading `/`. `None` when omen cannot resolve a
    /// part of the path, as a variable that is not a constant.
    pub path: Option<String>,
    /// The path with each part that omen cannot resolve as `${expression}`, when `path` is
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_template: Option<String>,
    /// The names of the parameters in the path, in order. For FastAPI, the names of the request
    /// parameters of the handler follow them.
    pub parameters: Vec<String>,
}

impl Route {
    /// The path, or the template when omen cannot resolve the path.
    pub fn path_text(&self) -> &str {
        self.path
            .as_deref()
            .or(self.path_template.as_deref())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPoint {
    /// `file:line:column:kind`.
    pub key: String,
    pub file: String,
    pub kind: EntryKind,
    /// The range of the declaration, the registration call, or the file.
    pub range: Span,
    /// The key of the handler symbol, when the index has it. For an inline handler, the key
    /// names the function value: see `inline_handler`.
    pub symbol: Option<String>,
    pub framework: String,
    pub route: Option<Route>,
    /// The decorators, annotations, middleware names, and declared dependencies that the syntax
    /// attaches to the handler.
    pub guards: Vec<String>,
    /// `true` when omen read each place that can give the entry point a guard, so `guards` is
    /// the full list. `false` when code that omen did not read can add a guard or remove one.
    /// Then an empty `guards` list does not tell that the entry point has no guard.
    #[serde(default)]
    pub guards_complete: bool,
    /// Why `guards` is not complete. It is `None` when `guards_complete` is `true`.
    #[serde(default)]
    pub guards_unknown_reason: Option<GuardsUnknownReason>,
    pub basis: EntryBasis,
    pub in_test: bool,
    /// The function value without a name that is the handler, as an arrow function. The index
    /// has no symbol for it, so `symbol` is a key that only the security analysis knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_handler: Option<InlineHandler>,
}

/// A handler that is a function value without a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineHandler {
    /// The range of the function value.
    pub range: Span,
    /// The key of the nearest symbol that contains the function value. The index gives each call
    /// in the function value to this symbol.
    pub enclosing: Option<String>,
}

impl EntryPoint {
    /// The guards as one text for a table: the names, and a note when the list is not
    /// complete.
    pub fn guards_text(&self) -> String {
        let names = self.guards.join(", ");
        match (self.guards_unknown_reason, names.is_empty()) {
            (None, true) if self.guards_complete => String::from("none declared"),
            (None, _) if self.guards_complete => names,
            (reason, empty) => {
                let reason = reason.map_or("not known", GuardsUnknownReason::name);
                if empty {
                    format!("unknown ({reason})")
                } else {
                    format!("{names} (incomplete: {reason})")
                }
            }
        }
    }
}

/// The entry points of the files, in file order and then in source order.
///
/// `parsed[i]` is the syntax tree of `files[i]`. Give the files after
/// `resolve_imports`.
pub fn find_entry_points(parsed: &[ParseResult], files: &[FileSymbols]) -> Vec<EntryPoint> {
    find_entry_points_with(parsed, files, &[])
}

/// An entry point rule of the user: each function with one of the names is an entry
/// point of the kind and the framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRule {
    pub kind: EntryKind,
    pub framework: &'static str,
    pub languages: Vec<Language>,
    pub symbols: Vec<String>,
}

/// A syntax tree that a [`TreeSource`] gives.
pub enum TreeRef<'a> {
    Borrowed(&'a ParseResult),
    Shared(std::sync::Arc<ParseResult>),
}

impl std::ops::Deref for TreeRef<'_> {
    type Target = ParseResult;

    fn deref(&self) -> &ParseResult {
        match self {
            Self::Borrowed(tree) => tree,
            Self::Shared(tree) => tree,
        }
    }
}

/// The syntax trees of the files of one analysis, by position.
///
/// A source can parse a file when a caller asks. Thus the analysis does not need
/// the trees of all files at one time.
pub trait TreeSource: Sync {
    fn tree(&self, position: usize) -> Option<TreeRef<'_>>;

    /// The tree of a file for a walk that reads it one time. The source does not keep
    /// the tree for a later request.
    fn transient_tree(&self, position: usize) -> Option<TreeRef<'_>> {
        self.tree(position)
    }

    /// The data that the entry point search shares between the files of one analysis.
    fn shared(&self) -> &EntryShared;
}

/// Tells if a symbol is a function, a method, or a constructor.
fn is_callable_kind(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

/// The position of each function, method, and constructor of the files, by name.
fn callables_by_name(
    files: &[FileSymbols],
) -> std::collections::HashMap<String, Vec<(usize, usize)>> {
    let mut names: std::collections::HashMap<String, Vec<(usize, usize)>> =
        std::collections::HashMap::new();
    for (file, symbols) in files.iter().enumerate() {
        for (position, symbol) in symbols.symbols.iter().enumerate() {
            if is_callable_kind(symbol) {
                names
                    .entry(symbol.name.clone())
                    .or_default()
                    .push((file, position));
            }
        }
    }
    names
}

/// The data that one analysis builds one time, and that the search for entry points of each file
/// uses.
#[derive(Default)]
pub struct EntryShared {
    rails: std::sync::OnceLock<rails::RailsShared>,
    callables: std::sync::OnceLock<std::collections::HashMap<String, Vec<(usize, usize)>>>,
    go_callables: std::sync::OnceLock<go_routes::PackageCallables>,
    go_packages: std::sync::OnceLock<go_routes::GoPackages>,
    ecma: ecma::Shared,
}

/// The syntax trees that the entry point rules ask for in other files. The source
/// parses a file when a rule asks, and keeps the tree for the later requests. A walk
/// that reads each file of a language one time asks for a transient tree. The source
/// keeps that tree only while the walk uses it: it would otherwise keep a tree of each
/// file until the end of the analysis.
pub struct OnDemandTrees<'a> {
    parse_file: &'a (dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    error: parking_lot::Mutex<Option<crate::core::Error>>,
    parsed: parking_lot::Mutex<std::collections::HashMap<usize, Cell>>,
    kept: parking_lot::Mutex<std::collections::HashMap<usize, std::sync::Arc<ParseResult>>>,
    shared: EntryShared,
    /// A test runs this before a thread reads the parse that another thread
    /// stored, and thus orders that read against the other threads.
    #[cfg(test)]
    before_read_of_stored_parse: Option<&'a (dyn Fn() + Sync)>,
}

/// The parse of a file. The first thread that asks parses the file, and the other
/// threads wait for the result. A file that cannot be parsed stays without a tree.
type Cell = std::sync::Arc<std::sync::OnceLock<Option<std::sync::Weak<ParseResult>>>>;

impl<'a> OnDemandTrees<'a> {
    pub fn new(
        parse_file: &'a (dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    ) -> Self {
        Self {
            parse_file,
            error: parking_lot::Mutex::new(None),
            parsed: parking_lot::Mutex::new(std::collections::HashMap::new()),
            kept: parking_lot::Mutex::new(std::collections::HashMap::new()),
            shared: EntryShared::default(),
            #[cfg(test)]
            before_read_of_stored_parse: None,
        }
    }

    /// The error of a parse that a rule asked for, when a parse failed. The caller must
    /// call this after the analysis, and it must give the error to its own caller.
    pub fn finish(&self) -> crate::core::Result<()> {
        match self.error.lock().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl TreeSource for OnDemandTrees<'_> {
    fn tree(&self, position: usize) -> Option<TreeRef<'_>> {
        let tree = self.transient_tree(position)?;
        if let TreeRef::Shared(shared) = &tree {
            self.kept
                .lock()
                .entry(position)
                .or_insert_with(|| std::sync::Arc::clone(shared));
        }
        Some(tree)
    }

    fn transient_tree(&self, position: usize) -> Option<TreeRef<'_>> {
        let parse = || match (self.parse_file)(position) {
            Ok(tree) => tree.map(std::sync::Arc::new),
            Err(error) => {
                self.error.lock().get_or_insert(error);
                None
            }
        };
        // The thread that parsed the tree can release it before a waiting thread reads
        // it. Then the waiting thread asks again, and its parse is in the cache for the
        // later requests.
        let tree = loop {
            let cell = {
                let mut parsed = self.parsed.lock();
                let cell = parsed.entry(position).or_default();
                let released = matches!(cell.get(), Some(Some(tree)) if tree.strong_count() == 0);
                if released {
                    *cell = Cell::default();
                }
                std::sync::Arc::clone(cell)
            };
            let mut parsed_here = None;
            let stored = cell.get_or_init(|| {
                let tree = parse()?;
                let weak = std::sync::Arc::downgrade(&tree);
                parsed_here = Some(tree);
                Some(weak)
            });
            match (parsed_here, stored) {
                (Some(tree), _) => break tree,
                (None, None) => return None,
                (None, Some(weak)) => {
                    #[cfg(test)]
                    if let Some(hook) = self.before_read_of_stored_parse {
                        hook();
                    }
                    if let Some(tree) = weak.upgrade() {
                        break tree;
                    }
                }
            }
        };
        Some(TreeRef::Shared(tree))
    }

    fn shared(&self) -> &EntryShared {
        &self.shared
    }
}

struct Trees<'a>(&'a [ParseResult], EntryShared);

impl TreeSource for Trees<'_> {
    fn tree(&self, position: usize) -> Option<TreeRef<'_>> {
        self.0.get(position).map(TreeRef::Borrowed)
    }

    fn shared(&self) -> &EntryShared {
        &self.1
    }
}

/// Like [`find_entry_points`], with the rules of the user.
pub fn find_entry_points_with(
    parsed: &[ParseResult],
    files: &[FileSymbols],
    rules: &[EntryRule],
) -> Vec<EntryPoint> {
    assert_eq!(parsed.len(), files.len(), "each file needs its syntax tree");
    let trees = Trees(parsed, EntryShared::default());
    let per_file: Vec<Vec<EntryPoint>> = (0..files.len())
        .into_par_iter()
        .map(|position| entry_points_of_file(position, &parsed[position], files, &trees, rules))
        .collect();
    per_file.into_iter().flatten().collect()
}

/// The entry points of `files[position]`, in source order. `own` is the tree of that
/// file. `trees` gives the trees of the other files when a rule needs them.
pub fn entry_points_of_file(
    position: usize,
    own: &ParseResult,
    files: &[FileSymbols],
    trees: &dyn TreeSource,
    rules: &[EntryRule],
) -> Vec<EntryPoint> {
    let context = Context::new(position, own, &files[position], files, trees, rules);
    let mut found = context.detect();
    found.sort_by(|left, right| {
        (left.range.start_byte, &left.key).cmp(&(right.range.start_byte, &right.key))
    });
    found.dedup_by(|later, earlier| later.key == earlier.key);
    found
}

const HTTP_VERBS: [&str; 8] = [
    "get", "post", "put", "patch", "delete", "head", "options", "all",
];
const PYTHON_GUARDS: [&str; 9] = [
    "login_required",
    "permission_required",
    "user_passes_test",
    "jwt_required",
    "token_required",
    "auth_required",
    "requires_auth",
    "roles_required",
    "roles_accepted",
];
const JAVA_GUARDS: [&str; 6] = [
    "PreAuthorize",
    "PostAuthorize",
    "Secured",
    "RolesAllowed",
    "PermitAll",
    "DenyAll",
];
const CSHARP_GUARDS: [&str; 3] = ["Authorize", "AllowAnonymous", "RequireAuthorization"];
const JAVA_CONSUMERS: [&str; 5] = [
    "KafkaListener",
    "RabbitListener",
    "JmsListener",
    "SqsListener",
    "StreamListener",
];

/// The names of the parameters in a path, in order: `<int:id>`, `{id}`, `{id:int}`, `:id`.
pub fn route_parameters(path: &str) -> Vec<String> {
    let clean = |text: &str, separator: &[char]| -> String {
        let name = if separator == ['>'] {
            text.rsplit(':').next().unwrap_or(text)
        } else {
            text.split([':', '?', '=', '*']).next().unwrap_or(text)
        };
        name.trim().to_string()
    };
    let mut names = Vec::new();
    let characters: Vec<char> = path.chars().collect();
    let mut position = 0;
    while position < characters.len() {
        let (close, separator): (Option<char>, &[char]) = match characters[position] {
            '<' => (Some('>'), &['>']),
            '{' => (Some('}'), &['}']),
            ':' if position == 0 || characters[position - 1] == '/' => (None, &[]),
            _ => {
                position += 1;
                continue;
            }
        };
        let start = position + 1;
        let end = match close {
            Some(mark) => characters[start..]
                .iter()
                .position(|character| *character == mark)
                .map_or(characters.len(), |offset| start + offset),
            None => characters[start..]
                .iter()
                .position(|character| !(character.is_alphanumeric() || *character == '_'))
                .map_or(characters.len(), |offset| start + offset),
        };
        let text: String = characters[start..end].iter().collect();
        let name = if close.is_some() {
            clean(&text, separator)
        } else {
            text
        };
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            names.push(name);
        }
        position = end + 1;
    }
    names
}

/// Joins a prefix and a path with one `/`, and gives the result a leading `/`.
fn join_path(prefix: &str, path: &str) -> String {
    let mut joined = String::from("/");
    for part in [prefix, path] {
        let part = part.trim_matches('/');
        if part.is_empty() {
            continue;
        }
        if !joined.ends_with('/') {
            joined.push('/');
        }
        joined.push_str(part);
    }
    if path.ends_with('/') && !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

/// The name node of a value that names a function: the name itself, or the last name of a
/// path or a member. Rust `list::<T>` gives the name `list`.
fn value_name(value: Node<'_>) -> Option<Node<'_>> {
    match value.kind() {
        "identifier" => Some(value),
        "generic_function" => value_name(value.child_by_field_name("function")?),
        "scoped_identifier" | "member_access_expression" => value.child_by_field_name("name"),
        "attribute" => value.child_by_field_name("attribute"),
        "member_expression" => value.child_by_field_name("property"),
        "selector_expression" | "field_expression" => value.child_by_field_name("field"),
        _ => None,
    }
}

fn route(method: &str, path: String) -> Route {
    Route {
        method: method.to_uppercase(),
        parameters: route_parameters(&path),
        path: Some(path),
        path_template: None,
    }
}

/// Removes each value that is equal to an earlier value.
fn keep_first(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

/// The name of a Python decorator that is a known guard, as `login_required`.
fn python_guard(decorator: Node<'_>, source: &[u8]) -> Option<String> {
    let expression = decorator.named_child(0)?;
    let callee = if expression.kind() == "call" {
        expression.child_by_field_name("function")?
    } else {
        expression
    };
    let name = syntax::text(callee, source).rsplit('.').next()?;
    PYTHON_GUARDS.contains(&name).then(|| name.to_string())
}

/// The known guards in the decorators of the Python function with the range.
fn python_function_guards(root: Node<'_>, source: &[u8], range: &Span) -> Vec<String> {
    let decorated = root
        .descendant_for_byte_range(range.start_byte, range.end_byte)
        .and_then(|function| function.parent())
        .filter(|parent| parent.kind() == "decorated_definition");
    let Some(decorated) = decorated else {
        return Vec::new();
    };
    let mut cursor = decorated.walk();
    let decorators: Vec<Node<'_>> = decorated
        .children(&mut cursor)
        .filter(|child| child.kind() == "decorator")
        .collect();
    decorators
        .into_iter()
        .filter_map(|decorator| python_guard(decorator, source))
        .collect()
}

/// Tells if a declaration or a typed parameter binds `name` to a text with a marker.
fn bound_to_marker(node: Node<'_>, source: &[u8], name: &str, markers: &[&str]) -> bool {
    crate::parser::find_preorder(node, |node| binds_to_marker(node, source, name, markers))
        .is_some()
}

/// Tells if `node` itself binds `name` to a text with one of the markers.
fn binds_to_marker(node: Node<'_>, source: &[u8], name: &str, markers: &[&str]) -> bool {
    const BINDINGS: [(&str, &[&str], &[&str]); 5] = [
        ("variable_declarator", &["name"], &["value"]),
        ("assignment", &["left"], &["right"]),
        ("assignment_expression", &["left"], &["right"]),
        ("short_var_declaration", &["left"], &["right"]),
        ("let_declaration", &["pattern"], &["value", "type"]),
    ];
    const TYPED: [&str; 7] = [
        "parameter",
        "formal_parameter",
        "parameter_declaration",
        "typed_parameter",
        "typed_default_parameter",
        "required_parameter",
        "optional_parameter",
    ];
    let kind = node.kind();
    let sides: Option<(&[&str], &[&str])> = BINDINGS
        .iter()
        .find(|(binding, _, _)| *binding == kind)
        .map(|(_, left, right)| (*left, *right))
        .or_else(|| {
            TYPED
                .contains(&kind)
                .then_some((&["name", "pattern"][..], &["type"][..]))
        })
        .or_else(|| (kind == "var_spec").then_some((&["name"][..], &["value", "type"][..])));
    if let Some((left_fields, right_fields)) = sides {
        let left = left_fields
            .iter()
            .find_map(|field| node.child_by_field_name(field))
            .or_else(|| {
                if kind == "typed_parameter" {
                    node.named_child(0)
                } else {
                    None
                }
            });
        let right = right_fields
            .iter()
            .find_map(|field| node.child_by_field_name(field));
        if let (Some(left), Some(right)) = (left, right) {
            let names_it = syntax::text(left, source).split(',').any(|part| {
                part.trim()
                    .trim_start_matches('&')
                    .trim_start_matches("mut ")
                    .trim()
                    == name
            });
            if names_it
                && markers
                    .iter()
                    .any(|marker| syntax::text(right, source).contains(marker))
            {
                return true;
            }
        }
    }
    false
}

/// The local bindings of a Go file, by name.
type GoLocals = std::collections::HashMap<String, (Vec<usize>, Vec<usize>)>;

struct Context<'a> {
    parsed: &'a ParseResult,
    /// The position of the file in `all`.
    position: usize,
    file: &'a FileSymbols,
    all: &'a [FileSymbols],
    all_parsed: &'a dyn TreeSource,
    rules: &'a [EntryRule],
    source: &'a [u8],
    /// The start of the first statement of a list that jumps out of the block (`next`, `break`), by
    /// the id of the list node.
    rails_jumps: std::cell::RefCell<std::collections::HashMap<usize, Option<usize>>>,
    /// The ids of the children of a `case` that can run, by the id of the `case` node.
    rails_cases:
        std::cell::RefCell<std::collections::HashMap<usize, Option<std::rc::Rc<Vec<usize>>>>>,
    /// The result for a chain of modules, a controller, and an action, by the Rails reader.
    rails_handled: std::cell::RefCell<std::collections::HashMap<rails::HandlerKey, rails::Handler>>,
    /// Tells for a chain of modules and a controller if the repository has the class.
    rails_known: std::cell::RefCell<std::collections::HashMap<(usize, rails::ControllerKey), bool>>,
    /// The controller that a chain of modules names, by the id of the chain.
    rails_module_controllers:
        std::cell::RefCell<std::collections::HashMap<usize, rails::ScopedController>>,
    /// The names that a Rails routes file assigns a value to.
    rails_assigned: std::sync::OnceLock<std::collections::HashSet<String>>,
    /// The work that the expansion of Rails loops can still add to the walks of the file.
    rails_loop_work: std::cell::Cell<usize>,
    /// The position of the first symbol of the file for each start of a name.
    symbol_starts: std::sync::OnceLock<std::collections::HashMap<usize, usize>>,
    /// The positions of the functions, the methods, and the constructors of the file, by name.
    callables_here: std::sync::OnceLock<std::collections::HashMap<String, Vec<usize>>>,
    /// The local bindings of a Go file, by name: the starts of the bindings in order, and
    /// for each the highest end of a function that has a binding up to it.
    go_locals: std::sync::OnceLock<GoLocals>,
    /// The names of the variables that hold a `net/http` multiplexer, in a Go file.
    go_servemuxes: std::sync::OnceLock<std::collections::HashSet<String>>,
    /// The `net/http` patterns of a Go file that other patterns of the file can cover.
    go_covered: std::sync::OnceLock<std::collections::HashSet<String>>,
    /// The start and the highest end of the branches of a Go file that never run, by start.
    go_dead: std::sync::OnceLock<(Vec<usize>, Vec<usize>)>,
    /// The names that the package of a Go file declares at the top level.
    go_declared: std::sync::OnceLock<std::sync::Arc<std::collections::HashSet<String>>>,
    /// The first import of a module of a Go file for each local name.
    go_module_imports: std::sync::OnceLock<
        std::collections::HashMap<&'a str, &'a crate::index::model::ImportBinding>,
    >,
    /// The bindings of a Python file, by scope. Only a file with a FastAPI route builds them.
    python_scopes: std::cell::OnceCell<fastapi::Scopes<'a>>,
    /// Why the guards of the FastAPI route that omen reads now are not complete.
    guard_gap: std::cell::Cell<Option<GuardsUnknownReason>>,
    /// Tells for the text of the receiver of a Python route decorator if it is a framework value.
    python_receivers: std::cell::RefCell<std::collections::HashMap<&'a str, bool>>,
    ecma: ecma::State,
}

struct Draft {
    range: Span,
    kind: EntryKind,
    symbol: Option<String>,
    framework: &'static str,
    route: Option<Route>,
    guards: Vec<String>,
    basis: EntryBasis,
    /// Tells apart the entry points that one call registers, as the action of a Rails `resources` call.
    variant: Option<String>,
    inline_handler: Option<InlineHandler>,
    /// Why the reader of the framework cannot give the full list of the guards.
    unknown: Option<GuardsUnknownReason>,
}

impl<'a> Context<'a> {
    fn new(
        position: usize,
        parsed: &'a ParseResult,
        file: &'a FileSymbols,
        all: &'a [FileSymbols],
        all_parsed: &'a dyn TreeSource,
        rules: &'a [EntryRule],
    ) -> Self {
        Self {
            position,
            parsed,
            file,
            all,
            all_parsed,
            rules,
            source: &parsed.source,
            rails_jumps: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_cases: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_handled: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_known: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_module_controllers: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_assigned: std::sync::OnceLock::new(),
            rails_loop_work: std::cell::Cell::new(rails::LOOP_WORK_LIMIT),
            symbol_starts: std::sync::OnceLock::new(),
            callables_here: std::sync::OnceLock::new(),
            go_locals: std::sync::OnceLock::new(),
            go_servemuxes: std::sync::OnceLock::new(),
            go_covered: std::sync::OnceLock::new(),
            go_dead: std::sync::OnceLock::new(),
            go_declared: std::sync::OnceLock::new(),
            go_module_imports: std::sync::OnceLock::new(),
            python_scopes: std::cell::OnceCell::new(),
            guard_gap: std::cell::Cell::new(None),
            python_receivers: std::cell::RefCell::new(std::collections::HashMap::new()),
            ecma: ecma::State::default(),
        }
    }

    fn is_string(&self, node: Node<'_>) -> bool {
        syntax_of(self.file.language)
            .string_kinds
            .contains(&node.kind())
    }

    fn text(&self, node: Node<'_>) -> &'a str {
        syntax::text(node, self.source)
    }

    fn string_of(&self, node: Node<'_>) -> String {
        syntax::unquote(self.text(node)).to_string()
    }

    /// The first string literal in the node, or in its descendants.
    fn first_string(&self, node: Node<'_>) -> Option<String> {
        let mut found = None;
        crate::parser::walk_preorder(node, |descendant| {
            if descendant != node && !descendant.is_named() {
                Walk::SkipChildren
            } else if self.is_string(descendant) {
                found = Some(self.string_of(descendant));
                Walk::Stop
            } else {
                Walk::Children
            }
        });
        found
    }

    fn imports(&self, module: &str) -> bool {
        self.file.imports.iter().any(|binding| {
            let path = binding.module_path.replace(['/', '\\', ':'], ".");
            path == module || path.starts_with(&format!("{module}."))
        })
    }

    fn mentions(&self, word: &str) -> bool {
        self.imports(word) || self.text(self.parsed.root_node()).contains(word)
    }

    /// Tells if the receiver of a registration is a framework object. A receiver is a
    /// framework object when its text has a marker, as `express()` has, when a
    /// declaration or a typed parameter in the file binds its name to a text with a marker,
    /// or when an import binds its name and the import is of one of the `exports` of the
    /// framework, a module and the names that make a router or an application, or the
    /// module is a file of the repository that binds the name to a text with a marker.
    /// Other exports of the framework, such as `request`, are not framework values.
    fn is_framework_value(
        &self,
        receiver: Node<'_>,
        markers: &[&str],
        exports: &[(&str, &[&str])],
    ) -> bool {
        let text = self.text(receiver);
        if markers.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        if !matches!(receiver.kind(), "identifier" | "type_identifier") {
            return false;
        }
        let imported = self
            .file
            .imports
            .iter()
            .filter(|binding| binding.local_name.as_deref() == Some(text))
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return imported
                .into_iter()
                .any(|binding| self.import_is_framework_value(binding, markers, exports));
        }
        bound_to_marker(self.parsed.root_node(), self.source, text, markers)
    }

    fn import_is_framework_value(
        &self,
        binding: &crate::index::model::ImportBinding,
        markers: &[&str],
        exports: &[(&str, &[&str])],
    ) -> bool {
        use crate::index::model::ImportTarget;
        let path = binding.module_path.replace(['/', '\\', ':'], ".");
        let is_framework_module = exports
            .iter()
            .any(|(module, _)| path == *module || path.starts_with(&format!("{module}.")));
        if is_framework_module {
            return binding
                .imported_name
                .as_deref()
                .is_some_and(|name| exports.iter().any(|(_, names)| names.contains(&name)));
        }
        let targets: &[String] = match &binding.target_file {
            ImportTarget::Resolved(file) => std::slice::from_ref(file),
            ImportTarget::Package(files) => files,
            _ => return false,
        };
        let name = match binding.imported_name.as_deref() {
            Some("*") | Some("default") | None => binding.local_name.as_deref().unwrap_or(""),
            Some(name) => name,
        };
        targets.iter().any(|target| {
            self.all
                .iter()
                .position(|file| &file.file == target)
                .and_then(|position| self.all_parsed.tree(position))
                .is_some_and(|parsed| {
                    bound_to_marker(parsed.root_node(), &parsed.source, name, markers)
                })
        })
    }

    /// The position of the first symbol of the file for each start of a name. A reader that
    /// does not look up a symbol does not build the table.
    fn symbol_starts(&self) -> &std::collections::HashMap<usize, usize> {
        self.symbol_starts.get_or_init(|| {
            let mut starts = std::collections::HashMap::new();
            for (position, symbol) in self.file.symbols.iter().enumerate() {
                starts
                    .entry(symbol.name_range.start_byte)
                    .or_insert(position);
            }
            starts
        })
    }

    /// The positions of the functions, the methods, and the constructors of the file, by name.
    fn callables_here(&self) -> &std::collections::HashMap<String, Vec<usize>> {
        self.callables_here.get_or_init(|| {
            let mut names: std::collections::HashMap<String, Vec<usize>> =
                std::collections::HashMap::new();
            for (position, symbol) in self.file.symbols.iter().enumerate() {
                if is_callable_kind(symbol) {
                    names.entry(symbol.name.clone()).or_default().push(position);
                }
            }
            names
        })
    }

    fn symbol_at(&self, name_node: Node<'_>) -> Option<String> {
        let position = self.symbol_starts().get(&name_node.start_byte())?;
        Some(self.file.symbols[*position].key.to_string())
    }

    /// The one function or method with the name. A definition in the file wins over a definition in another file.
    fn symbol_named(&self, name: &str, container: Option<&str>) -> Option<String> {
        let (file, position) = self.callable_named(name, container)?;
        let file = file.map_or(self.file, |file| &self.all[file]);
        Some(file.symbols[position].key.to_string())
    }

    /// The file and the position of the one function or method with the name. The file `None`
    /// is this file. A definition in this file wins over a definition in another file.
    fn callable_named(
        &self,
        name: &str,
        container: Option<&str>,
    ) -> Option<(Option<usize>, usize)> {
        let fits = |symbol: &Symbol| {
            container.is_none_or(|wanted| symbol.container.as_deref() == Some(wanted))
        };
        let here: Vec<usize> = self
            .callables_here()
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|position| fits(&self.file.symbols[*position]))
            .collect();
        if here.len() == 1 {
            return Some((None, here[0]));
        }
        let everywhere: Vec<(usize, usize)> = self
            .all_parsed
            .shared()
            .callables
            .get_or_init(|| callables_by_name(self.all))
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|(file, position)| fits(&self.all[*file].symbols[*position]))
            .collect();
        (everywhere.len() == 1).then(|| (Some(everywhere[0].0), everywhere[0].1))
    }

    fn handler_symbol(&self, handler: Node<'_>) -> Option<String> {
        if self.file.language == Language::Go {
            return self.go_handler_symbol(handler);
        }
        if let Some(symbol) = self.referenced_function(handler) {
            return Some(symbol);
        }
        let name = match handler.kind() {
            "identifier" | "shorthand_property_identifier" => self.text(handler),
            "attribute" | "member_expression" | "selector_expression" | "field_expression" => {
                self.text(handler).rsplit(['.', ':']).next().unwrap_or("")
            }
            // A path names a module or a type. Its last name alone is no evidence.
            _ => return None,
        };
        self.symbol_named(name, None)
    }

    /// The one function that the index resolves for the use of the handler as a value. A
    /// resolution by the name alone does not count.
    fn referenced_function(&self, handler: Node<'_>) -> Option<String> {
        let name = value_name(handler)?;
        let references = &self.file.function_references;
        let first = references
            .partition_point(|reference| reference.name_range.start_byte < name.start_byte());
        let mut found = references[first..]
            .iter()
            .take_while(|reference| reference.name_range.start_byte == name.start_byte())
            .filter(|reference| {
                matches!(
                    reference.resolution.status,
                    ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
                ) && reference.resolution.basis != Some(ResolutionBasis::NameOnly)
            })
            .flat_map(|reference| reference.resolution.candidates.iter());
        let symbol = found.next()?;
        found
            .all(|other| other == symbol)
            .then(|| symbol.to_string())
    }

    fn arguments_of(call: Node<'_>) -> Vec<Node<'_>> {
        let Some(list) = call.child_by_field_name("arguments") else {
            return Vec::new();
        };
        let mut cursor = list.walk();
        let nodes: Vec<Node<'_>> = list.named_children(&mut cursor).collect();
        nodes
            .into_iter()
            .filter(|node| !node.is_extra())
            .map(|node| {
                if node.kind() == "argument" {
                    node.named_child(0).unwrap_or(node)
                } else {
                    node
                }
            })
            .collect()
    }

    fn detect(&self) -> Vec<EntryPoint> {
        let mut drafts = Vec::new();
        self.walk(self.parsed.root_node(), &mut drafts);
        self.convention_entries(&mut drafts);
        let mut entries: Vec<EntryPoint> = drafts
            .into_iter()
            .map(|draft| {
                let method = draft
                    .route
                    .as_ref()
                    .map(|route| format!(":{}", route.method))
                    .unwrap_or_default()
                    + &draft
                        .variant
                        .as_ref()
                        .map(|variant| format!("#{variant}"))
                        .unwrap_or_default();
                let layer = (draft.kind == EntryKind::HttpHandler
                    && FRAMEWORKS_WITH_UNREAD_LAYERS.contains(&draft.framework))
                .then_some(GuardsUnknownReason::LayerNotRead);
                let placeholder = draft
                    .guards
                    .iter()
                    .any(|guard| {
                        guard == go_routes::UNREAD_GUARD
                            || guard == go_routes::UNNAMED_FUNCTION_GUARD
                    })
                    .then_some(GuardsUnknownReason::UnreadMiddleware);
                let unknown = first_reason(draft.unknown, first_reason(placeholder, layer));
                EntryPoint {
                    key: format!(
                        "{}:{}:{}:{}{}",
                        self.file.file,
                        draft.range.start_line,
                        draft.range.start_column,
                        draft.kind.name(),
                        method
                    ),
                    file: self.file.file.clone(),
                    kind: draft.kind,
                    range: draft.range,
                    symbol: draft.symbol,
                    framework: draft.framework.to_string(),
                    route: draft.route,
                    guards: draft.guards,
                    guards_complete: unknown.is_none(),
                    guards_unknown_reason: unknown,
                    basis: draft.basis,
                    inline_handler: draft.inline_handler,
                    in_test: self.file.is_test_at(draft.range.start_byte),
                }
            })
            .collect();
        // One registration can give more than one entry, as a router that two paths mount
        // does. The path then separates the keys. A key without a collision does not change.
        // Two entries with the same path, as two calls of a function give, get the number of
        // their place in the walk. The walk order is the same for each run.
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for entry in &entries {
            *counts.entry(entry.key.clone()).or_default() += 1;
        }
        for entry in &mut entries {
            if counts[&entry.key] > 1 {
                if let Some(path) = entry.route.as_ref().map(Route::path_text) {
                    entry.key = format!("{}:{}", entry.key, path);
                }
            }
        }
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut totals: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for entry in &entries {
            *totals.entry(entry.key.clone()).or_default() += 1;
        }
        for entry in &mut entries {
            if totals[&entry.key] > 1 {
                let number = seen.entry(entry.key.clone()).or_default();
                let suffixed = format!("{}#{number}", entry.key);
                *number += 1;
                entry.key = suffixed;
            }
        }
        entries
    }

    /// Visits the node and its descendants in source order. The visit keeps its own stack, so a deep
    /// tree does not overflow the stack of the thread.
    /// In a Rails routes file, the visit also gives each node its scope from the scope of its parent,
    /// so no node asks for its ancestors. A node with no readable scope has no readable descendant,
    /// and the visit does not enter it.
    fn walk(&self, root: Node<'_>, out: &mut Vec<Draft>) {
        if self.file.language == Language::Ruby && self.file.file.ends_with("routes.rb") {
            self.walk_scoped(
                root,
                None,
                rails::root_scope(),
                false,
                &mut |node, scope| self.visit(node, Some(scope), out),
            );
            return;
        }
        let mut stack: Vec<Node<'_>> = vec![root];
        while let Some(node) = stack.pop() {
            self.visit(node, None, out);
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
    }

    /// Visits the nodes of Rails routes from `start` in source order, each with its scope. The
    /// block of a loop over a literal list is visited one time for each item. `entered` tells that
    /// `scope` is the scope of `start`, and not of its parent.
    fn walk_scoped<'t>(
        &self,
        start: Node<'t>,
        parent: Option<Node<'t>>,
        scope: std::rc::Rc<rails::Scope>,
        entered: bool,
        visit: &mut dyn FnMut(Node<'t>, &std::rc::Rc<rails::Scope>),
    ) {
        // The flag tells that the scope is the scope of the node, and not of its parent.
        let mut stack: Vec<(Node<'t>, Option<Node<'t>>, std::rc::Rc<rails::Scope>, bool)> =
            vec![(start, parent, scope, entered)];
        while let Some((node, parent, outer, entered)) = stack.pop() {
            let scope = if entered {
                outer
            } else {
                match self.scope_inside(node, parent, outer) {
                    rails::Entered::One(scope) => scope,
                    rails::Entered::Each(scopes) => {
                        stack.extend(
                            scopes
                                .into_iter()
                                .rev()
                                .map(|scope| (node, parent, scope, true)),
                        );
                        continue;
                    }
                    rails::Entered::No => continue,
                }
            };
            visit(node, &scope);
            let mut cursor = node.walk();
            let children: Vec<Node<'t>> = node.children(&mut cursor).collect();
            stack.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child, Some(node), scope.clone(), false)),
            );
        }
    }

    fn visit(
        &self,
        node: Node<'_>,
        scope: Option<&std::rc::Rc<rails::Scope>>,
        out: &mut Vec<Draft>,
    ) {
        match self.file.language {
            Language::Python => self.python(node, out),
            Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => {
                self.ecma(node, out)
            }
            Language::Java => self.java(node, out),
            Language::CSharp => self.csharp(node, out),
            Language::Go => self.go(node, out),
            Language::Rust => self.rust(node, out),
            Language::Php => self.php(node, out),
            Language::Ruby => self.ruby(node, scope, out),
            Language::C | Language::Cpp | Language::Bash => {}
        }
    }

    /// The entries that a name or a file location gives.
    fn convention_entries(&self, out: &mut Vec<Draft>) {
        let language = self.file.language;
        for rule in self
            .rules
            .iter()
            .filter(|rule| rule.languages.contains(&language))
        {
            for symbol in &self.file.symbols {
                if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                    && rule.symbols.contains(&symbol.name)
                {
                    out.push(Draft {
                        range: symbol.range,
                        kind: rule.kind,
                        symbol: Some(symbol.key.to_string()),
                        framework: rule.framework,
                        route: None,
                        guards: Vec::new(),
                        basis: EntryBasis::Convention,
                        variant: None,
                        inline_handler: None,
                        unknown: None,
                    });
                }
            }
        }
        if language == Language::Bash {
            if self.source.starts_with(b"#!") {
                out.push(Draft {
                    range: Span::of(&self.parsed.root_node()),
                    kind: EntryKind::CliMain,
                    symbol: None,
                    framework: "bash",
                    route: None,
                    guards: Vec::new(),
                    basis: EntryBasis::Convention,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
            }
            return;
        }
        if matches!(
            language,
            Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx
        ) {
            self.next_route_file(out);
        }
        let framework = match language {
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Java => "java",
            Language::CSharp => "csharp",
            _ => return,
        };
        let main_name = if language == Language::CSharp {
            "Main"
        } else {
            "main"
        };
        // The first `main` of each container path. A later `main` that is the
        // same item under a different `cfg` condition is not a second entry point.
        let mut first_mains: std::collections::HashMap<&[String], usize> = Default::default();
        for (position, symbol) in self.file.symbols.iter().enumerate() {
            if symbol.name != main_name
                || !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            {
                continue;
            }
            let path = self.file.link_facts[position].container_path.as_slice();
            let first = *first_mains.entry(path).or_insert(position);
            if self.file.are_condition_twins(&[first, position]) {
                continue;
            }
            let parent = self.file.link_facts[position]
                .parent
                .map(|parent| self.file.symbols[parent].kind);
            let top_level = parent.is_none_or(|kind| kind == SymbolKind::Module);
            let static_member = matches!(language, Language::Java | Language::CSharp)
                && symbol.signature.contains("static");
            if top_level || static_member {
                out.push(Draft {
                    range: symbol.range,
                    kind: EntryKind::CliMain,
                    symbol: Some(symbol.key.to_string()),
                    framework,
                    route: None,
                    guards: Vec::new(),
                    basis: EntryBasis::Convention,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
            }
        }
    }

    /// A file `app/**/route.ts` of the Next router exports one function for each method.
    fn next_route_file(&self, out: &mut Vec<Draft>) {
        let parts: Vec<&str> = self.file.file.split('/').collect();
        let Some(app) = parts.iter().position(|part| *part == "app") else {
            return;
        };
        let Some(last) = parts.last() else {
            return;
        };
        if !last.starts_with("route.") {
            return;
        }
        let segments: Vec<String> = parts[app + 1..parts.len() - 1]
            .iter()
            .filter(|part| !(part.starts_with('(') && part.ends_with(')')))
            .map(|part| {
                match part
                    .strip_prefix('[')
                    .and_then(|rest| rest.strip_suffix(']'))
                {
                    Some(name) => format!("{{{}}}", name.trim_start_matches("...")),
                    None => (*part).to_string(),
                }
            })
            .collect();
        let path = join_path("", &segments.join("/"));
        for symbol in &self.file.symbols {
            let verb = symbol.name.as_str();
            if symbol.kind == SymbolKind::Function
                && symbol.container.is_none()
                && matches!(
                    verb,
                    "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
                )
            {
                out.push(Draft {
                    range: symbol.range,
                    kind: EntryKind::HttpHandler,
                    symbol: Some(symbol.key.to_string()),
                    framework: "next",
                    route: Some(route(verb, path.clone())),
                    guards: Vec::new(),
                    basis: EntryBasis::Convention,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
            }
        }
    }

    fn python(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        match node.kind() {
            "decorated_definition" => self.python_decorated(node, out),
            "call" => self.python_django(node, out),
            "if_statement" => {
                let condition = node
                    .child_by_field_name("condition")
                    .map(|condition| self.text(condition))
                    .unwrap_or("");
                if condition.contains("__name__") && condition.contains("__main__") {
                    out.push(Draft {
                        range: Span::of(&node),
                        kind: EntryKind::CliMain,
                        symbol: None,
                        framework: "python",
                        route: None,
                        guards: Vec::new(),
                        basis: EntryBasis::Convention,
                        variant: None,
                        inline_handler: None,
                        unknown: None,
                    });
                }
            }
            _ => {}
        }
    }

    fn python_decorated(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let mut cursor = node.walk();
        let decorators: Vec<Node<'_>> = node
            .children(&mut cursor)
            .filter(|child| child.kind() == "decorator")
            .collect();
        let symbol = node
            .child_by_field_name("definition")
            .and_then(|definition| definition.child_by_field_name("name"))
            .and_then(|name| self.symbol_at(name));
        let decorator_guards: Vec<Option<String>> = decorators
            .iter()
            .map(|decorator| python_guard(*decorator, self.source))
            .collect();
        for (position, decorator) in decorators.iter().enumerate() {
            let Some(expression) = decorator.named_child(0) else {
                continue;
            };
            if expression.kind() != "call" {
                continue;
            }
            let Some(callee) = expression.child_by_field_name("function") else {
                continue;
            };
            let name = self.text(callee).rsplit('.').next().unwrap_or("");
            let arguments = Self::arguments_of(expression);
            if (name == "task" || name == "shared_task") && self.imports("celery") {
                out.push(Draft {
                    range: Span::of(decorator),
                    kind: EntryKind::MessageConsumer,
                    symbol: symbol.clone(),
                    framework: "celery",
                    route: None,
                    guards: Vec::new(),
                    basis: EntryBasis::Declared,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
                continue;
            }
            let is_method = callee.kind() == "attribute";
            let framework = if self.imports("fastapi") {
                "fastapi"
            } else if self.imports("flask") {
                "flask"
            } else {
                continue;
            };
            if !is_method || !(name == "route" || HTTP_VERBS.contains(&name)) {
                continue;
            }
            let (markers, framework_module, exports): (&[&str], &str, &[&str]) =
                if framework == "fastapi" {
                    (
                        &["FastAPI(", "APIRouter("],
                        "fastapi",
                        &["FastAPI", "APIRouter"],
                    )
                } else {
                    (&["Flask(", "Blueprint("], "flask", &["Flask", "Blueprint"])
                };
            let Some(receiver) = callee.child_by_field_name("object") else {
                continue;
            };
            // The answer for a receiver needs a search of the file. A file has one framework,
            // so the text of the receiver gives the answer for each later route.
            let known = self
                .python_receivers
                .borrow()
                .get(self.text(receiver))
                .copied();
            let is_framework_value = known.unwrap_or_else(|| {
                let answer =
                    self.is_framework_value(receiver, markers, &[(framework_module, exports)]);
                self.python_receivers
                    .borrow_mut()
                    .insert(self.text(receiver), answer);
                answer
            });
            if !is_framework_value {
                continue;
            }
            let Some(path) = arguments
                .iter()
                .find(|argument| self.is_string(**argument))
                .map(|argument| self.string_of(*argument))
            else {
                continue;
            };
            let methods: Vec<String> = if name == "route" {
                let listed: Vec<String> = arguments
                    .iter()
                    .filter(|argument| argument.kind() == "keyword_argument")
                    .filter(|argument| {
                        argument
                            .child_by_field_name("name")
                            .is_some_and(|key| self.text(key) == "methods")
                    })
                    .flat_map(|argument| {
                        let mut found = Vec::new();
                        if let Some(value) = argument.child_by_field_name("value") {
                            collect_strings(value, self, &mut found);
                        }
                        found
                    })
                    .map(|method| method.to_uppercase())
                    .collect();
                if listed.is_empty() {
                    vec!["GET".to_string()]
                } else {
                    listed
                }
            } else {
                vec![name.to_uppercase()]
            };
            // A decorator above the route decorator wraps the function after the framework
            // registers it. The framework does not call the wrapper, so it is not a guard.
            let mut guards: Vec<String> = decorator_guards[position + 1..]
                .iter()
                .flatten()
                .cloned()
                .collect();
            let mut route = route(&methods.join(","), path);
            self.guard_gap.set(None);
            if framework == "fastapi" {
                let (dependencies, parameters) =
                    self.fastapi_declared(node, *decorator, expression, receiver);
                guards.extend(dependencies);
                route.parameters.extend(parameters);
                keep_first(&mut route.parameters);
            }
            keep_first(&mut guards);
            out.push(Draft {
                range: Span::of(decorator),
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework,
                route: Some(route),
                guards,
                basis: EntryBasis::Declared,
                variant: None,
                inline_handler: None,
                unknown: self.guard_gap.take(),
            });
        }
    }

    /// The known guards in the decorators of the view function of a Django route. The function
    /// can be in a different file. Each decorator wraps the function before the route gets it.
    fn python_view_guards(&self, view: (Option<usize>, usize)) -> Vec<String> {
        match view {
            (None, position) => python_function_guards(
                self.parsed.root_node(),
                self.source,
                &self.file.symbols[position].range,
            ),
            (Some(file), position) => self.all_parsed.tree(file).map_or_else(Vec::new, |parsed| {
                python_function_guards(
                    parsed.root_node(),
                    &parsed.source,
                    &self.all[file].symbols[position].range,
                )
            }),
        }
    }

    /// The file and the position of the view function of a Django route. The file `None` is
    /// this file. A name is the function of this file, or the one function with the name. A
    /// qualified name, as `views.detail`, is never a function of this file: it is the one
    /// function with the name in the module that the import of the qualifier gives. If the
    /// index did not resolve the import, it is the one function with the name in the
    /// repository.
    fn python_view(&self, view: Node<'_>) -> Option<(Option<usize>, usize)> {
        use crate::index::model::ImportTarget;
        if view.kind() == "identifier" {
            return self.callable_named(self.text(view), None);
        }
        if view.kind() != "attribute" {
            return None;
        }
        let qualifier = view.child_by_field_name("object")?;
        let name = self.text(view.child_by_field_name("attribute")?);
        let targets: Vec<&str> = self
            .file
            .imports
            .iter()
            .filter(|binding| {
                qualifier.kind() == "identifier"
                    && binding.local_name.as_deref() == Some(self.text(qualifier))
            })
            .flat_map(|binding| match &binding.target_file {
                ImportTarget::Resolved(file) => std::slice::from_ref(file),
                ImportTarget::Package(files) => files.as_slice(),
                _ => &[],
            })
            .map(String::as_str)
            .collect();
        let candidates: Vec<(usize, usize)> = self
            .all_parsed
            .shared()
            .callables
            .get_or_init(|| callables_by_name(self.all))
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|(file, position)| {
                self.all[*file].symbols[*position].container.is_none()
                    && (targets.is_empty() || targets.contains(&self.all[*file].file.as_str()))
            })
            .collect();
        let [(file, position)] = candidates.as_slice() else {
            return None;
        };
        Some((Some(*file), *position))
    }

    fn python_django(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        if callee.kind() != "identifier"
            || !matches!(self.text(callee), "path" | "re_path" | "url")
            || !self.imports("django")
        {
            return;
        }
        let arguments = Self::arguments_of(node);
        let (Some(first), Some(view)) = (arguments.first(), arguments.get(1)) else {
            return;
        };
        if !self.is_string(*first) {
            return;
        }
        let Some(target) = self.python_view(*view) else {
            return;
        };
        let (file, position) = target;
        let symbol = &file.map_or(self.file, |file| &self.all[file]).symbols[position];
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: Some(symbol.key.to_string()),
            framework: "django",
            route: Some(route("ANY", join_path("", &self.string_of(*first)))),
            guards: self.python_view_guards(target),
            basis: EntryBasis::Registered,
            variant: None,
            inline_handler: None,
            unknown: None,
        });
    }

    fn annotations_of<'t>(&self, node: Node<'t>) -> Vec<Node<'t>> {
        let mut found = Vec::new();
        let mut cursor = node.walk();
        let children: Vec<Node<'t>> = node.children(&mut cursor).collect();
        for child in children {
            match child.kind() {
                "modifiers" => {
                    let mut inner = child.walk();
                    found.extend(child.children(&mut inner).filter(|modifier| {
                        matches!(modifier.kind(), "annotation" | "marker_annotation")
                    }));
                }
                "attribute_list" => {
                    let mut inner = child.walk();
                    found.extend(
                        child
                            .children(&mut inner)
                            .filter(|attribute| attribute.kind() == "attribute"),
                    );
                }
                _ => {}
            }
        }
        found
    }

    fn annotation_name(&self, annotation: Node<'_>) -> &'a str {
        annotation
            .child_by_field_name("name")
            .map(|name| self.text(name))
            .unwrap_or("")
    }

    /// The value of the pair `key = value` in an annotation, or the first string.
    fn annotation_value(&self, annotation: Node<'_>, keys: &[&str]) -> Option<String> {
        let mut cursor = annotation.walk();
        let list: Vec<Node<'_>> = annotation.named_children(&mut cursor).collect();
        for list in list {
            if !matches!(
                list.kind(),
                "annotation_argument_list" | "attribute_argument_list"
            ) {
                continue;
            }
            let mut inner = list.walk();
            let arguments: Vec<Node<'_>> = list.named_children(&mut inner).collect();
            for argument in &arguments {
                let key = match argument.kind() {
                    "element_value_pair" => argument.child_by_field_name("key"),
                    "attribute_argument" => argument.child_by_field_name("name"),
                    _ => None,
                }
                .map(|key| self.text(key));
                if key.is_none_or(|key| keys.contains(&key)) {
                    if let Some(found) = self.first_string(*argument) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    fn spring_method(&self, annotation: Node<'_>, name: &str) -> Option<String> {
        let by_name = match name {
            "GetMapping" => "GET",
            "PostMapping" => "POST",
            "PutMapping" => "PUT",
            "DeleteMapping" => "DELETE",
            "PatchMapping" => "PATCH",
            "RequestMapping" => "",
            _ => return None,
        };
        if !by_name.is_empty() {
            return Some(by_name.to_string());
        }
        let text = self.text(annotation);
        let method = ["GET", "POST", "PUT", "DELETE", "PATCH"]
            .iter()
            .find(|method| text.contains(&format!("RequestMethod.{method}")))
            .copied()
            .unwrap_or("ANY");
        Some(method.to_string())
    }

    fn java(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() != "method_declaration" {
            return;
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let symbol = self.symbol_at(name_node);
        let annotations = self.annotations_of(node);
        let class = Self::enclosing(node, "class_declaration");
        let class_annotations = class
            .map(|class| self.annotations_of(class))
            .unwrap_or_default();
        let guards: Vec<String> = class_annotations
            .iter()
            .chain(&annotations)
            .map(|annotation| self.annotation_name(*annotation))
            .filter(|name| JAVA_GUARDS.contains(name))
            .map(str::to_string)
            .collect();
        let prefix = class_annotations
            .iter()
            .find(|annotation| self.annotation_name(**annotation) == "RequestMapping")
            .and_then(|annotation| self.annotation_value(*annotation, &["value", "path"]))
            .unwrap_or_default();
        for annotation in &annotations {
            let name = self.annotation_name(*annotation);
            if let Some(method) = self.spring_method(*annotation, name) {
                let path = self
                    .annotation_value(*annotation, &["value", "path"])
                    .unwrap_or_default();
                out.push(Draft {
                    range: Span::of(&node),
                    kind: EntryKind::HttpHandler,
                    symbol: symbol.clone(),
                    framework: "spring",
                    route: Some(route(&method, join_path(&prefix, &path))),
                    guards: guards.clone(),
                    basis: EntryBasis::Declared,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
            } else if JAVA_CONSUMERS.contains(&name) || name == "Scheduled" {
                out.push(Draft {
                    range: Span::of(&node),
                    kind: if name == "Scheduled" {
                        EntryKind::ScheduledJob
                    } else {
                        EntryKind::MessageConsumer
                    },
                    symbol: symbol.clone(),
                    framework: "spring",
                    route: None,
                    guards: guards.clone(),
                    basis: EntryBasis::Declared,
                    variant: None,
                    inline_handler: None,
                    unknown: None,
                });
            }
        }
    }

    fn enclosing<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
        let mut parent = node.parent();
        while let Some(candidate) = parent {
            if candidate.kind() == kind {
                return Some(candidate);
            }
            parent = candidate.parent();
        }
        None
    }

    fn csharp(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        match node.kind() {
            "method_declaration" => self.csharp_action(node, out),
            "invocation_expression" => self.csharp_minimal(node, out),
            _ => {}
        }
    }

    fn csharp_action(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let action = self.text(name_node);
        let attributes = self.annotations_of(node);
        let class = Self::enclosing(node, "class_declaration");
        let class_attributes = class
            .map(|class| self.annotations_of(class))
            .unwrap_or_default();
        let controller = class
            .and_then(|class| class.child_by_field_name("name"))
            .map(|name| {
                self.text(name)
                    .trim_end_matches("Controller")
                    .to_lowercase()
            })
            .unwrap_or_default();
        let expand = |path: String| {
            path.replace("[controller]", &controller)
                .replace("[action]", &action.to_lowercase())
        };
        let prefix = class_attributes
            .iter()
            .find(|attribute| self.annotation_name(**attribute) == "Route")
            .and_then(|attribute| self.annotation_value(*attribute, &[]))
            .map(&expand)
            .unwrap_or_default();
        let guards: Vec<String> = class_attributes
            .iter()
            .chain(&attributes)
            .map(|attribute| self.annotation_name(*attribute))
            .filter(|name| CSHARP_GUARDS.contains(name))
            .map(str::to_string)
            .collect();
        let symbol = self.symbol_at(name_node);
        for attribute in &attributes {
            let method = match self.annotation_name(*attribute) {
                "HttpGet" => "GET",
                "HttpPost" => "POST",
                "HttpPut" => "PUT",
                "HttpDelete" => "DELETE",
                "HttpPatch" => "PATCH",
                _ => continue,
            };
            let path = self
                .annotation_value(*attribute, &[])
                .map(&expand)
                .unwrap_or_default();
            out.push(Draft {
                range: Span::of(&node),
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework: "aspnet",
                route: Some(route(method, join_path(&prefix, &path))),
                guards: guards.clone(),
                basis: EntryBasis::Declared,
                variant: None,
                inline_handler: None,
                unknown: None,
            });
        }
    }

    fn csharp_minimal(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        if function.kind() != "member_access_expression" {
            return;
        }
        let name = function
            .child_by_field_name("name")
            .map(|name| self.text(name))
            .unwrap_or("");
        let Some(method) = name.strip_prefix("Map").map(str::to_uppercase) else {
            return;
        };
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH") {
            return;
        }
        let arguments = Self::arguments_of(node);
        let (Some(path), Some(handler)) = (arguments.first(), arguments.last()) else {
            return;
        };
        if arguments.len() < 2 || !self.is_string(*path) {
            return;
        }
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: self.handler_symbol(*handler),
            framework: "aspnet",
            route: Some(route(&method, join_path("", &self.string_of(*path)))),
            guards: Vec::new(),
            basis: EntryBasis::Registered,
            variant: None,
            inline_handler: None,
            unknown: None,
        });
    }

    fn go(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() == "source_file" {
            go_routes::collect(self, node, out);
            go_wrappers::collect(self, node, out);
            return;
        }
        if node.kind() != "call_expression" {
            return;
        }
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        if function.kind() != "selector_expression" {
            return;
        }
        let Some(field) = function.child_by_field_name("field") else {
            return;
        };
        let name = self.text(field);
        let Some(receiver) = function.child_by_field_name("operand") else {
            return;
        };
        let arguments = Self::arguments_of(node);
        let (Some(first), Some(handler)) = (arguments.first(), arguments.last()) else {
            return;
        };
        if arguments.len() < 2 || !self.is_string(*first) {
            return;
        }
        if !(matches!(name, "HandleFunc" | "Handle")
            && self.imports("net.http")
            && self.is_go_http_receiver(receiver))
        {
            return;
        }
        let Some((method, path)) = self
            .go_string(*first)
            .and_then(|pattern| go_routes::servemux_pattern(&pattern))
        else {
            return;
        };
        if self.go_covered_patterns().contains(&path) || self.go_in_dead_branch(node) {
            return;
        }
        let wrapped = self.go_handler(*handler, go_routes::HandlerKind::Http);
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: wrapped.symbol,
            framework: "net/http",
            route: Some(Route {
                method,
                parameters: go_routes::servemux_parameters(&path),
                path: Some(path),
                path_template: None,
            }),
            guards: wrapped.guards,
            basis: EntryBasis::Registered,
            variant: None,
            inline_handler: None,
            unknown: wrapped
                .unread_wrapper
                .then_some(GuardsUnknownReason::UnreadMiddleware),
        });
    }

    fn rust(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        match node.kind() {
            "function_item" => self.rust_function(node, out),
            "call_expression" => self.rust_axum(node, out),
            _ => {}
        }
    }

    fn rust_function(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let mut attributes = Vec::new();
        let mut previous = node.prev_sibling();
        while let Some(sibling) = previous {
            if sibling.kind() != "attribute_item" && !sibling.kind().contains("comment") {
                break;
            }
            if sibling.kind() == "attribute_item" {
                attributes.push(sibling);
            }
            previous = sibling.prev_sibling();
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let symbol = self.symbol_at(name_node);
        let first = attributes.last().copied().unwrap_or(node);
        let range = Span::between(&first, &node);
        let exported = self.text(node).starts_with("pub extern")
            || node.children(&mut node.walk()).any(|child| {
                child.kind() == "function_modifiers" && self.text(child).contains("extern")
            });
        let no_mangle = attributes
            .iter()
            .any(|attribute| self.text(*attribute).contains("no_mangle"));
        if no_mangle || (exported && self.text(node).trim_start().starts_with("pub")) {
            out.push(Draft {
                range,
                kind: EntryKind::FfiExport,
                symbol: symbol.clone(),
                framework: "ffi",
                route: None,
                guards: Vec::new(),
                basis: EntryBasis::Declared,
                variant: None,
                inline_handler: None,
                unknown: None,
            });
        }
        for attribute in attributes.iter().rev() {
            let text = self.text(*attribute);
            let Some(inner) = text
                .trim()
                .strip_prefix("#[")
                .and_then(|rest| rest.strip_suffix(']'))
            else {
                continue;
            };
            let Some((verb, rest)) = inner.split_once('(') else {
                continue;
            };
            let verb = verb.trim();
            if !HTTP_VERBS.contains(&verb) || verb == "all" {
                continue;
            }
            let Some(path) = rest.split('"').nth(1).filter(|path| path.starts_with('/')) else {
                continue;
            };
            let framework = if self.imports("rocket") {
                "rocket"
            } else {
                "actix"
            };
            out.push(Draft {
                range,
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework,
                route: Some(route(verb, path.to_string())),
                guards: Vec::new(),
                basis: EntryBasis::Declared,
                variant: None,
                inline_handler: None,
                unknown: None,
            });
        }
    }

    /// The `net/http` package itself, or a variable that has one binding in the file: a call of
    /// `http.NewServeMux()`. A name that a local variable hides is not the package.
    fn is_go_http_receiver(&self, receiver: Node<'_>) -> bool {
        let text = self.text(receiver);
        let names = go_routes::net_http_names(self.file);
        if names.iter().any(|name| name == text) {
            return !self.is_go_shadowed(receiver);
        }
        receiver.kind() == "identifier" && self.is_go_servemux(text)
    }

    fn rust_axum(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        if function.kind() != "field_expression"
            || function
                .child_by_field_name("field")
                .is_none_or(|field| self.text(field) != "route")
            || !self.mentions("axum")
        {
            return;
        }
        let arguments = Self::arguments_of(node);
        let (Some(path), Some(routers)) = (arguments.first(), arguments.get(1)) else {
            return;
        };
        if !self.is_string(*path) || !self.is_router_chain(function) {
            return;
        }
        let path = self.string_of(*path);
        let mut handlers = Vec::new();
        self.method_router(*routers, &mut handlers);
        for (verb, handler) in handlers {
            out.push(Draft {
                range: Span::of(&node),
                kind: EntryKind::HttpHandler,
                symbol: handler.and_then(|handler| self.handler_symbol(handler)),
                framework: "axum",
                route: Some(route(&verb, path.clone())),
                guards: Vec::new(),
                basis: EntryBasis::Registered,
                variant: None,
                inline_handler: None,
                unknown: None,
            });
        }
    }

    /// A chain of `.route(...)` calls that starts at `Router::new()`, or at a value that
    /// the file binds to a router.
    fn is_router_chain(&self, function: Node<'_>) -> bool {
        let mut current = function;
        loop {
            let Some(value) = current.child_by_field_name("value") else {
                return false;
            };
            if value.kind() != "call_expression" {
                return self.is_framework_value(value, &["Router"], &[("axum", &["Router"])]);
            }
            let Some(inner) = value.child_by_field_name("function") else {
                return false;
            };
            if inner.kind() == "field_expression" {
                current = inner;
                continue;
            }
            return self.text(inner).contains("Router");
        }
    }

    fn method_router<'t>(&self, node: Node<'t>, out: &mut Vec<(String, Option<Node<'t>>)>) {
        let mut node = node;
        while node.kind() == "call_expression" {
            let Some(function) = node.child_by_field_name("function") else {
                return;
            };
            let handler = Self::arguments_of(node).first().copied();
            match function.kind() {
                "identifier" if HTTP_VERBS.contains(&self.text(function)) => {
                    out.push((self.text(function).to_uppercase(), handler));
                    return;
                }
                "field_expression" => {
                    if let Some(field) = function.child_by_field_name("field") {
                        if HTTP_VERBS.contains(&self.text(field)) {
                            out.push((self.text(field).to_uppercase(), handler));
                        }
                    }
                    let Some(value) = function.child_by_field_name("value") else {
                        return;
                    };
                    node = value;
                }
                _ => return,
            }
        }
    }

    fn php(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() != "scoped_call_expression" {
            return;
        }
        let (Some(scope), Some(name)) = (
            node.child_by_field_name("scope"),
            node.child_by_field_name("name"),
        ) else {
            return;
        };
        let verb = self.text(name);
        let facade = self.file.file.starts_with("routes/")
            || self.file.imports.iter().any(|binding| {
                binding.local_name.as_deref() == Some("Route")
                    && binding
                        .module_path
                        .replace('\\', ".")
                        .ends_with("Facades.Route")
            });
        if !facade
            || self.text(scope) != "Route"
            || !matches!(
                verb,
                "get" | "post" | "put" | "patch" | "delete" | "options" | "any"
            )
        {
            return;
        }
        let arguments = Self::arguments_of(node);
        let Some(path) = arguments.first() else {
            return;
        };
        let target = arguments.get(1).copied();
        let symbol = target.and_then(|target| match target.kind() {
            "array_creation_expression" => {
                let strings = self.class_and_method(target);
                strings.and_then(|(class, method)| self.symbol_named(&method, Some(&class)))
            }
            "string" | "encapsed_string" => {
                let text = self.string_of(target);
                text.split_once('@')
                    .and_then(|(class, method)| self.symbol_named(method, Some(class)))
            }
            _ => None,
        });
        let mut guards = Vec::new();
        let mut current = node;
        while let Some(parent) = current.parent() {
            if parent.kind() != "member_call_expression"
                || parent.child_by_field_name("object") != Some(current)
            {
                break;
            }
            if parent
                .child_by_field_name("name")
                .is_some_and(|name| self.text(name) == "middleware")
            {
                for argument in Self::arguments_of(parent) {
                    guards.push(self.string_of(argument));
                }
            }
            current = parent;
        }
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol,
            framework: "laravel",
            route: Some(route(
                if verb == "any" { "ANY" } else { verb },
                join_path("", &self.string_of(*path)),
            )),
            guards,
            basis: EntryBasis::Registered,
            variant: None,
            inline_handler: None,
            unknown: None,
        });
    }

    /// `[UserController::class, 'show']` gives `UserController` and `show`.
    fn class_and_method(&self, array: Node<'_>) -> Option<(String, String)> {
        let mut cursor = array.walk();
        let items: Vec<Node<'_>> = array.named_children(&mut cursor).collect();
        let class = self.text(items.first()?.named_child(0)?);
        let class = class
            .strip_suffix("::class")?
            .rsplit('\\')
            .next()?
            .to_string();
        let method = self.string_of(items.get(1)?.named_child(0)?);
        Some((class, method))
    }
}

fn collect_strings(node: Node<'_>, context: &Context<'_>, out: &mut Vec<String>) {
    crate::parser::walk_preorder(node, |descendant| {
        if descendant != node && !descendant.is_named() {
            Walk::SkipChildren
        } else if context.is_string(descendant) {
            out.push(context.string_of(descendant));
            Walk::SkipChildren
        } else {
            Walk::Children
        }
    });
}

#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use super::*;
    use crate::core::Language;
    use crate::index::extract::{extract_file, link_files};
    use crate::index::imports::{resolve_imports, Manifests};
    use crate::index::resolve::resolve_references;
    pub(super) use crate::index::timing::{assert_scales_linearly, assert_scales_linearly_above};
    use crate::parser::Parser;

    /// The entry points of one file, found on a thread with a small stack.
    fn deep_entries(language: Language, file: &'static str, source: String) -> Vec<EntryPoint> {
        crate::parser::deep::on_small_stack(move || entries(language, &[(file, source.as_str())]))
    }

    fn frameworks(found: &[EntryPoint]) -> Vec<String> {
        found.iter().map(|entry| entry.framework.clone()).collect()
    }

    /// `bound_to_marker` reads each node of a file with deep nesting.
    #[test]
    fn an_axum_router_in_a_variable_beside_deep_nesting() {
        use crate::parser::deep::nest;
        let source = format!(
            "use axum::routing::get;\n\nasync fn list() {{}}\n\nfn deep() -> i32 {{ {} }}\n\nfn app() {{\n    let router = Router::new();\n    router.route(\"/x\", get(list));\n}}\n",
            nest("(", "1", ")")
        );
        deep_entries(Language::Rust, "main.rs", source);
    }

    /// `method_router` reads each call of a long chain of methods.
    #[test]
    fn an_axum_route_with_a_long_chain_of_methods() {
        let source = format!(
            "use axum::{{routing::get, Router}};\n\nasync fn list() {{}}\n\npub fn app() -> Router {{\n    Router::new().route(\"/x\", get(list){})\n}}\n",
            ".post(list)".repeat(crate::parser::deep::DEPTH)
        );
        let found = deep_entries(Language::Rust, "main.rs", source);
        assert!(frameworks(&found).contains(&"axum".to_string()));
    }

    /// `first_string` reads each level of a deep annotation argument.
    #[test]
    fn a_spring_mapping_with_a_deep_path() {
        use crate::parser::deep::nest;
        let source = format!(
            "@RestController\npublic class UserController {{\n    @GetMapping({})\n    public String show() {{\n        return \"\";\n    }}\n}}\n",
            nest("(", "\"/users\"", ")")
        );
        deep_entries(Language::Java, "UserController.java", source);
    }

    /// `collect_strings` reads each level of the methods of a Flask route.
    #[test]
    fn a_flask_route_with_deep_methods() {
        use crate::parser::deep::nest;
        let source = format!(
            "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/x\", methods={})\ndef update():\n    return 1\n",
            nest("[", "\"POST\"", "]")
        );
        deep_entries(Language::Python, "app.py", source);
    }

    fn go_main(body: &str) -> String {
        format!(
            "package main\n\nimport \"net/http\"\n\nfunc health(w http.ResponseWriter, r *http.Request) {{}}\n\nfunc auth(next http.Handler) http.Handler {{ return next }}\n\nfunc main() {{\n\tmux := http.NewServeMux()\n{body}\n\thttp.ListenAndServe(\":80\", mux)\n}}\n"
        )
    }

    /// `go_handler_at` reads each wrapper call around a handler.
    #[test]
    fn a_go_handler_inside_deep_wrapper_calls() {
        use crate::parser::deep::nest;
        let body = format!("\tmux.Handle(\"/c\", {})", nest("wrap(", "health", ")"));
        deep_entries(Language::Go, "main.go", go_main(&body));
    }

    /// `go_callee` and `go_guard_name` read each call of a chain of calls.
    #[test]
    fn a_go_handler_wrapped_by_a_long_chain_of_calls() {
        let body = format!(
            "\tmux.Handle(\"/c\", auth{}(http.HandlerFunc(health)))",
            "()".repeat(crate::parser::deep::DEPTH)
        );
        deep_entries(Language::Go, "main.go", go_main(&body));
    }

    fn gin_main(body: &str) -> String {
        format!(
            "package main\n\nimport \"github.com/gin-gonic/gin\"\n\nfunc show(c *gin.Context) {{}}\n\nfunc main() {{\n\trouter := gin.Default()\n{body}\n\trouter.GET(\"/a\", show)\n}}\n"
        )
    }

    /// `escape` reads each parenthesis around an argument.
    #[test]
    fn a_go_router_inside_deep_parentheses() {
        use crate::parser::deep::nest;
        let body = format!("\tuse({})", nest("(", "router", ")"));
        deep_entries(Language::Go, "main.go", gin_main(&body));
    }

    /// `literal_values` reads each level of a composite literal.
    #[test]
    fn a_go_router_inside_a_deep_composite_literal() {
        use crate::parser::deep::nest;
        let body = format!(
            "\tvalues := []any{{{}}}\n\t_ = values",
            nest("{", "router", "}")
        );
        deep_entries(Language::Go, "main.go", gin_main(&body));
    }

    /// `eval` reads each call of a long chain of groups.
    #[test]
    fn a_go_router_with_a_long_chain_of_groups() {
        let body = format!(
            "\trouter{}.GET(\"/b\", show)",
            ".Group(\"/g\")".repeat(crate::parser::deep::DEPTH)
        );
        deep_entries(Language::Go, "main.go", gin_main(&body));
    }

    /// `eval` reads each parenthesis around the value of a router.
    #[test]
    fn a_go_router_made_inside_deep_parentheses() {
        use crate::parser::deep::nest;
        let body = format!(
            "\tother := {}\n\tother.GET(\"/b\", show)",
            nest("(", "gin.Default()", ")")
        );
        deep_entries(Language::Go, "main.go", gin_main(&body));
    }

    #[test]
    fn concurrent_requests_for_one_file_parse_it_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let parses = AtomicUsize::new(0);
        let parse_file = |_: usize| {
            parses.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(Parser::new()
                .parse(b"x = 1\n", Language::Python, Path::new("a.py"))
                .ok())
        };
        let trees = OnDemandTrees::new(&parse_file);
        // Each thread keeps its tree until all threads have one, so the tree is in use
        // during each request.
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let tree = trees.tree(0);
                    assert!(tree.is_some());
                    barrier.wait();
                });
            }
        });
        assert_eq!(parses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_tree_that_a_rule_asks_for_is_kept() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let parses = AtomicUsize::new(0);
        let parse_file = |_: usize| {
            parses.fetch_add(1, Ordering::SeqCst);
            Ok(Parser::new()
                .parse(b"x = 1\n", Language::Python, Path::new("a.py"))
                .ok())
        };
        let trees = OnDemandTrees::new(&parse_file);
        drop(trees.tree(0));
        assert!(trees.tree(0).is_some());
        assert!(trees.transient_tree(0).is_some());
        assert_eq!(parses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_tree_for_one_walk_is_not_kept() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let parses = AtomicUsize::new(0);
        let parse_file = |_: usize| {
            parses.fetch_add(1, Ordering::SeqCst);
            Ok(Parser::new()
                .parse(b"x = 1\n", Language::Python, Path::new("a.py"))
                .ok())
        };
        let trees = OnDemandTrees::new(&parse_file);
        let Some(TreeRef::Shared(tree)) = trees.transient_tree(0) else {
            panic!("a shared tree");
        };
        let released = std::sync::Arc::downgrade(&tree);
        drop(tree);
        assert!(released.upgrade().is_none());
        assert!(trees.transient_tree(0).is_some());
        assert_eq!(parses.load(Ordering::SeqCst), 2);
    }

    /// The first thread parses the file and releases the tree before the
    /// second thread reads the stored parse. The second thread then parses the
    /// file again and keeps that parse for the later requests. The hook makes
    /// each read of a stored parse wait until the first thread has released
    /// its tree, thus the order does not depend on the speed of the threads.
    #[test]
    fn a_waiter_whose_tree_was_released_publishes_its_parse_for_later_requests() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let parses = AtomicUsize::new(0);
        let released = AtomicBool::new(false);
        let parse_file = |_: usize| {
            parses.fetch_add(1, Ordering::SeqCst);
            Ok(Parser::new()
                .parse(b"x = 1\n", Language::Python, Path::new("a.py"))
                .ok())
        };
        let wait_for_release = || {
            while !released.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
        };
        let mut trees = OnDemandTrees::new(&parse_file);
        trees.before_read_of_stored_parse = Some(&wait_for_release);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                drop(trees.transient_tree(0));
                released.store(true, Ordering::SeqCst);
            });
            while parses.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            let second = scope.spawn(|| trees.transient_tree(0));
            first.join().unwrap();
            let held = second.join().unwrap();
            assert!(held.is_some());
            assert!(trees.transient_tree(0).is_some());
        });
        assert_eq!(parses.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_file_without_a_tree_is_not_parsed_again() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let parses = AtomicUsize::new(0);
        let parse_file = |_: usize| {
            parses.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        };
        let trees = OnDemandTrees::new(&parse_file);
        assert!(trees.tree(0).is_none());
        assert!(trees.tree(0).is_none());
        assert_eq!(parses.load(Ordering::SeqCst), 1);
    }

    /// A source of the trees of a test that counts the requests.
    struct CountedTrees<'a> {
        parsed: &'a [ParseResult],
        shared: EntryShared,
        requests: std::sync::atomic::AtomicUsize,
    }

    impl TreeSource for CountedTrees<'_> {
        fn tree(&self, position: usize) -> Option<TreeRef<'_>> {
            self.requests
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.parsed.get(position).map(TreeRef::Borrowed)
        }

        fn shared(&self) -> &EntryShared {
            &self.shared
        }
    }

    #[test]
    fn the_go_package_rules_read_each_go_file_once_and_again_only_for_a_factory_call_or_a_router_type(
    ) {
        let sources = [
            ("app/main.go", "package app\n\nimport \"net/http\"\n\nfunc main() {\n\thttp.ListenAndServe(\":80\", newRouter())\n}\n"),
            ("app/routes.go", "package app\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n\nfunc newRouter() chi.Router {\n\tr := chi.NewRouter()\n\tr.Get(\"/a\", list)\n\treturn r\n}\n\nfunc list(w http.ResponseWriter, r *http.Request) {}\n"),
            ("main.go", "package main\n\nimport (\n\t\"net/http\"\n\n\t\"example.com/app/web\"\n)\n\nfunc Home(w http.ResponseWriter, r *http.Request) {}\n\nfunc main() {\n\tm := web.NewRouter()\n\tm.Get(\"/wrapped\", Home)\n}\n"),
            ("other/a.go", "package other\n\nfunc A() int { return 1 }\n"),
            ("other/b.go", "package other\n\nfunc B() int { return A() }\n"),
            ("web/router.go", "package web\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n\ntype Router struct{ chiRouter chi.Router }\n\nfunc NewRouter() *Router {\n\treturn &Router{chiRouter: chi.NewRouter()}\n}\n\nfunc (r *Router) Get(pattern string, h ...any) {\n\tr.chiRouter.Get(pattern, h[0].(http.HandlerFunc))\n}\n"),
        ];
        let parsed: Vec<ParseResult> = sources
            .iter()
            .map(|(name, text)| {
                Parser::new()
                    .parse(text.as_bytes(), Language::Go, Path::new(name))
                    .unwrap()
            })
            .collect();
        let mut index: Vec<FileSymbols> = sources
            .iter()
            .zip(&parsed)
            .map(|((name, _), parsed)| extract_file(parsed, name))
            .collect();
        link_files(&mut index);
        resolve_imports(&mut index, &test_manifests());
        resolve_references(&mut index);
        let trees = CountedTrees {
            parsed: &parsed,
            shared: EntryShared::default(),
            requests: std::sync::atomic::AtomicUsize::new(0),
        };
        let found: Vec<EntryPoint> = (0..index.len())
            .flat_map(|position| {
                entry_points_of_file(position, &parsed[position], &index, &trees, &[])
            })
            .collect();
        let mut paths: Vec<&str> = found
            .iter()
            .filter_map(|entry| entry.route.as_ref().and_then(|route| route.path.as_deref()))
            .collect();
        paths.sort_unstable();
        assert_eq!(paths, ["/a", "/wrapped"]);
        // One walk reads each Go file. The factories and the router types are known only
        // after the walk of all files, so one second walk reads each file that needs them:
        // `app/main.go` calls the factory `newRouter`, `web/router.go` has the router type
        // `Router`, and `main.go` imports its package.
        assert_eq!(
            trees.requests.load(std::sync::atomic::Ordering::SeqCst),
            sources.len() + 3
        );
    }

    pub(super) fn entries(language: Language, files: &[(&str, &str)]) -> Vec<EntryPoint> {
        entries_timed(language, files, true).0
    }

    /// Like `entries`, without the resolution of the references. The search for Rails entry points
    /// does not use them, and the resolution of a large input takes much more time than the search.
    pub(super) fn entries_lean(
        language: Language,
        files: &[(&str, &str)],
    ) -> (Vec<EntryPoint>, std::time::Duration) {
        entries_timed(language, files, false)
    }

    /// The entry points, and the time of the search for entry points alone.
    pub(super) fn entries_timed(
        language: Language,
        files: &[(&str, &str)],
        resolve: bool,
    ) -> (Vec<EntryPoint>, std::time::Duration) {
        let mut files = files.to_vec();
        files.sort_by_key(|(name, _)| *name);
        let files = files.as_slice();
        let parsed: Vec<ParseResult> = files
            .iter()
            .map(|(name, text)| {
                Parser::new()
                    .parse(text.as_bytes(), language, Path::new(name))
                    .unwrap()
            })
            .collect();
        let mut index: Vec<FileSymbols> = files
            .iter()
            .zip(&parsed)
            .map(|((name, _), parsed)| extract_file(parsed, name))
            .collect();
        link_files(&mut index);
        if resolve {
            resolve_imports(&mut index, &test_manifests());
            resolve_references(&mut index);
        }
        let started = crate::index::timing::Stopwatch::start();
        let found = find_entry_points(&parsed, &index);
        (found, started.elapsed())
    }

    /// The Go module `example.com/app` is at the root of the files of a test.
    fn test_manifests() -> Manifests {
        let mut manifests = Manifests::default();
        manifests
            .go_modules
            .insert(String::new(), "example.com/app".to_string());
        manifests
    }

    /// The keys of each draft of the files, before the removal of equal keys.
    fn raw_entry_keys(language: Language, files: &[(&str, &str)]) -> Vec<String> {
        let mut sorted = files.to_vec();
        sorted.sort_by_key(|(name, _)| *name);
        let files = sorted.as_slice();
        let parsed: Vec<ParseResult> = files
            .iter()
            .map(|(name, text)| {
                Parser::new()
                    .parse(text.as_bytes(), language, Path::new(name))
                    .unwrap()
            })
            .collect();
        let mut index: Vec<FileSymbols> = files
            .iter()
            .zip(&parsed)
            .map(|((name, _), parsed)| extract_file(parsed, name))
            .collect();
        link_files(&mut index);
        resolve_imports(&mut index, &test_manifests());
        resolve_references(&mut index);
        let trees = Trees(&parsed, EntryShared::default());
        (0..parsed.len())
            .flat_map(|position| {
                Context::new(
                    position,
                    &parsed[position],
                    &index[position],
                    &index,
                    &trees,
                    &[],
                )
                .detect()
                .into_iter()
                .map(|entry| entry.key)
            })
            .collect()
    }

    fn route(method: &str, path: &str, parameters: &[&str]) -> Option<Route> {
        Some(Route {
            method: method.to_string(),
            path: Some(path.to_string()),
            path_template: None,
            parameters: parameters.iter().map(|name| name.to_string()).collect(),
        })
    }

    fn http_frameworks(language: Language, files: &[(&str, &str)]) -> Vec<String> {
        entries(language, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| entry.framework)
            .collect()
    }

    #[test]
    fn an_express_route_needs_a_receiver_that_is_an_express_value() {
        let source = "const express = require(\"express\");\nconst store = { get(key, callback) { return callback; } };\nconst app = express();\nconst router = express.Router();\n\nstore.get(\"/x\", audit);\napp.get(\"/a\", first);\nrouter.post(\"/b\", second);\n";
        let found = entries(Language::JavaScript, &[("app.js", source)]);
        let lines: Vec<u32> = found.iter().map(|entry| entry.range.start_line).collect();
        assert_eq!(lines, [7, 8], "`store.get` is not a route");
        let imported = "const express = require(\"express\");\nconst { app } = require(\"./server\");\n\napp.get(\"/a\", first);\n";
        let server = "const express = require(\"express\");\nconst app = express();\nmodule.exports = { app };\n";
        assert_eq!(
            http_frameworks(
                Language::JavaScript,
                &[("routes.js", imported), ("server.js", server)]
            ),
            ["express"],
            "a receiver from a module that exports the application"
        );
    }

    #[test]
    fn an_imported_receiver_is_an_application_only_if_its_module_exports_one() {
        let routes = "const express = require(\"express\");\nconst { store } = require(\"./store\");\n\nfunction audit(user) {\n  eval(user);\n}\n\nstore.get(\"/x\", audit);\n";
        let missing = entries(Language::JavaScript, &[("routes.js", routes)]);
        assert!(
            missing
                .iter()
                .all(|entry| entry.kind != EntryKind::HttpHandler),
            "the module is not in the repository"
        );
        let plain = "const store = { get(key, callback) { return callback; } };\nmodule.exports = { store };\n";
        let found = entries(
            Language::JavaScript,
            &[("routes.js", routes), ("store.js", plain)],
        );
        assert!(found
            .iter()
            .all(|entry| entry.kind != EntryKind::HttpHandler));
        let server = "const express = require(\"express\");\nconst store = express();\nmodule.exports = { store };\n";
        let found = entries(
            Language::JavaScript,
            &[("routes.js", routes), ("store.js", server)],
        );
        assert_eq!(
            found
                .iter()
                .filter(|entry| entry.kind == EntryKind::HttpHandler)
                .count(),
            1,
            "the module exports an Express application"
        );
        // `Router` makes a router. It is not a router.
        let from_express = "import { Router } from \"express\";\n\nRouter.get(\"/x\", handler);\n";
        assert!(!entries(Language::JavaScript, &[("a.js", from_express)])
            .iter()
            .any(|entry| entry.kind == EntryKind::HttpHandler));
    }

    #[test]
    fn only_a_constructor_of_the_framework_makes_an_imported_receiver_a_framework_value() {
        let express = "const { request } = require(\"express\");\n\nfunction audit(user) {\n  eval(user);\n}\n\nrequest.get(\"/x\", audit);\n";
        assert!(
            http_frameworks(Language::JavaScript, &[("a.js", express)]).is_empty(),
            "`request` is not an application"
        );
        let flask =
            "from flask import request\n\n@request.get(\"/x\")\ndef audit():\n    return 1\n";
        assert!(http_frameworks(Language::Python, &[("a.py", flask)]).is_empty());
        let router = "const { Router } = require(\"express\");\nconst router = Router();\n\nrouter.get(\"/x\", audit);\n";
        assert_eq!(
            http_frameworks(Language::JavaScript, &[("b.js", router)]),
            ["express"]
        );
    }

    #[test]
    fn a_python_import_needs_a_module_that_binds_a_framework_value() {
        let routes = "from flask import Flask\nfrom store import cache\n\n@cache.get(\"/x\")\ndef cached():\n    return 1\n";
        let plain = "cache = {}\n";
        let found = entries(
            Language::Python,
            &[("routes.py", routes), ("store.py", plain)],
        );
        assert!(found.is_empty(), "{found:?}");
        let server = "from flask import Flask\n\ncache = Flask(__name__)\n";
        let found = entries(
            Language::Python,
            &[("routes.py", routes), ("store.py", server)],
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_python_route_decorator_needs_a_receiver_that_is_a_framework_value() {
        let source = "from flask import Flask\nfrom cachelib import Cache\n\napp = Flask(__name__)\ncache = Cache()\n\n@cache.get(\"/x\")\ndef cached():\n    return 1\n\n@app.route(\"/y\")\ndef shown():\n    return 2\n";
        let found = entries(Language::Python, &[("app.py", source)]);
        let symbols: Vec<Option<&str>> =
            found.iter().map(|entry| entry.symbol.as_deref()).collect();
        assert_eq!(symbols, [Some("app.py#shown")]);
        let api = "from fastapi import FastAPI, APIRouter\n\nrouter = APIRouter()\nstore = {}\n\n@router.get(\"/a\")\ndef first():\n    return 1\n\n@store.get(\"/b\")\ndef second():\n    return 2\n";
        let found = entries(Language::Python, &[("api.py", api)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].symbol.as_deref(), Some("api.py#first"));
    }

    #[test]
    fn a_go_route_needs_a_receiver_that_is_a_framework_value() {
        let source = "package main\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/gin-gonic/gin\"\n)\n\nfunc show(c *gin.Context) {}\n\nfunc health(w http.ResponseWriter, r *http.Request) {}\n\nfunc main() {\n\trouter := gin.Default()\n\tcache := newCache()\n\tmux := http.NewServeMux()\n\trouter.GET(\"/a\", show)\n\tcache.GET(\"/b\", show)\n\tmux.HandleFunc(\"/c\", health)\n\tcache.HandleFunc(\"/d\", health)\n\thttp.HandleFunc(\"/e\", health)\n}\n";
        let found = entries(Language::Go, &[("main.go", source)]);
        let lines: Vec<u32> = found
            .iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| entry.range.start_line)
            .collect();
        assert_eq!(lines, [17, 19, 21]);
    }

    #[test]
    fn an_axum_route_needs_a_router_receiver() {
        let source = "use axum::{routing::get, Router};\n\nstruct Table;\n\nimpl Table {\n    fn route(&self, path: &str, handler: u32) {}\n}\n\nasync fn list() {}\n\nfn app(table: Table) -> Router {\n    table.route(\"/x\", get(list));\n    Router::new().route(\"/items\", get(list))\n}\n\nfn extend(router: Router) -> Router {\n    router.route(\"/more\", get(list))\n}\n";
        let found = entries(Language::Rust, &[("main.rs", source)]);
        let lines: Vec<u32> = found
            .iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| entry.range.start_line)
            .collect();
        assert_eq!(lines, [13, 17]);
    }

    #[test]
    fn a_laravel_route_needs_the_route_facade_or_a_route_file() {
        let other =
            "<?php\nuse App\\Support\\Route;\n\nRoute::get('/x', [Thing::class, 'show']);\n";
        assert!(http_frameworks(Language::Php, &[("app/Thing.php", other)]).is_empty());
        let facade = "<?php\nuse Illuminate\\Support\\Facades\\Route;\n\nRoute::get('/x', [Thing::class, 'show']);\n";
        assert_eq!(
            http_frameworks(Language::Php, &[("app/Boot.php", facade)]),
            ["laravel"]
        );
        let routes = "<?php\nRoute::get('/x', [Thing::class, 'show']);\n";
        assert_eq!(
            http_frameworks(Language::Php, &[("routes/web.php", routes)]),
            ["laravel"]
        );
    }

    #[test]
    fn a_flask_route_has_its_methods_its_parameters_and_its_guards() {
        let source = "from flask import Flask\nfrom flask_login import login_required\n\napp = Flask(__name__)\n\n@app.route(\"/users/<int:user_id>\", methods=[\"POST\"])\n@login_required\ndef update(user_id):\n    return user_id\n";
        let found = entries(Language::Python, &[("app.py", source)]);
        assert_eq!(found.len(), 1);
        let entry = &found[0];
        assert_eq!(entry.kind, EntryKind::HttpHandler);
        assert_eq!(entry.framework, "flask");
        assert_eq!(
            entry.route,
            route("POST", "/users/<int:user_id>", &["user_id"])
        );
        assert_eq!(entry.guards, ["login_required"]);
        assert_eq!(entry.basis, EntryBasis::Declared);
        assert_eq!(entry.symbol.as_deref(), Some("app.py#update"));
        assert_eq!(entry.range.start_line, 6);
    }

    #[test]
    fn a_fastapi_verb_decorator_is_a_handler() {
        let source = "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get(\"/items/{item_id}\")\nasync def read(item_id: int):\n    return item_id\n";
        let found = entries(Language::Python, &[("main.py", source)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "fastapi");
        assert_eq!(
            found[0].route,
            route("GET", "/items/{item_id}", &["item_id"])
        );
    }

    #[test]
    fn a_decorator_of_a_name_that_no_framework_import_gives_is_not_a_handler() {
        let source = "app = Registry()\n\n@app.route(\"/x\")\ndef view():\n    return 1\n";
        assert_eq!(entries(Language::Python, &[("a.py", source)]), []);
    }

    #[test]
    fn a_django_path_registers_a_view() {
        let urls = "from django.urls import path\nfrom . import views\n\nurlpatterns = [\n    path(\"items/<int:pk>/\", views.detail),\n]\n";
        let views = "def detail(request, pk):\n    return pk\n";
        let found = entries(Language::Python, &[("urls.py", urls), ("views.py", views)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "django");
        assert_eq!(found[0].basis, EntryBasis::Registered);
        assert_eq!(found[0].symbol.as_deref(), Some("views.py#detail"));
        assert_eq!(found[0].route, route("ANY", "/items/<int:pk>/", &["pk"]));
    }

    #[test]
    fn a_python_main_guard_is_a_cli_entry() {
        let source = "def main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n";
        let found = entries(Language::Python, &[("tool.py", source)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, EntryKind::CliMain);
        assert_eq!(found[0].basis, EntryBasis::Convention);
    }

    #[test]
    fn a_framework_with_guards_that_omen_does_not_read_has_a_list_that_is_not_complete() {
        let express = "const express = require(\"express\");\nconst app = express();\n\nfunction show(req, res) {}\napp.get(\"/users/:id\", authenticate, show);\n";
        let found = entries(Language::JavaScript, &[("server.js", express)]);
        assert_eq!(found[0].guards, ["authenticate"]);
        assert!(!found[0].guards_complete);
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::LayerNotRead)
        );
        assert_eq!(
            found[0].guards_text(),
            "authenticate (incomplete: layer_not_read)"
        );
        let axum = "use axum::{routing::get, Router};\n\nasync fn list() {}\n\nfn app() -> Router {\n    Router::new().route(\"/items\", get(list)).layer(middleware::from_fn(auth::validate_header))\n}\n";
        let found: Vec<EntryPoint> = entries(Language::Rust, &[("main.rs", axum)])
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .collect();
        assert_eq!(found.len(), 1);
        assert!(found[0].guards.is_empty());
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::LayerNotRead)
        );
        assert_eq!(found[0].guards_text(), "unknown (layer_not_read)");
        let flask = "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/a\")\n@login_required\ndef a():\n    return 1\n";
        let found = entries(Language::Python, &[("app.py", flask)]);
        assert_eq!(found[0].guards, ["login_required"]);
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::LayerNotRead)
        );
    }

    #[test]
    fn an_entry_point_that_is_not_an_http_handler_has_a_complete_empty_list() {
        let source = "def main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n";
        let found = entries(Language::Python, &[("tool.py", source)]);
        assert!(found[0].guards.is_empty());
        assert!(found[0].guards_complete);
        assert_eq!(found[0].guards_unknown_reason, None);
        assert_eq!(found[0].guards_text(), "none declared");
    }

    #[test]
    fn a_go_middleware_that_is_not_a_name_makes_the_guards_not_complete() {
        let source = go_source(
            &[CHI],
            "func routes() http.Handler {\n\tr := chi.NewRouter()\n\tr.Use(logger)\n\tr.Get(\"/a\", list)\n\tr.Group(func(r chi.Router) {\n\t\tr.Use(chain[0])\n\t\tr.Get(\"/b\", list)\n\t})\n\treturn r\n}\n",
        );
        let found: Vec<EntryPoint> = entries(Language::Go, &[("main.go", source.as_str())])
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .collect();
        let state = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.guards.join(","), entry.guards_unknown_reason)
        };
        assert_eq!(state("/a"), ("logger".to_string(), None));
        assert_eq!(
            state("/b"),
            (
                "logger,<expression>".to_string(),
                Some(GuardsUnknownReason::UnreadMiddleware)
            )
        );
    }

    #[test]
    fn an_entry_point_of_an_older_version_reads_as_not_complete() {
        let old = serde_json::json!({
            "key": "a.py:1:0:http_handler", "file": "a.py", "kind": "http_handler",
            "range": {"start_line": 1, "start_column": 0, "end_line": 1, "end_column": 1,
                      "start_byte": 0, "end_byte": 1},
            "symbol": null, "framework": "flask", "route": null, "guards": [],
            "basis": "declared", "in_test": false
        });
        let entry: EntryPoint = serde_json::from_value(old).unwrap();
        assert!(!entry.guards_complete);
        assert_eq!(entry.guards_text(), "unknown (not known)");
    }

    #[test]
    fn an_express_route_has_its_middleware_as_guards() {
        let source = "const express = require(\"express\");\nconst app = express();\n\nfunction show(req, res) {}\napp.get(\"/users/:id\", authenticate, show);\n";
        let found = entries(Language::JavaScript, &[("server.js", source)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "express");
        assert_eq!(found[0].route, route("GET", "/users/:id", &["id"]));
        assert_eq!(found[0].guards, ["authenticate"]);
        assert_eq!(found[0].symbol.as_deref(), Some("server.js#show"));
    }

    #[test]
    fn a_next_route_file_exports_a_handler_for_each_verb() {
        let source = "export async function GET(request) {\n  return null;\n}\n\nexport async function POST(request) {\n  return null;\n}\n";
        let found = entries(
            Language::TypeScript,
            &[("app/api/users/[id]/route.ts", source)],
        );
        let routes: Vec<Option<Route>> = found.iter().map(|entry| entry.route.clone()).collect();
        assert_eq!(
            routes,
            [
                route("GET", "/api/users/{id}", &["id"]),
                route("POST", "/api/users/{id}", &["id"])
            ]
        );
        assert!(found.iter().all(|entry| entry.framework == "next"));
        assert!(found
            .iter()
            .all(|entry| entry.basis == EntryBasis::Convention));
    }

    #[test]
    fn a_spring_mapping_joins_the_class_path_and_has_its_guards() {
        let source = "@RestController\n@RequestMapping(\"/api\")\npublic class UserController {\n    @PreAuthorize(\"hasRole('ADMIN')\")\n    @GetMapping(\"/users/{id}\")\n    public String show(@PathVariable String id) {\n        return id;\n    }\n}\n";
        let found = entries(Language::Java, &[("UserController.java", source)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "spring");
        assert_eq!(found[0].route, route("GET", "/api/users/{id}", &["id"]));
        assert_eq!(found[0].guards, ["PreAuthorize"]);
    }

    #[test]
    fn a_spring_listener_and_a_scheduled_method_are_entries() {
        let source = "public class Jobs {\n    @KafkaListener(topics = \"orders\")\n    public void consume(String message) {}\n\n    @Scheduled(fixedRate = 1000)\n    public void tick() {}\n\n    public static void main(String[] arguments) {}\n}\n";
        let found = entries(Language::Java, &[("Jobs.java", source)]);
        let kinds: Vec<EntryKind> = found.iter().map(|entry| entry.kind).collect();
        assert_eq!(
            kinds,
            [
                EntryKind::MessageConsumer,
                EntryKind::ScheduledJob,
                EntryKind::CliMain
            ]
        );
    }

    #[test]
    fn an_aspnet_action_joins_the_controller_route_and_has_its_guards() {
        let source = "[ApiController]\n[Route(\"api/users\")]\npublic class UsersController {\n    [Authorize(Roles = \"Admin\")]\n    [HttpGet(\"{id}\")]\n    public string Show(string id) {\n        return id;\n    }\n}\n";
        let found = entries(Language::CSharp, &[("UsersController.cs", source)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "aspnet");
        assert_eq!(found[0].route, route("GET", "/api/users/{id}", &["id"]));
        assert_eq!(found[0].guards, ["Authorize"]);
    }

    fn go_routes(files: &[(&str, &str)]) -> Vec<String> {
        let keys = raw_entry_keys(Language::Go, files);
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(
            keys.len(),
            unique.len(),
            "each draft has its own key: {keys:?}"
        );
        entries(Language::Go, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| {
                let route = entry.route.as_ref().unwrap();
                format!(
                    "{} {} {} [{}] {}",
                    entry.framework,
                    route.method,
                    route.path_text(),
                    entry.guards.join(","),
                    entry
                        .symbol
                        .as_deref()
                        .map_or("-", |symbol| symbol.rsplit('#').next().unwrap())
                )
            })
            .collect()
    }

    const GO_HANDLERS: &str = r#"func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func auth(next http.Handler) http.Handler { return next }

func audit(next http.Handler) http.Handler { return next }

func authFunc(next http.HandlerFunc) http.HandlerFunc { return next }

func combine(first, second http.HandlerFunc) http.HandlerFunc { return first }

func onError(err error) {}

func makeHandler(callback func(error)) http.Handler { return nil }

"#;

    fn go_source(imports: &[&str], body: &str) -> String {
        let imports: String = imports.iter().map(|name| format!("\t{name}\n")).collect();
        format!("package main\n\nimport (\n\t\"net/http\"\n{imports})\n\n{GO_HANDLERS}{body}")
    }

    const CHI: &str = "\"github.com/go-chi/chi/v5\"";

    /// The Go routes of `source` on a thread with a small stack, and the same routes on a
    /// thread with a stack of 1 GiB.
    fn go_routes_on_small_and_large_stack(source: String) -> (Vec<String>, Vec<String>) {
        let copy = source.clone();
        let small =
            crate::parser::deep::on_small_stack(move || go_routes(&[("main.go", copy.as_str())]));
        let large = std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || go_routes(&[("main.go", source.as_str())]))
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        (small, large)
    }

    /// The nesting depth of the tests of the Go interpreter. In a debug build, the recursion of
    /// the interpreter without `stacker` overflows the stack of `on_small_stack` at a depth of 50.
    const INTERPRETER_DEPTH: usize = 200;

    fn interpreter_nest(open: &str, middle: &str, close: &str) -> String {
        crate::parser::deep::nest_to(INTERPRETER_DEPTH, open, middle, close)
    }

    /// `closure` reads a function literal inside each level of nested function literals.
    #[test]
    fn go_routes_inside_deeply_nested_closures() {
        let body = format!(
            "func routes() http.Handler {{\n\tr := chi.NewRouter()\n\t{}\n\treturn r\n}}\n",
            interpreter_nest("func() {\n", "r.Get(\"/x\", list)\n", "}()\n")
        );
        let (small, large) = go_routes_on_small_and_large_stack(go_source(&[CHI], &body));
        assert_eq!(small, large);
    }

    /// `chi_route` reads the callback of each level of nested `Route` calls.
    #[test]
    fn go_routes_inside_deeply_nested_chi_routes() {
        let body = format!(
            "func routes() http.Handler {{\n\tr := chi.NewRouter()\n\t{}\n\treturn r\n}}\n",
            interpreter_nest(
                "r.Route(\"/a\", func(r chi.Router) {\n",
                "r.Get(\"/x\", list)\n",
                "})\n"
            )
        );
        let (small, large) = go_routes_on_small_and_large_stack(go_source(&[CHI], &body));
        assert_eq!(small, large);
        assert!(
            small
                .iter()
                .any(|route| route.contains(&format!("{}/x", "/a".repeat(INTERPRETER_DEPTH)))),
            "{:?}",
            small.first()
        );
    }

    /// `chi_group` reads the callback of each level of nested `Group` calls.
    #[test]
    fn go_routes_inside_deeply_nested_chi_groups() {
        let body = format!(
            "func routes() http.Handler {{\n\tr := chi.NewRouter()\n\t{}\n\treturn r\n}}\n",
            interpreter_nest(
                "r.Group(func(r chi.Router) {\n",
                "r.Get(\"/x\", list)\n",
                "})\n"
            )
        );
        let (small, large) = go_routes_on_small_and_large_stack(go_source(&[CHI], &body));
        assert_eq!(small, large);
        assert!(!small.is_empty());
    }

    /// `eval`, `call_function`, and `walk_declared` read a chain of more factories than the
    /// limit of the evaluation.
    #[test]
    fn go_routes_from_a_chain_of_factories_longer_than_the_limit() {
        let count = super::go_routes::EVAL_DEPTH_LIMIT + 100;
        let mut body = String::new();
        for index in 0..count {
            body.push_str(&format!(
                "func f{index}() chi.Router {{ return f{}() }}\n",
                index + 1
            ));
        }
        body.push_str(&format!(
            "func f{count}() chi.Router {{ return chi.NewRouter() }}\n\nfunc routes() http.Handler {{\n\tr := f0()\n\tr.Get(\"/x\", list)\n\treturn r\n}}\n"
        ));
        let (small, large) = go_routes_on_small_and_large_stack(go_source(&[CHI], &body));
        assert_eq!(small, large);
    }
    const ECHO: &str = "\"github.com/labstack/echo/v4\"";
    const MUX: &str = "\"github.com/gorilla/mux\"";
    const GIN: &str = "\"github.com/gin-gonic/gin\"";

    #[test]
    fn chi_routes_join_prefixes_and_apply_use_to_later_routes() {
        let source = go_source(
            &[CHI],
            r#"func routes() http.Handler {
	r := chi.NewRouter()
	r.Use(logger)
	r.Get("/health", list)
	r.Route("/api", func(r chi.Router) {
		r.Use(auth)
		r.Get("/users/{id}", show)
		r.Group(func(r chi.Router) {
			r.Use(admin)
			r.Post("/users", list)
		})
		r.Put("/after", list)
	})
	r.With(audit).Delete("/x/{id:[0-9]+}", show)
	r.Method("PATCH", "/m", list)
	r.HandleFunc("/any", list)
	r.Handle("/handle", http.HandlerFunc(show))
	return r
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /health [logger] list",
                "chi GET /api/users/{id} [logger,auth] show",
                "chi POST /api/users [logger,auth,admin] list",
                "chi PUT /api/after [logger,auth] list",
                "chi DELETE /x/{id:[0-9]+} [logger,audit] show",
                "chi PATCH /m [logger] list",
                "chi ANY /any [logger] list",
                "chi ANY /handle [logger] show",
            ]
        );
        let found = entries(Language::Go, &[("main.go", &source)]);
        let delete = found
            .iter()
            .find(|entry| entry.route.as_ref().is_some_and(|r| r.method == "DELETE"))
            .unwrap();
        assert_eq!(delete.route.as_ref().unwrap().parameters, ["id"]);
    }

    #[test]
    fn a_chi_use_guards_the_routes_of_its_router_in_any_source_order() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("/before", show)
	r.Use(auth)
	r.Get("/after", show)
	root := chi.NewRouter()
	root.Route("/api", func(r chi.Router) {
		root.Use(audit)
		r.Get("/x", show)
	})
	g := chi.NewRouter()
	g.Get("/ok", list)
	g.Group(func(g chi.Router) {
		g.Use(auth)
		g.Get("/inner", show)
	})
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /before [auth] show",
                "chi GET /after [auth] show",
                "chi GET /api/x [audit] show",
                "chi GET /ok [] list",
                "chi GET /inner [auth] show",
            ]
        );
    }

    #[test]
    fn chi_route_returns_the_sub_router_and_query_is_a_method() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	root := chi.NewRouter()
	api := root.Route("/api", func(r chi.Router) {})
	api.Get("/x", show)
	root.Query("/search", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /api/x [] show", "chi QUERY /search [] show"]
        );
    }

    #[test]
    fn chi_mount_follows_a_router_of_the_same_file_only() {
        let source = go_source(
            &[CHI, "\"example.com/app/external\""],
            r#"func routes() http.Handler {
	r := chi.NewRouter()
	r.Use(logger)
	r.Mount("/sub", subRouter())
	r.Mount("/ext", external.Routes())
	inline := chi.NewMux()
	inline.Get("/", show)
	r.Mount("/inline", inline)
	return r
}

func subRouter() chi.Router {
	s := chi.NewRouter()
	s.Get("/", list)
	return s
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /inline/ [logger] show"]
        );
    }

    #[test]
    fn a_chi_router_mounted_at_two_paths_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	root := chi.NewRouter()
	sub := chi.NewRouter()
	sub.Get("/x", show)
	root.Mount("/a", sub)
	root.Mount("/b", sub)
	solo := chi.NewRouter()
	solo.Get("/y", show)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), ["chi GET /y [] show"]);
    }

    #[test]
    fn a_named_chi_route_callback_has_an_unknown_prefix_so_it_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"func api(r chi.Router) {
	r.Get("/users", show)
}

func unused(r chi.Router) {
	r.Get("/never", list)
}

func routes() {
	root := chi.NewRouter()
	root.Route("/api", api)
	root.Route("/v2", api)
	root.Route("/other", elsewhere.Routes)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn a_chi_pattern_without_a_leading_slash_is_not_a_route() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("bad", list)
	r.Route("/api/", func(r chi.Router) {
		r.Get("/users", show)
		r.Get("nope", show)
	})
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /api/users [] show"]
        );
    }

    #[test]
    fn a_chi_receiver_must_be_proven_through_the_import() {
        let aliased = go_source(
            &["routing \"github.com/go-chi/chi/v5\""],
            r#"func routes() {
	r := routing.NewRouter()
	r.Get("/a", list)
}

func typed(r routing.Router) {
	r.Get("/b", show)
}

func root(m *routing.Mux) {
	m.Get("/c", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &aliased)]),
            ["chi GET /a [] list", "chi GET /c [] show"]
        );
        let unproven = go_source(
            &[CHI],
            r#"type store struct{}

func (store) Get(path string, handler http.HandlerFunc) {}

func routes(r store, chi store) {
	r.Get("/a", list)
	chi.Get("/b", list)
	router := newRouter()
	router.Get("/c", list)
}
"#,
        );
        assert!(go_routes(&[("main.go", &unproven)]).is_empty());
        let no_import = go_source(
            &[],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("/a", list)
}
"#,
        );
        assert!(go_routes(&[("main.go", &no_import)]).is_empty());
    }

    #[test]
    fn echo_routes_join_group_prefixes_and_guards() {
        let source = go_source(
            &[ECHO, "\"github.com/labstack/echo/v4/middleware\""],
            r#"func routes() *echo.Echo {
	e := echo.New()
	e.GET("/health", list)
	e.Use(middleware.Logger())
	api := e.Group("/api", auth)
	api.GET("/users/:id", show, audit)
	admin := api.Group("/admin", root)
	admin.Use(extra)
	admin.POST("/users", list)
	api.PUT("/late", list)
	e.Match([]string{"GET", "POST"}, "/multi", list)
	e.Add("TRACE", "/add", list)
	return e
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "echo GET /health [middleware.Logger] list",
                "echo GET /api/users/:id [middleware.Logger,auth,audit] show",
                "echo POST /api/admin/users [middleware.Logger,auth,root,extra] list",
                "echo PUT /api/late [middleware.Logger,auth] list",
                "echo GET /multi [middleware.Logger] list",
                "echo POST /multi [middleware.Logger] list",
                "echo TRACE /add [middleware.Logger] list",
            ]
        );
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found[1].route.as_ref().unwrap().parameters, ["id"]);
    }

    #[test]
    fn an_echo_pre_middleware_guards_every_route() {
        let source = go_source(
            &[ECHO],
            r#"func routes() {
	e := echo.New()
	e.GET("/a", show)
	e.Use(logger)
	e.Pre(gate)
	g := e.Group("/g")
	g.GET("/b", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "echo GET /a [gate,logger] show",
                "echo GET /g/b [gate,logger] show"
            ]
        );
    }

    #[test]
    fn echo_joins_paths_by_plain_concatenation() {
        let source = go_source(
            &[ECHO],
            r#"func routes() {
	e := echo.New()
	g := e.Group("/api/")
	g.GET("/users", show)
	e.GET("plain", show)
	e.GET("", show)
	h := e.Group("api")
	h.GET("x", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "echo GET /api//users [] show",
                "echo GET /plain [] show",
                "echo GET / [] show",
                "echo GET /apix [] show",
            ]
        );
    }

    #[test]
    fn an_echo_receiver_must_be_proven_through_the_import() {
        let aliased = go_source(
            &["framework \"github.com/labstack/echo/v4\""],
            r#"func routes() {
	e := framework.New()
	e.GET("/a", list)
}

func typed(g *framework.Group) {
	g.POST("/b", show)
}

func root(e *framework.Echo) {
	e.POST("/c", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &aliased)]),
            ["echo GET /a [] list", "echo POST /c [] show"]
        );
        let unproven = go_source(
            &[ECHO],
            r#"type store struct{}

func (store) GET(path string, handler http.HandlerFunc) {}

func routes(e store) {
	e.GET("/a", list)
	g := e.Group("/x")
	g.GET("/b", list)
	x := echo.Echo{}
	x.GET("/c", list)
}
"#,
        );
        assert!(go_routes(&[("main.go", &unproven)]).is_empty());
    }

    #[test]
    fn gorilla_mux_routes_have_methods_prefixes_and_guards() {
        let source = go_source(
            &[MUX],
            r#"func routes() http.Handler {
	http.HandleFunc("/plain", list)
	r := mux.NewRouter()
	r.HandleFunc("/health", list)
	r.HandleFunc("/users/{id}", show).Methods("GET", "POST")
	r.HandleFunc("/items/{id:[0-9]+}", show).Methods("DELETE").Name("item")
	r.Handle("/handle", http.HandlerFunc(list))
	api := r.PathPrefix("/api").Subrouter()
	api.Use(auth)
	api.HandleFunc("/ping", list).Methods("GET")
	r.Use(logger)
	return r
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "net/http ANY /plain [] list",
                "gorilla/mux ANY /health [logger] list",
                "gorilla/mux GET /users/{id} [logger] show",
                "gorilla/mux POST /users/{id} [logger] show",
                "gorilla/mux DELETE /items/{id:[0-9]+} [logger] show",
                "gorilla/mux ANY /handle [logger] list",
                "gorilla/mux GET /api/ping [logger,auth] list",
            ]
        );
        let found = entries(Language::Go, &[("main.go", &source)]);
        let item = found
            .iter()
            .find(|entry| entry.route.as_ref().is_some_and(|r| r.method == "DELETE"))
            .unwrap();
        assert_eq!(item.route.as_ref().unwrap().parameters, ["id"]);
    }

    #[test]
    fn gorilla_mux_route_builders_register_routes() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.Methods("GET").Path("/x").HandlerFunc(show)
	r.Path("/y/{id}").Methods("POST", "PUT").Handler(http.HandlerFunc(list))
	r.PathPrefix("/static/").HandlerFunc(list)
	r.Methods("GET", "POST").Methods("POST", "PUT").Path("/i").HandlerFunc(list)
	r.Methods("GET").Methods("PUT").Path("/n").HandlerFunc(list)
	sub := r.PathPrefix("/api/").Subrouter()
	sub.Methods("GET").Path("/z").HandlerFunc(show)
	sub.HandleFunc("w", show)
	r.Use(auth)
	q := mux.NewRouter()
	q.Methods("GET").HandlerFunc(list)
	q.Methods(verbs...).Path("/s").HandlerFunc(list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux GET /x [auth] show",
                "gorilla/mux POST /y/{id} [auth] list",
                "gorilla/mux PUT /y/{id} [auth] list",
                "gorilla/mux ANY /static/ [auth] list",
                "gorilla/mux POST /i [auth] list",
                "gorilla/mux GET /api/z [auth] show",
            ]
        );
    }

    #[test]
    fn a_gorilla_mux_receiver_must_be_proven_through_the_import() {
        let aliased = go_source(
            &["gorilla \"github.com/gorilla/mux\""],
            r#"func routes() {
	r := gorilla.NewRouter()
	r.HandleFunc("/a", list).Methods("PUT")
}

func typed(s *gorilla.Router) {
	s.HandleFunc("/b", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &aliased)]),
            ["gorilla/mux PUT /a [] list"]
        );
        let unproven = go_source(
            &[MUX],
            r#"func routes(r thing) {
	r.HandleFunc("/a", list).Methods("GET")
	m := http.NewServeMux()
	m.HandleFunc("/b", list)
	n := mux.Vars(nil)
	n.HandleFunc("/c", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &unproven)]),
            ["net/http ANY /b [] list"]
        );
    }

    #[test]
    fn gin_groups_join_prefixes_and_use_gives_guards() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	r := gin.New()
	r.GET("/early", list)
	r.Use(logger)
	v1 := r.Group("/v1", auth)
	v1.GET("/users/:id", audit, show)
	inner := v1.Group("/inner")
	inner.Use(extra)
	inner.POST("/x", list)
	r.Group("/chain").DELETE("/c", list)
	v1.PUT("/late", list)
	r.GET("/after", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /early [] list",
                "gin GET /v1/users/:id [logger,auth,audit] show",
                "gin POST /v1/inner/x [logger,auth,extra] list",
                "gin DELETE /chain/c [logger] list",
                "gin PUT /v1/late [logger,auth] list",
                "gin GET /after [logger] list",
            ]
        );
    }

    #[test]
    fn a_gin_use_returns_the_router_so_calls_can_chain() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	gin.New().Use(auth).GET("/x", show)
	r := gin.New()
	r.Use(first).Use(second).GET("/y", show)
	r.GET("/z", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /x [auth] show",
                "gin GET /y [first,second] show",
                "gin GET /z [first,second] show",
            ]
        );
    }

    #[test]
    fn gin_joins_paths_as_the_library_does() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	r := gin.New()
	g := r.Group("/api/")
	g.GET("/users/", show)
	h := r.Group("/a//b")
	h.GET("/../c", show)
	r.GET("", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /api/users/ [] show",
                "gin GET /a/c [] show",
                "gin GET / [] show",
            ]
        );
    }

    #[test]
    fn gin_any_handle_and_match_register_routes() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	r := gin.New()
	r.Any("/a", show)
	r.Handle("PATCH", "/b", audit, show)
	r.Match([]string{"GET", "POST"}, "/d", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin CONNECT /a [] show",
                "gin DELETE /a [] show",
                "gin GET /a [] show",
                "gin HEAD /a [] show",
                "gin OPTIONS /a [] show",
                "gin PATCH /a [] show",
                "gin POST /a [] show",
                "gin PUT /a [] show",
                "gin TRACE /a [] show",
                "gin PATCH /b [audit] show",
                "gin GET /d [] show",
                "gin POST /d [] show",
            ]
        );
    }
    #[test]
    fn a_gin_route_call_returns_the_router_so_calls_can_chain() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	gin.New().GET("/a", show).POST("/b", list).Use(auth).PUT("/c", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /a [] show",
                "gin POST /b [] list",
                "gin PUT /c [auth] list",
            ]
        );
    }

    #[test]
    fn match_needs_a_literal_list_of_strings_to_prove_the_methods() {
        let source = go_source(
            &[GIN, "\"github.com/labstack/echo/v4\""],
            r#"func methods(name string) []string { return []string{"DELETE"} }

func routes() {
	g := gin.New()
	g.Match(methods("GET"), "/a", show)
	g.Match([]string{"GET", "PUT"}, "/b", show)
	verbs := []string{"GET"}
	g.Match(verbs, "/c", show)
	e := echo.New()
	e.Match(methods("GET"), "/d", show)
	e.Match([]string{"POST"}, "/e", show)
	e.Match([]string{"POST", name()}, "/f", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /b [] show",
                "gin PUT /b [] show",
                "echo POST /e [] show",
            ]
        );
    }

    #[test]
    fn echo_any_registers_each_method_of_the_library() {
        let source = go_source(
            &[ECHO],
            r#"func routes() {
	e := echo.New()
	e.Any("/a", show)
}
"#,
        );
        let methods: Vec<String> = go_routes(&[("main.go", &source)])
            .iter()
            .map(|line| line.split(' ').nth(1).unwrap().to_string())
            .collect();
        assert_eq!(
            methods,
            [
                "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PROPFIND", "PUT",
                "REPORT", "TRACE"
            ]
        );
    }

    #[test]
    fn a_router_that_two_parents_mount_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	sub := chi.NewRouter()
	sub.Get("/x", show)
	first := chi.NewRouter()
	first.Use(auth)
	first.Mount("/a", sub)
	first.Mount("/a", sub)
	once := chi.NewRouter()
	once.Get("/y", show)
	second := chi.NewRouter()
	second.Use(audit)
	second.Mount("/a", sub)
	second.Mount("/b", once)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /b/y [audit] show"]
        );
    }

    fn go_file(imports: &[&str], body: &str) -> String {
        let imports: String = imports.iter().map(|name| format!("\t{name}\n")).collect();
        format!("package main\n\nimport (\n{imports})\n\n{body}")
    }

    fn factory_file(name: &str, path: &str) -> String {
        go_source(
            &[CHI],
            &format!(
                "func {name}() *chi.Mux {{\n\tr := chi.NewRouter()\n\tr.Get(\"{path}\", list)\n\treturn r\n}}\n"
            ),
        )
    }

    #[test]
    fn a_call_in_another_file_that_mounts_a_factory_removes_its_routes() {
        let router = factory_file("NewRouter", "/x");
        let main = go_file(
            &[CHI],
            "func serve() {\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", NewRouter())\n}\n",
        );
        assert_eq!(
            go_routes(&[("router.go", &router), ("main.go", &main)]),
            Vec::<String>::new()
        );
    }
    #[test]
    fn a_named_result_with_a_bare_return_makes_a_factory() {
        let router = go_source(
            &[CHI],
            "func NewRouter() (r *chi.Mux) {\n\tr = chi.NewRouter()\n\tr.Get(\"/x\", list)\n\treturn\n}\n",
        );
        let main = go_file(
            &[CHI],
            "func serve() {\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", NewRouter())\n}\n",
        );
        assert_eq!(
            go_routes(&[("a_main.go", &main), ("b_router.go", &router)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_return_path_can_return_a_router_that_the_function_made() {
        let router = go_source(
            &[CHI],
            "func NewRouter(first bool) (*chi.Mux, error) {\n\ta := chi.NewRouter()\n\ta.Get(\"/a\", list)\n\tif first {\n\t\treturn a, nil\n\t}\n\tb := chi.NewRouter()\n\tb.Get(\"/b\", list)\n\treturn b, nil\n}\n",
        );
        let main = go_file(
            &[CHI],
            "func serve() {\n\troot := chi.NewRouter()\n\tr, _ := NewRouter(false)\n\troot.Mount(\"/api\", r)\n}\n",
        );
        assert_eq!(
            go_routes(&[("a_main.go", &main), ("b_router.go", &router)]),
            Vec::<String>::new()
        );
    }

    fn symbols_of(files: &[(&str, &str)]) -> Vec<Option<String>> {
        entries(Language::Go, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| entry.symbol)
            .collect()
    }

    #[test]
    fn a_qualified_handler_resolves_through_the_import_to_the_package_of_the_import() {
        let main = go_file(
            &["\"net/http\"", CHI, "\"example.com/app/dep\"", "\"other.com/x\""],
            "func Show(w http.ResponseWriter, r *http.Request) {}\n\nfunc routes() {\n\tr := chi.NewRouter()\n\tr.Get(\"/a\", dep.Show)\n\tr.Get(\"/b\", Show)\n\tr.Get(\"/c\", x.Show)\n}\n",
        );
        let dep = "package dep\n\nimport \"net/http\"\n\nfunc Show(w http.ResponseWriter, r *http.Request) {}\n";
        assert_eq!(
            symbols_of(&[("main.go", &main), ("dep/show.go", dep)]),
            [
                Some("dep/show.go#Show".to_string()),
                Some("main.go#Show".to_string()),
                None
            ]
        );
    }

    #[test]
    fn a_name_without_a_qualifier_resolves_in_the_same_package_only() {
        let main = go_file(
            &["\"net/http\"", CHI],
            "func routes() {\n\tr := chi.NewRouter()\n\tr.Get(\"/a\", Show)\n}\n",
        );
        let dep = "package dep\n\nimport \"net/http\"\n\nfunc Show(w http.ResponseWriter, r *http.Request) {}\n";
        assert_eq!(
            symbols_of(&[("main.go", &main), ("dep/show.go", dep)]),
            [None]
        );
    }

    #[test]
    fn a_caller_that_adds_middleware_to_the_result_of_a_factory_blocks_its_routes() {
        let source = go_source(
            &[CHI, ECHO, GIN, MUX],
            r#"func NewEcho() *echo.Echo {
	e := echo.New()
	e.GET("/e", show)
	return e
}

func NewGin() *gin.Engine {
	g := gin.New()
	g.GET("/g", show)
	return g
}

func NewChi() *chi.Mux {
	c := chi.NewRouter()
	c.Get("/c", show)
	return c
}

func NewMux() *mux.Router {
	m := mux.NewRouter()
	m.HandleFunc("/m", show)
	return m
}

func serve() {
	e := NewEcho()
	e.Use(auth)
	g := NewGin()
	g.Use(auth)
	c := NewChi()
	c.Use(auth)
	m := NewMux()
	m.Use(auth)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn a_handler_name_that_a_local_binding_shadows_gives_no_symbol() {
        let source = go_source(
            &[CHI],
            r#"func Show(w http.ResponseWriter, r *http.Request) {}

func real(w http.ResponseWriter, r *http.Request) {}

func byparameter(Show http.HandlerFunc) {
	r := chi.NewRouter()
	r.Get("/p", Show)
}

func routes() {
	Show := real
	r := chi.NewRouter()
	r.Get("/x", Show)
	http.HandleFunc("/n", Show)
	for _, Show := range []http.HandlerFunc{real} {
		r.Get("/y", Show)
	}
	func(Show http.HandlerFunc) {
		r.Get("/z", Show)
	}(real)
}

func outside() {
	r := chi.NewRouter()
	r.Get("/ok", Show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /p [] -",
                "chi GET /x [] -",
                "net/http ANY /n [] -",
                "chi GET /y [] -",
                "chi GET /z [] -",
                "chi GET /ok [] Show",
            ]
        );
    }

    #[test]
    fn a_wrapper_type_blocks_each_use_of_the_result_that_is_not_serving() {
        let server = go_source(
            &[CHI],
            "var held *Server\n\ntype Server struct{ *chi.Mux }\n\nfunc NewServer() *Server {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n\treturn &Server{Mux: r}\n}\n\nfunc (s *Server) Group() { held = s }\n",
        );
        let main = go_file(
            &[CHI],
            "func serve() {\n\ts := NewServer()\n\ts.Group()\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", held)\n}\n",
        );
        assert_eq!(
            go_routes(&[("a_main.go", &main), ("b_server.go", &server)]),
            Vec::<String>::new()
        );
    }

    fn go_parameters(files: &[(&str, &str)]) -> Vec<String> {
        entries(Language::Go, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| {
                let route = entry.route.unwrap();
                format!(
                    "{} {} {:?}",
                    route.method,
                    route.path_text(),
                    route.parameters
                )
            })
            .collect()
    }

    #[test]
    fn audit_chi_handle_splits_a_method_and_method_must_be_known() {
        let source = go_source(
            &[CHI],
            "func routes() {\n\tr := chi.NewRouter()\n\tr.Handle(\"get /a\", list)\n\tr.HandleFunc(\"POST\t /b\", list)\n\tr.Handle(\"/c\", list)\n\tr.Method(\"PURGE\", \"/d\", list)\n\tr.Method(\"get\", \"/e\", list)\n\tr.Handle(\"PURGE /f\", list)\n}\n",
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /a [] list",
                "chi POST /b [] list",
                "chi ANY /c [] list",
                "chi GET /e [] list",
            ]
        );
    }

    #[test]
    fn audit_each_framework_reads_the_parameters_of_its_own_syntax() {
        let source = go_source(
            &[CHI, ECHO, GIN, MUX],
            r#"func routes() {
	c := chi.NewRouter()
	c.Get("/users/:id", list)
	c.Get("/files/{name}.{ext:[a-z]{2}}/*", list)
	e := echo.New()
	e.GET("/a:id/b/{no}", list)
	e.GET(`/x/\:lit/*`, list)
	g := gin.New()
	g.GET("/u/:id/*rest", list)
	g.GET("/b/{no}", list)
	m := mux.NewRouter()
	m.HandleFunc("/m/{id:[0-9]{3}}/{slug}", list)
	m.HandleFunc("/n/:no", list)
}
"#,
        );
        assert_eq!(
            go_parameters(&[("main.go", &source)]),
            [
                "GET /users/:id []",
                "GET /files/{name}.{ext:[a-z]{2}}/* [\"name\", \"ext\", \"*\"]",
                "GET /a:id/b/{no} [\"id\"]",
                "GET /x/\\:lit/* [\"*\"]",
                "GET /u/:id/*rest [\"id\", \"rest\"]",
                "GET /b/{no} []",
                "ANY /m/{id:[0-9]{3}}/{slug} [\"id\", \"slug\"]",
                "ANY /n/:no []",
            ]
        );
    }

    #[test]
    fn audit_a_gorilla_route_that_is_build_only_never_matches() {
        let source = go_source(
            &[MUX],
            "func routes() {\n\tr := mux.NewRouter()\n\tr.HandleFunc(\"/a\", list).BuildOnly()\n\tr.HandleFunc(\"/b\", list).Name(\"b\")\n\tr.Methods(\"GET\").Path(\"/c\").HandlerFunc(list).BuildOnly()\n}\n",
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /b [] list"]
        );
    }

    #[test]
    fn audit_net_http_patterns_follow_the_servemux_syntax() {
        let source = go_source(
            &["\"example.com/custom\""],
            r#"func routes() {
	http.HandleFunc("GET example.com/x/{id}", list)
	http.HandleFunc("get /y", list)
	http.HandleFunc("/a/{rest...}", list)
	http.HandleFunc("GET  /w", list)
	http.HandleFunc("GET /../c", list)
	http.HandleFunc("/a/../b", list)
	http.HandleFunc("GET example.com", list)
	http.HandleFunc("FOO /q", list)
	http.HandleFunc("GET	/t", list)
	http.HandleFunc("/u/{$}", list)
	m := custom.NewServeMux()
	m.HandleFunc("/no", list)
	s := http.NewServeMux()
	s.HandleFunc("/yes", list)
}
"#,
        );
        assert_eq!(
            go_parameters(&[("main.go", &source)]),
            [
                "GET /x/{id} [\"id\"]",
                "get /y []",
                "ANY /a/{rest...} [\"rest\"]",
                "GET /w []",
                "FOO /q []",
                "GET /t []",
                "ANY /u/{$} []",
                "ANY /yes []",
            ]
        );
    }

    #[test]
    fn audit_middleware_that_the_walk_cannot_name_is_not_dropped() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	g := gin.New()
	g.Use(func(c *gin.Context) {})
	g.GET("/a", list)
	mws := []gin.HandlerFunc{}
	h := g.Group("/h", mws...)
	h.GET("/b", list)
	g.GET("/c", mws...)
	n := gin.New()
	n.Use(mws...)
	n.GET("/d", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gin GET /a [<function literal>] list"]
        );
    }

    #[test]
    fn a_value_of_a_type_that_wraps_a_servemux_is_not_a_multiplexer() {
        let source = go_source(
            &[],
            r#"type wrapped struct{ *http.ServeMux }

func (*wrapped) HandleFunc(string, http.HandlerFunc) {}

func routes() {
	m := &wrapped{ServeMux: http.NewServeMux()}
	m.HandleFunc("/ghost", list)
	s := http.NewServeMux()
	s.HandleFunc("/real", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["net/http ANY /real [] list"]
        );
    }

    #[test]
    fn a_wrapper_type_with_a_method_of_the_framework_gives_no_entry_for_its_factory() {
        let source = go_source(
            &[CHI],
            r#"type Server struct{ *chi.Mux }

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	http.NotFound(w, r)
}

func NewServer() *Server {
	r := chi.NewRouter()
	r.Get("/ghost", list)
	return &Server{Mux: r}
}

type Plain struct{ *chi.Mux }

func NewPlain() *Plain {
	r := chi.NewRouter()
	r.Get("/real", list)
	return &Plain{Mux: r}
}

type Hider struct{ *chi.Mux }

func (h *Hider) Get(path string, handler http.HandlerFunc) {}

func NewHider() *Hider {
	r := chi.NewRouter()
	r.Get("/hidden", list)
	return &Hider{Mux: r}
}

func serve() {
	http.ListenAndServe(":80", NewServer())
	http.ListenAndServe(":81", NewPlain())
	http.ListenAndServe(":82", NewHider())
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /real [] list"]
        );
    }

    #[test]
    fn a_path_that_a_go_string_literal_spells_is_decoded() {
        let source = go_source(
            &[GIN],
            r#"func routes() {
	r := gin.New()
	r.GET("/a/\x62", list)
	r.GET("/é", list)
	r.GET("/t\101", list)
	r.GET(`/raw\x62`, list)
	r.GET("/bad\q", list)
	r.GET("/\xff", list)
	r.GET("/q\"uote", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /a/b [] list",
                "gin GET /é [] list",
                "gin GET /tA [] list",
                "gin GET /raw\\x62 [] list",
                "gin GET /q\"uote [] list",
            ]
        );
    }

    #[test]
    fn a_gorilla_route_with_a_matcher_that_can_set_an_error_gives_no_entry() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.HandleFunc("/a", list).Headers("X")
	r.HandleFunc("/b", list).Headers("X", "y")
	r.HandleFunc("/c/{id:[}", list)
	r.HandleFunc("/d/{a}/{a}", list)
	r.HandleFunc("/e/{id", list)
	r.HandleFunc("/f/{}", list)
	r.HandleFunc("/g", list).Name("x").Name("y")
	r.HandleFunc("/j/{id:[0-9]+}", list)
	r.HandleFunc("/h", list).Queries("a", "{b}")
	r.HandleFunc("/i", list).Host("example.com")
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /b [] list",
                "gorilla/mux ANY /d/{a}/{a} [] list",
                "gorilla/mux ANY /j/{id:[0-9]+} [] list"
            ]
        );
    }

    #[test]
    fn a_binding_hides_a_name_only_in_its_block_after_its_declaration() {
        let source = go_source(
            &[CHI],
            r#"func Show(w http.ResponseWriter, r *http.Request) {}

func real(w http.ResponseWriter, r *http.Request) {}

func routes() {
	{
		Show := real
		_ = Show
	}
	r := chi.NewRouter()
	r.Get("/x", Show)
	if Show := real; Show != nil {
		r.Get("/y", Show)
	}
	r.Get("/z", Show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /x [] Show",
                "chi GET /y [] -",
                "chi GET /z [] Show"
            ]
        );
    }

    #[test]
    fn a_gorilla_pattern_outside_the_common_regular_expression_grammar_gives_no_entry() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.HandleFunc("/a/{id:(?x)a}", list)
	r.HandleFunc("/b/{id:a{1001}}", list)
	r.HandleFunc("/c/{id:[0-9]+}", list)
	r.HandleFunc("/d/{id:[a-z]{2,5}?}", list)
	r.HandleFunc("/e/{id:\\d+}", list)
	r.HandleFunc("/f/{id:a|b}", list)
	r.HandleFunc("/g/{id:(a)}", list)
	r.HandleFunc("/h/{id:a**}", list)
	r.HandleFunc("/i/{id:.*}", list)
	r.HandleFunc("/j/{id:[[:alpha:]]}", list)
	r.HandleFunc("/k/{id:\\.}", list)
	r.HandleFunc("/l/{id:[z-a]}", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /c/{id:[0-9]+} [] list",
                "gorilla/mux ANY /d/{id:[a-z]{2,5}?} [] list",
                "gorilla/mux ANY /f/{id:a|b} [] list",
                "gorilla/mux ANY /i/{id:.*} [] list",
                "gorilla/mux ANY /k/{id:\\.} [] list",
            ]
        );
    }

    #[test]
    fn a_selector_handler_needs_an_import_or_a_resolved_selector() {
        let source = go_source(
            &[CHI],
            r#"func Show(w http.ResponseWriter, r *http.Request) {}

func real(w http.ResponseWriter, r *http.Request) {}

var box = struct {
	Show http.HandlerFunc
}{Show: real}

func routes() {
	r := chi.NewRouter()
	r.Get("/x", box.Show)
	r.Get("/y", Show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /x [] -", "chi GET /y [] Show"]
        );
    }

    #[test]
    fn a_generic_wrapper_type_with_a_serve_method_gives_no_entry_for_its_factory() {
        let source = go_source(
            &[CHI],
            r#"type Server[T any] struct{ *chi.Mux }

func (s *Server[T]) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	http.NotFound(w, r)
}

func NewServer() *Server[int] {
	r := chi.NewRouter()
	r.Get("/ghost", list)
	return &Server[int]{Mux: r}
}

type Box[T any] struct{ *chi.Mux }

func NewBox() *Box[int] {
	r := chi.NewRouter()
	r.Get("/real", list)
	return &Box[int]{Mux: r}
}

func serve() {
	http.ListenAndServe(":80", NewServer())
	http.ListenAndServe(":81", NewBox())
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /real [] list"]
        );
    }

    #[test]
    fn a_parameter_name_is_kept_as_the_framework_reads_it() {
        let source = go_source(
            &[MUX, CHI],
            r#"func routes() {
	m := mux.NewRouter()
	m.HandleFunc("/{ user }", list)
	c := chi.NewRouter()
	c.Get("/{ id :[0-9]+}", list)
}
"#,
        );
        assert_eq!(
            go_parameters(&[("main.go", &source)]),
            [
                "ANY /{ user } [\" user \"]",
                "GET /{ id :[0-9]+} [\" id \"]"
            ]
        );
    }

    #[test]
    fn a_local_name_that_hides_a_package_qualifier_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"type fakeHTTP struct{}

type fakeMux struct{}

func (fakeHTTP) NewServeMux() *fakeMux { return &fakeMux{} }

func (fakeHTTP) HandlerFunc(any) http.HandlerFunc { return list }

func (*fakeMux) HandleFunc(string, http.HandlerFunc) {}

func wrong(int) {}

func hidden() {
	http := fakeHTTP{}
	m := http.NewServeMux()
	m.HandleFunc("/ghost", list)
	http.HandleFunc("/also-ghost", list)
	r := chi.NewRouter()
	r.Get("/x", http.HandlerFunc(wrong))
}

func ranged() {
	for _, chi := range []fakeHTTP{{}} {
		r := chi.NewRouter()
		r.Get("/ghost", list)
	}
}

func visible() {
	s := http.NewServeMux()
	s.HandleFunc("/real", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /x [] -", "net/http ANY /real [] list"]
        );
    }

    #[test]
    fn a_type_alias_is_resolved_before_the_wrapper_type_is_compared() {
        let source = go_source(
            &[CHI, "\"example.com/other\""],
            r#"type Server struct{ *chi.Mux }

func (*Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	http.NotFound(w, r)
}

type Alias = Server

type Chain = Alias

type Plain struct{ *chi.Mux }

type Same = Plain

type Far = other.Thing

func NewAlias() *Alias {
	r := chi.NewRouter()
	r.Get("/ghost-a", list)
	return &Alias{Mux: r}
}

func NewChain() *Chain {
	r := chi.NewRouter()
	r.Get("/ghost-b", list)
	return &Chain{Mux: r}
}

func NewSame() *Same {
	r := chi.NewRouter()
	r.Get("/real", list)
	return &Same{Mux: r}
}

func NewFar() *Far {
	r := chi.NewRouter()
	r.Get("/ghost-c", list)
	return &Far{Mux: r}
}

func serve() {
	http.ListenAndServe(":80", NewAlias())
	http.ListenAndServe(":81", NewChain())
	http.ListenAndServe(":82", NewSame())
	http.ListenAndServe(":83", NewFar())
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /real [] list"]
        );
    }

    #[test]
    fn a_type_alias_belongs_to_its_package() {
        let server = "package a\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n\ntype Base struct{ *chi.Mux }\n\nfunc (*Base) ServeHTTP(w http.ResponseWriter, r *http.Request) {\n\thttp.NotFound(w, r)\n}\n\ntype Alias = Base\n\nfunc show(http.ResponseWriter, *http.Request) {}\n\nfunc New() *Alias {\n\tr := chi.NewRouter()\n\tr.Get(\"/ghost\", show)\n\treturn &Alias{Mux: r}\n}\n\nfunc Start() {\n\thttp.ListenAndServe(\":8080\", New())\n}\n";
        let other = "package z\n\ntype Other struct{}\n\ntype Alias = Other\n";
        let plain = "package b\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n\ntype Base struct{ *chi.Mux }\n\ntype Alias = Base\n\nfunc show(http.ResponseWriter, *http.Request) {}\n\nfunc New() *Alias {\n\tr := chi.NewRouter()\n\tr.Get(\"/real\", show)\n\treturn &Alias{Mux: r}\n}\n\nfunc Start() {\n\thttp.ListenAndServe(\":8080\", New())\n}\n";
        let twice = "package c\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n\ntype Base struct{ *chi.Mux }\n\ntype Alias = Base\n\nfunc show(http.ResponseWriter, *http.Request) {}\n\nfunc New() *Alias {\n\tr := chi.NewRouter()\n\tr.Get(\"/twice\", show)\n\treturn &Alias{Mux: r}\n}\n\nfunc Start() {\n\thttp.ListenAndServe(\":8080\", New())\n}\n";
        let twice_again = "package c\n\ntype Alias = Elsewhere\n";
        assert_eq!(
            go_routes(&[
                ("a/server.go", server),
                ("z/other.go", other),
                ("b/plain.go", plain),
                ("c/twice.go", twice),
                ("c/again.go", twice_again),
            ]),
            ["chi GET /real [] show"]
        );
    }

    #[test]
    fn a_route_in_a_branch_that_never_runs_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"func routes(enabled bool) {
	r := chi.NewRouter()
	if false {
		r.Get("/ghost-a", list)
	}
	if true {
		r.Get("/real-a", list)
	} else {
		r.Get("/ghost-b", list)
	}
	for false {
		r.Get("/ghost-c", list)
	}
	if enabled {
		r.Get("/real-b", list)
	} else {
		r.Get("/real-c", list)
	}
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /real-a [] list",
                "chi GET /real-b [] list",
                "chi GET /real-c [] list"
            ]
        );
    }

    #[test]
    fn a_registration_that_a_later_one_replaces_or_hides_gives_no_entry() {
        let source = go_source(
            &[CHI, ECHO, MUX],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("/x", list)
	r.Get("/x", show)
	r.With(audit).Get("/x", list)
	r.Handle("/y", list)
	r.Get("/y", show)
	r.Get("/z", list)
	r.Handle("/z", show)
	r.Route("/p", func(r chi.Router) {
		r.Get("/x", list)
	})
	e := echo.New()
	e.GET("/e/:id", list)
	e.GET("/e/:name", show)
	m := mux.NewRouter()
	m.HandleFunc("/m", list)
	m.HandleFunc("/m", show)
	m.HandleFunc("/n", list).Methods("GET")
	m.HandleFunc("/n", show).Methods("GET", "POST")
	m.HandleFunc("/o", list).Headers("X", "y")
	m.HandleFunc("/o", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /x [audit] list",
                "chi GET /y [] show",
                "chi ANY /z [] show",
                "chi GET /p/x [] list",
                "echo GET /e/:name [] show",
                "gorilla/mux ANY /m [] list",
                "gorilla/mux GET /n [] list",
                "gorilla/mux POST /n [] show",
                "gorilla/mux ANY /o [] list",
            ]
        );
    }

    #[test]
    fn a_gorilla_route_name_follows_the_current_name_of_the_route() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.HandleFunc("/a", list).Name("").Name("x")
	r.HandleFunc("/b", list).Name("x").Name("y")
	r.HandleFunc("/c", list).Name("x").Name("")
	r.HandleFunc("/d", list).Name("x")
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /a [] list", "gorilla/mux ANY /d [] list"]
        );
    }

    #[test]
    fn a_name_that_another_file_of_the_package_declares_hides_the_constant() {
        let main = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	if true {
		r.Get("/ghost", list)
	} else {
		r.Get("/other", list)
	}
}
"#,
        );
        let flags = "package main\n\nconst true = false\n";
        assert_eq!(
            go_routes(&[("flags.go", flags), ("main.go", &main)]),
            ["chi GET /ghost [] list", "chi GET /other [] list"]
        );
        let elsewhere = "package flags\n\nconst true = false\n";
        assert_eq!(
            go_routes(&[("flags/flags.go", elsewhere), ("main.go", &main)]),
            ["chi GET /ghost [] list"]
        );
        let other_package = "package other\n\nconst true = false\n";
        assert_eq!(
            go_routes(&[("other.go", other_package), ("main.go", &main)]),
            ["chi GET /ghost [] list"]
        );
    }

    #[test]
    fn chi_replaces_a_handler_when_only_the_parameter_name_differs() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("/a/{id}", list)
	r.Get("/a/{name}", show)
	r.Get("/b/{id:[0-9]+}", list)
	r.Get("/b/{key:[0-9]+}", show)
	r.Get("/c/{id}", list)
	r.Get("/c/{id:[0-9]+}", show)
	r.Get("/d/{x}/e", list)
	r.Get("/d/{y}/f", show)
	r.Get("/g/*", list)
	r.Get("/g/*", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /a/{name} [] show",
                "chi GET /b/{key:[0-9]+} [] show",
                "chi GET /c/{id:[0-9]+} [] show",
                "chi GET /d/{x}/e [] list",
                "chi GET /d/{y}/f [] show",
                "chi GET /g/* [] show",
            ]
        );
    }

    #[test]
    fn an_earlier_gorilla_route_that_can_match_hides_a_later_route() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.PathPrefix("/api/").HandlerFunc(list)
	r.Path("/api/x").HandlerFunc(show)
	r.Path("/b/x").HandlerFunc(show)
	r.PathPrefix("/b/").HandlerFunc(list)
	r.HandleFunc("/u/me", show)
	r.HandleFunc("/u/{id}", list)
	r.HandleFunc("/v/{id}", list)
	r.HandleFunc("/v/me", show)
	r.HandleFunc("/w/x", list).Headers("X", "y")
	r.HandleFunc("/w/x", show).Methods("GET")
	r.HandleFunc("/z/{id:[a-b]}", list)
	r.HandleFunc("/z/a", show)
	r.HandleFunc("/q/a", show)
	r.HandleFunc("/q/b", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /api/ [] list",
                "gorilla/mux ANY /b/x [] show",
                "gorilla/mux ANY /b/ [] list",
                "gorilla/mux ANY /u/me [] show",
                "gorilla/mux ANY /u/{id} [] list",
                "gorilla/mux ANY /v/{id} [] list",
                "gorilla/mux ANY /w/x [] list",
                "gorilla/mux ANY /z/{id:[a-b]} [] list",
                "gorilla/mux ANY /q/a [] show",
                "gorilla/mux ANY /q/b [] list",
            ]
        );
    }

    #[test]
    fn a_gorilla_registration_that_omen_cannot_read_hides_the_routes_after_it() {
        let source = go_source(
            &[MUX],
            r#"func routes(path string) {
	r := mux.NewRouter()
	r.HandleFunc("/before", list)
	r.HandleFunc(path, show)
	r.HandleFunc("/after", list)
	s := mux.NewRouter()
	s.HandleFunc("/before", list)
	s.Path("/p").Host("example.com").HandlerFunc(show)
	s.HandleFunc("/after", list)
	t := mux.NewRouter()
	t.HandleFunc("/before", list)
	route := t.Path("/p")
	route.HandlerFunc(show)
	t.HandleFunc("/after", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /before [] list",
                "gorilla/mux ANY /before [] list",
                "gorilla/mux ANY /before [] list",
            ]
        );
    }

    #[test]
    fn a_gorilla_sub_router_route_can_be_hidden_by_a_route_of_the_parent() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.PathPrefix("/api").HandlerFunc(list)
	sub := r.PathPrefix("/api").Subrouter()
	sub.HandleFunc("/x", show)
	other := r.PathPrefix("/other").Subrouter()
	other.HandleFunc("/y", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /api [] list",
                "gorilla/mux ANY /other/y [] show"
            ]
        );
    }

    #[test]
    fn a_gorilla_chain_may_set_build_vars_and_needs_a_path() {
        let source = go_source(
            &[MUX],
            r#"func vars(values map[string]string) map[string]string { return values }

func routes() {
	r := mux.NewRouter()
	r.Path("/x").BuildVarsFunc(vars).HandlerFunc(show)
	r.Methods("GET").BuildVarsFunc(vars).Path("/y").HandlerFunc(list)
	r.Methods("GET").HandlerFunc(list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /x [] show", "gorilla/mux GET /y [] list"]
        );
    }

    #[test]
    fn a_gorilla_chain_with_two_path_matchers_gives_no_entry() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.Path("/ok").HandlerFunc(list)
	r.Path("/a").Path("/b").HandlerFunc(show)
	r.PathPrefix("/c").PathPrefix("/d").HandlerFunc(show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /ok [] list"]
        );
    }

    #[test]
    fn an_empty_gorilla_prefix_matches_every_path() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.PathPrefix("").HandlerFunc(list)
	r.Path("/x").HandlerFunc(show)
	s := mux.NewRouter()
	api := s.PathPrefix("/api").Subrouter()
	api.PathPrefix("").HandlerFunc(list)
	api.Path("/x").HandlerFunc(show)
	s.Path("/other").HandlerFunc(show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /api [] list",
                "gorilla/mux ANY /other [] show"
            ]
        );
    }

    #[test]
    fn a_gorilla_matcher_without_values_gives_no_entry() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.Path("/c").Schemes("https").HandlerFunc(list)
	r.Path("/a").Schemes().HandlerFunc(show)
	r.Path("/b").Methods().HandlerFunc(show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /c [] list"]
        );
    }

    #[test]
    fn a_chi_route_on_a_mount_key_replaces_the_mounted_routes() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	sub := chi.NewRouter()
	sub.Get("/a", list)
	root := chi.NewRouter()
	root.Mount("/x", sub)
	root.Get("/x/*", show)

	other := chi.NewRouter()
	other.Get("/b", list)
	root.Mount("/y", other)
	root.Get("/y/z", show)

	root.Route("/w", func(r chi.Router) { r.Get("/d", list) })
	root.Get("/w/", show)

	fourth := chi.NewRouter()
	fourth.Get("/e", list)
	root.Mount("/v/", fourth)
	root.Get("/v/*", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /x/* [] show",
                "chi GET /y/z [] show",
                "chi GET /w/ [] show",
                "chi GET /v/* [] show",
            ]
        );
    }
    #[test]
    fn a_chi_node_that_has_a_higher_child_hides_the_lower_children() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	r.Get("/a/{id:.+}", list)
	r.Get("/a/{name}", show)
	r.Get("/b/{id}", list)
	r.Get("/b/*", show)
	r.Get("/c/{id:[0-9]+}", list)
	r.Get("/c/{key:[a-z]+}", show)
	r.Get("/d/{id}", list)
	r.Get("/d/new", show)
	r.Get("/e/{id}", list)
	r.Get("/f/{id:[0-9]+}", show)
	r.Get("/g/{id:[0-9]+}/x", list)
	r.Get("/g/{id:[0-9]+}/{name}", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /a/{id:.+} [] list",
                "chi GET /b/{id} [] list",
                "chi GET /d/{id} [] list",
                "chi GET /d/new [] show",
                "chi GET /e/{id} [] list",
                "chi GET /f/{id:[0-9]+} [] show",
                "chi GET /g/{id:[0-9]+}/x [] list",
                "chi GET /g/{id:[0-9]+}/{name} [] show",
            ]
        );
    }

    #[test]
    fn a_gorilla_scheme_or_method_must_be_a_value_that_a_request_can_have() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.Path("/ok").Schemes("HTTPS").HandlerFunc(list)
	r.Path("/ok2").Methods("get").HandlerFunc(list)
	r.Path("/a").Schemes("").HandlerFunc(show)
	r.Path("/b").Schemes("ftp").HandlerFunc(show)
	r.Path("/c").Methods("").HandlerFunc(show)
	r.Path("/d").Methods("GE T").HandlerFunc(show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /ok [] list",
                "gorilla/mux GET /ok2 [] list"
            ]
        );
    }

    #[test]
    fn a_chi_route_that_omen_omits_still_hides_the_lower_routes() {
        let source = go_source(
            &[CHI],
            r#"func routes(path string) {
	mw := []func(http.Handler) http.Handler{}
	r := chi.NewRouter()
	r.With(mw...).Get("/a/{id:.+}", list)
	r.Get("/a/{name}", show)
	r.Get("/b/{id}", show)
	s := chi.NewRouter()
	s.Get(path, list)
	s.Get("/c", show)
	sub := chi.NewRouter()
	sub.Get("/d", show)
	s.Mount("/m", sub)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /b/{id} [] show"]
        );
    }

    #[test]
    fn an_echo_registration_that_omen_cannot_read_hides_the_routes_of_its_tree() {
        let source = go_source(
            &[ECHO],
            r#"func routes(path string, mw ...echo.MiddlewareFunc) {
	e := echo.New()
	e.GET("/a", list)
	e.GET("/b", list)
	e.GET("/b", show, mw...)
	f := echo.New()
	f.GET("/c", list)
	f.GET(path, show)
	g := echo.New()
	g.GET("/d", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["echo GET /a [] list", "echo GET /d [] list"]
        );
    }

    #[test]
    fn a_route_of_the_parent_below_a_chi_mount_hides_the_mounted_routes() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	sub := chi.NewRouter()
	sub.Get("/x", list)
	inner := chi.NewRouter()
	inner.Get("/z", list)
	sub.Mount("/in", inner)
	root := chi.NewRouter()
	root.Mount("/api", sub)
	root.Get("/api/x", show)
	other := chi.NewRouter()
	other.Get("/y", list)
	root.Mount("/other", other)
	root.Get("/outside", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /api/x [] show",
                "chi GET /other/y [] list",
                "chi GET /outside [] show"
            ]
        );
    }

    #[test]
    fn an_echo_registration_with_an_unread_method_replaces_the_routes_of_its_path_for_every_method()
    {
        let source = go_source(
            &[ECHO],
            r#"func routes(method string, methods []string) {
	e := echo.New()
	e.Add("FOO", "/x", list)
	e.Add(method, "/x", show)
	e.Add("GET", "/y", list)
	e.Match(methods, "/y", show)
	e.Add("POST", "/z", list)
	e.Add(method, "/w", show)
	e.Add("BAR", "/w", list)
	e.Add("GET", "/other", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "echo POST /z [] list",
                "echo BAR /w [] list",
                "echo GET /other [] list"
            ]
        );
    }
    #[test]
    fn an_echo_wildcard_that_has_a_parameter_sibling_gives_no_entry() {
        let source = go_source(
            &[ECHO],
            r#"func routes() {
	e := echo.New()
	e.GET("/x/", list)
	e.GET("/x/:id", show)
	e.GET("/x/*", list)
	e.GET("/y/*", list)
	e.GET("/z/:id/w", show)
	e.GET("/z/*", list)
	g := e.Group("/g")
	g.GET("/:id", show)
	e.GET("/g/*", list)
	e.GET("/h/:id", list)
	e.GET("/h/new", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "echo GET /x/ [] list",
                "echo GET /x/:id [] show",
                "echo GET /y/* [] list",
                "echo GET /z/:id/w [] show",
                "echo GET /g/:id [] show",
                "echo GET /h/:id [] list",
                "echo GET /h/new [] show",
            ]
        );
    }

    #[test]
    fn a_router_that_a_call_gets_can_get_routes_that_omen_does_not_read() {
        let source = go_source(
            &[ECHO, CHI, MUX],
            r#"func api(e *echo.Echo) { e.GET("/x", list) }
func unused(e *echo.Echo) { e.GET("/u", list) }
func chiApi(r chi.Router) { r.Get("/x", list) }
func muxApi(r *mux.Router) { r.HandleFunc("/x", list) }

func routes() {
	e := echo.New()
	e.GET("/x", show)
	api(e)

	r := chi.NewRouter()
	r.Get("/x", show)
	chiApi(r)

	add := func(r chi.Router) { r.Get("/c", list) }
	s := chi.NewRouter()
	s.Get("/s", show)
	add(s)

	m := mux.NewRouter()
	m.HandleFunc("/before", show)
	muxApi(m)
	m.HandleFunc("/x", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["echo GET /u [] list", "gorilla/mux ANY /before [] show"]
        );
    }

    #[test]
    fn echo_file_and_static_calls_replace_the_routes_of_their_paths() {
        let source = go_source(
            &[ECHO],
            r#"func routes(h echo.HandlerFunc) {
	e := echo.New()
	e.GET("/robots.txt", show)
	e.File("/robots.txt", "public/robots.txt")
	e.GET("/assets/*", show)
	e.Static("/assets/", "public")
	g := e.Group("/g")
	g.GET("/f", show)
	g.File("/f", "x")
	e.GET("/ok", list)
	f := echo.New()
	f.GET("/a", show)
	f.Router().Add("GET", "/a", h)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), ["echo GET /ok [] list"]);
    }

    #[test]
    fn an_echo_wildcard_in_the_middle_of_a_path_takes_the_node_of_the_wildcard() {
        let source = go_source(
            &[ECHO],
            r#"func routes() {
	e := echo.New()
	e.GET("/x/*", list)
	e.GET("/x/*/y", show)
	e.GET("/z/*/y", show)
	e.GET("/z/*", list)
	e.GET("/f", show)
	e.Add("echo_route_not_found", "/f", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["echo GET /z/* [] list", "echo GET /f [] show"]
        );
    }

    #[test]
    fn a_net_http_pattern_that_other_patterns_of_the_file_cover_gives_no_entry() {
        let source = go_source(
            &["\"net/http\""],
            r#"func routes() {
	mux := http.NewServeMux()
	mux.HandleFunc("/x/{a...}", list)
	mux.HandleFunc("/x/{$}", show)
	mux.HandleFunc("/x/{a}", show)
	mux.HandleFunc("/y/{a...}", list)
	mux.HandleFunc("/y/{$}", show)
}
"#,
        );
        let found = go_routes(&[("main.go", &source)]);
        assert!(
            !found.iter().any(|line| line.ends_with("/x/{a...} [] list")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|line| line.ends_with("/y/{a...} [] list")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|line| line.ends_with("/x/{a} [] show")),
            "{found:?}"
        );
    }

    #[test]
    fn a_chi_mount_replaces_the_routes_of_its_keys_for_all_methods() {
        let source = go_source(
            &[CHI],
            r#"func api(r chi.Router) { r.Get("/y", list) }

func routes() {
	r := chi.NewRouter()
	r.Get("/static", show)
	r.Get("/static/", list)
	r.Mount("/static", http.FileServer(http.Dir(".")))
	sub := chi.NewRouter()
	sub.Get("/y", list)
	r.Get("/api", show)
	r.Mount("/api", sub)
	r.Get("/rt", show)
	r.Route("/rt", api)
	r.Get("/keep", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /api/y [] list", "chi GET /keep [] show"]
        );
    }

    #[test]
    fn a_caller_that_registers_on_the_result_of_a_factory_shares_one_order_with_it() {
        let chi = go_source(
            &[CHI],
            r#"func newAPI() *chi.Mux { r := chi.NewRouter(); r.Get("/x", show); return r }

func main() {
	r := newAPI()
	r.Get("/x", list)
	http.ListenAndServe(":1", r)
}
"#,
        );
        // chi keeps the last registration of a key, and the registrations of both are in order.
        assert_eq!(go_routes(&[("main.go", &chi)]), ["chi GET /x [] list"]);
        let api = go_source(
            &[CHI],
            "func newAPI() *chi.Mux { r := chi.NewRouter(); r.Get(\"/x\", show); return r }\n",
        );
        let main = go_file(
            &["\"net/http\"", CHI],
            "func main() {\n\tr := newAPI()\n\tr.Get(\"/x\", list)\n\thttp.ListenAndServe(\":1\", r)\n}\n",
        );
        assert_eq!(
            go_routes(&[("api.go", &api), ("main.go", &main)]),
            Vec::<String>::new()
        );
        let mux = go_source(
            &[MUX],
            r#"func newAPI() *mux.Router { r := mux.NewRouter(); r.HandleFunc("/x", show); return r }

func main() {
	r := newAPI()
	r.HandleFunc("/x", list)
	http.ListenAndServe(":1", r)
}
"#,
        );
        // gorilla/mux takes the first route that matches.
        assert_eq!(
            go_routes(&[("main.go", &mux)]),
            ["gorilla/mux ANY /x [] show"]
        );
        let echo = go_source(
            &[ECHO],
            r#"func newAPI() *echo.Echo { e := echo.New(); e.GET("/x", show); e.GET("/keep", show); return e }

func main() {
	e := newAPI()
	e.GET("/x", list)
	e.GET("/other", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &echo)]),
            [
                "echo GET /keep [] show",
                "echo GET /x [] list",
                "echo GET /other [] list"
            ]
        );
        let twice = go_source(
            &[CHI],
            r#"func newAPI() *chi.Mux { r := chi.NewRouter(); r.Get("/x", show); return r }

func main() {
	r := newAPI()
	r.Get("/y", list)
	s := newAPI()
	_ = s
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &twice)]), Vec::<String>::new());
    }

    #[test]
    fn a_chi_child_of_an_inline_router_keeps_the_guards_of_its_creation() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	r := chi.NewRouter()
	r.Group(func(g chi.Router) {
		admin := g.With(audit)
		g.Use(auth)
		admin.Get("/x", show)
	})
	h := r.With(audit)
	sub := h.With()
	h.Use(auth)
	sub.Get("/y", show)
	r.Use(auth)
	r.Get("/z", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /x [auth,audit] show",
                "chi GET /y [auth,audit] show",
                "chi GET /z [auth] show"
            ]
        );
    }

    #[test]
    fn a_route_on_a_router_that_omen_marks_partial_still_replaces_the_earlier_routes() {
        let source = go_source(
            &[CHI, ECHO],
            r#"func routes(mws []func(http.Handler) http.Handler, ems []echo.MiddlewareFunc) {
	r := chi.NewRouter()
	r.Get("/x", show)
	r.With(mws...).Get("/x", list)
	k := chi.NewRouter()
	k.Get("/keep", show)
	e := echo.New()
	e.GET("/g/x", show)
	g := e.Group("/g", ems...)
	g.GET("/x", list)
	f := echo.New()
	f.GET("/g/x", show)
	h := f.Group("/g")
	h.Use(ems...)
	h.GET("/x", list)
	prefix := ""
	i := echo.New()
	i.GET("/x", show)
	j := i.Group(prefix)
	j.GET("/x", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /keep [] show"]
        );
    }

    #[test]
    fn a_handler_name_resolves_to_a_function_and_never_to_a_method() {
        let main = go_file(
            &["\"net/http\"", CHI],
            "type S struct{}\n\nfunc (s *S) handle(w http.ResponseWriter, r *http.Request) {}\n\nfunc (s *S) onlyMethod(w http.ResponseWriter, r *http.Request) {}\n\nfunc routes() {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", handle)\n\tr.Get(\"/y\", onlyMethod)\n}\n",
        );
        let other = go_file(
            &["\"net/http\""],
            "func handle(w http.ResponseWriter, r *http.Request) {}\n",
        );
        let symbols = symbols_of(&[("main.go", &main), ("other.go", &other)]);
        assert_eq!(symbols.len(), 2);
        let first = symbols[0].as_deref().unwrap();
        assert!(first.contains("other.go"), "{first}");
        assert!(!first.contains("S.handle"), "{first}");
        assert_eq!(symbols[1], None);
    }

    #[test]
    fn a_gorilla_subrouter_with_a_not_found_handler_hides_the_later_routes_of_its_parent() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	s := r.PathPrefix("/api").Subrouter()
	s.NotFoundHandler = http.HandlerFunc(show)
	r.HandleFunc("/api/x", list)
	k := mux.NewRouter()
	t := k.PathPrefix("/api").Subrouter()
	t.HandleFunc("/x", list).Methods("POST")
	t.MethodNotAllowedHandler = http.HandlerFunc(show)
	k.HandleFunc("/api/x", show).Methods("GET")
	m := mux.NewRouter()
	m.HandleFunc("/keep", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux POST /api/x [] list",
                "gorilla/mux ANY /keep [] show"
            ]
        );
    }

    #[test]
    fn a_gorilla_subrouter_that_omen_cannot_read_hides_the_later_routes() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	prefix := "/api"
	r := mux.NewRouter()
	s := r.PathPrefix(prefix).Subrouter()
	s.HandleFunc("/x", list)
	r.HandleFunc("/api/x", show)
	t := mux.NewRouter()
	t.NewRoute().Subrouter().HandleFunc("/x", list)
	t.HandleFunc("/x", show)
	m := mux.NewRouter()
	m.HandleFunc("/keep", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /keep [] show"]
        );
    }

    #[test]
    fn gorilla_strict_slash_makes_routes_that_differ_by_a_final_slash_overlap() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	r := mux.NewRouter()
	r.StrictSlash(true)
	r.HandleFunc("/x", list)
	r.HandleFunc("/x/", show)
	m := mux.NewRouter()
	m.StrictSlash(false)
	m.HandleFunc("/keep", show)
	m.HandleFunc("/keep/", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gorilla/mux ANY /keep [] show",
                "gorilla/mux ANY /keep/ [] list"
            ]
        );
    }

    #[test]
    fn a_net_http_route_in_a_branch_that_never_runs_gives_no_entry() {
        let source = go_source(
            &[],
            r#"func routes() {
	if false {
		http.HandleFunc("/ghost", show)
	}
	m := http.NewServeMux()
	if false {
		m.HandleFunc("/ghost2", show)
	}
	if true {
		m.HandleFunc("/live", show)
	} else {
		m.HandleFunc("/ghost3", show)
	}
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["net/http ANY /live [] show"]
        );
    }

    #[test]
    fn a_chi_group_with_a_named_function_hides_the_earlier_routes_of_its_tree() {
        let source = go_source(
            &[CHI],
            r#"func api(r chi.Router) { r.Get("/x", list) }

func routes() {
	r := chi.NewRouter()
	r.Get("/x", show)
	r.Group(api)
	k := chi.NewRouter()
	k.Get("/keep", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /keep [] show"]
        );
    }

    #[test]
    fn a_gorilla_template_that_the_router_cleans_gives_no_entry() {
        let source = go_source(
            &[MUX],
            r#"func routes() {
	m := mux.NewRouter()
	m.HandleFunc("/keep", show)
	m.HandleFunc("/a//b", show)
	m.HandleFunc("/c/./d", show)
	m.HandleFunc("/e/../f", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /keep [] show"]
        );
    }
    // The timing tests of the Go routes time only the search for entry points. A test that is
    // not ignored checks that the search takes less than three times as long for twice the
    // input, with a floor for times that noise decides. A test that is ignored runs a large
    // input in release.

    type Search = (usize, std::time::Duration);

    fn timed_search(files: &[(String, String)]) -> (Vec<EntryPoint>, std::time::Duration) {
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        entries_timed(Language::Go, &borrowed, true)
    }

    fn timed_source(source: String) -> (Vec<EntryPoint>, std::time::Duration) {
        timed_search(&[("main.go".to_string(), source)])
    }

    fn assert_found_counts(small: usize, run: fn(usize) -> Search, expected: fn(usize) -> usize) {
        assert_eq!(run(small).0, expected(small));
        assert_eq!(run(small * 2).0, expected(small * 2));
    }

    // The ratio check runs only in the release timing step: a debug or coverage
    // build on a shared runner is too slow and too noisy for a ratio of times.
    macro_rules! go_timing {
        ($scenario:ident, $small_test:ident, $large_test:ident, $small:expr, $large:expr, $expected:expr) => {
            #[test]
            fn $small_test() {
                assert_found_counts($small, $scenario, $expected);
            }

            #[test]
            #[ignore = "timing: run in release"]
            fn $large_test() {
                assert_scales_linearly_above(
                    stringify!($scenario),
                    $small,
                    std::time::Duration::from_millis(300),
                    |size| $scenario(size).1,
                );
                let (_, took) = $scenario($large);
                assert!(took < std::time::Duration::from_secs(20), "took {took:?}");
            }
        };
    }

    fn scenario_handlers_and_registrations(count: usize) -> Search {
        let mut body = String::new();
        for index in 0..count {
            body.push_str(&format!("func h{index}(c *gin.Context) {{}}\n\n"));
        }
        body.push_str("func routes() {\n\tr := gin.New()\n");
        for index in 0..count {
            body.push_str(&format!("\tr.GET(\"/{index}\", h{index})\n"));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[GIN], &body));
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        (found.len(), took)
    }

    go_timing!(
        scenario_handlers_and_registrations,
        handlers_and_registrations_are_read_in_linear_time,
        timing_ten_thousand_handlers_and_registrations_are_read_in_linear_time,
        1_000,
        10_000,
        |count| count
    );

    fn scenario_returned_routers(count: usize) -> Search {
        let mut body = String::from("func pick(n int) *chi.Mux {\n");
        for index in 0..count {
            body.push_str(&format!(
                "\tif n == {index} {{\n\t\tr{index} := chi.NewRouter()\n\t\treturn r{index}\n\t}}\n"
            ));
        }
        body.push_str("\treturn nil\n}\n");
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_returned_routers,
        many_returned_routers_are_read_in_linear_time,
        timing_a_function_with_many_returned_routers_is_read_in_linear_time,
        600,
        5_000,
        |_| 0
    );

    fn scenario_mount_chain(count: usize) -> Search {
        let mut body = String::from("func chain() {\n");
        for index in 0..=count {
            body.push_str(&format!("\tr{index} := chi.NewRouter()\n"));
        }
        for index in 0..count {
            body.push_str(&format!("\tr{index}.Mount(\"/x\", r{})\n", index + 1));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_mount_chain,
        a_chain_of_mounts_is_read_in_linear_time,
        timing_a_chain_of_ten_thousand_mounts_is_read_in_linear_time,
        1_250,
        10_000,
        |_| 0
    );

    /// The hidden records of a long chain of mounts must not each build the path of the chain.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_the_hidden_records_of_a_chain_of_mounts_cost_linear_time() {
        assert_scales_linearly("mount chain", 10_000, |size| scenario_mount_chain(size).1);
    }

    /// The handler of each package must not search all the imports of the file.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_the_handlers_of_many_imported_packages_cost_linear_time() {
        assert_scales_linearly("imported packages", 8_000, |size| {
            scenario_packages_with_one_handler_name(size).1
        });
    }

    fn scenario_packages_with_one_handler_name(count: usize) -> Search {
        let mut imports = vec!["\"net/http\"".to_string(), CHI.to_string()];
        let mut body = String::from("func routes() {\n\tr := chi.NewRouter()\n");
        let mut files: Vec<(String, String)> = Vec::new();
        for index in 0..count {
            imports.push(format!("\"example.com/app/p{index:05}\""));
            body.push_str(&format!("\tr.Get(\"/{index}\", p{index:05}.Show)\n"));
            files.push((
                format!("p{index:05}/show.go"),
                format!(
                    "package p{index:05}\n\nimport \"net/http\"\n\nfunc Show(w http.ResponseWriter, r *http.Request) {{}}\n"
                ),
            ));
        }
        body.push_str("}\n");
        let import_refs: Vec<&str> = imports.iter().map(String::as_str).collect();
        files.push(("main.go".to_string(), go_file(&import_refs, &body)));
        let (found, took) = timed_search(&files);
        assert!(found.iter().all(|entry| entry
            .symbol
            .as_deref()
            .is_some_and(|symbol| symbol.ends_with("/show.go#Show"))));
        (found.len(), took)
    }

    go_timing!(
        scenario_packages_with_one_handler_name,
        many_packages_with_one_handler_name_are_read_in_linear_time,
        timing_many_packages_with_one_handler_name_are_read_in_linear_time,
        250,
        2_000,
        |count| count
    );

    fn scenario_wrapped_handlers(count: usize) -> Search {
        let mut body =
            String::from("func wrap(next http.HandlerFunc) http.HandlerFunc { return next }\n\n");
        for index in 0..count {
            body.push_str(&format!(
                "func h{index}(w http.ResponseWriter, r *http.Request) {{}}\n\n"
            ));
        }
        body.push_str("func routes() {\n");
        for index in 0..count {
            body.push_str(&format!(
                "\thttp.HandleFunc(\"/{index}\", wrap(h{index}))\n"
            ));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_wrapped_handlers,
        wrapped_handlers_are_read_in_linear_time,
        timing_ten_thousand_wrapped_handlers_are_read_in_linear_time,
        1_250,
        10_000,
        |count| count
    );

    fn scenario_mentioned_factories(count: usize) -> Search {
        let files: Vec<(String, String)> = (0..count)
            .map(|index| {
                (
                    format!("p{index:05}/router.go"),
                    format!(
                        "package p{index:05}\n\nimport \"github.com/go-chi/chi/v5\"\n\nfunc New() *chi.Mux {{\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", nil)\n\treturn r\n}}\n\nvar Keep = New\n"
                    ),
                )
            })
            .collect();
        let (found, took) = timed_search(&files);
        (found.len(), took)
    }

    go_timing!(
        scenario_mentioned_factories,
        many_factories_with_one_name_that_are_mentioned_are_read_in_linear_time,
        timing_many_factories_with_one_name_that_are_mentioned_are_read_in_linear_time,
        600,
        5_000,
        |_| 0
    );

    fn scenario_parents_of_one_router(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tchild := chi.NewRouter()\n");
        for index in 0..count {
            body.push_str(&format!(
                "\tchild.Route(\"/c{index}\", func(chi.Router) {{}})\n"
            ));
        }
        for index in 0..count {
            body.push_str(&format!(
                "\tp{index} := chi.NewRouter()\n\tp{index}.Mount(\"/x\", child)\n"
            ));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_parents_of_one_router,
        one_router_that_many_parents_mount_is_read_in_linear_time,
        timing_one_router_that_many_parents_mount_is_read_in_linear_time,
        600,
        5_000,
        |_| 0
    );

    fn scenario_servemux_variables(count: usize) -> Search {
        let mut body = String::from("func routes() {\n");
        for index in 0..count {
            body.push_str(&format!(
                "\tm{index} := http.NewServeMux()\n\tm{index}.HandleFunc(\"/{index}\", list)\n"
            ));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_servemux_variables,
        servemux_variables_are_read_in_linear_time,
        timing_ten_thousand_servemux_variables_are_read_in_linear_time,
        1_250,
        10_000,
        |count| count
    );

    fn scenario_group_chain(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n\tr0.GET(\"/0\", list)\n");
        for index in 1..count {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"\")\n\tr{index}.GET(\"/{index}\", list)\n",
                previous = index - 1
            ));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[GIN], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_group_chain,
        a_chain_of_groups_is_read_in_linear_time,
        timing_a_chain_of_twenty_thousand_groups_is_read_in_linear_time,
        2_500,
        20_000,
        |count| count
    );

    fn scenario_wildcard_pattern(count: usize) -> Search {
        let mut pattern = String::new();
        for index in 0..count {
            pattern.push_str(&format!("/{{p{index}}}"));
        }
        let started = crate::index::timing::Stopwatch::start();
        assert!(go_routes::servemux_pattern(&pattern).is_some());
        (1, started.elapsed())
    }

    go_timing!(
        scenario_wildcard_pattern,
        a_pattern_with_many_wildcards_is_read_in_linear_time,
        timing_a_pattern_with_a_hundred_thousand_wildcards_is_read_in_linear_time,
        12_500,
        100_000,
        |_| 1
    );

    fn scenario_conditional_group_returns(count: usize) -> Search {
        let mut body = String::from("func pick(n int) *gin.RouterGroup {\n\tr0 := gin.New()\n");
        for index in 1..count {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"\")\n\tif n == {index} {{\n\t\treturn r{index}\n\t}}\n",
                previous = index - 1
            ));
        }
        body.push_str("\treturn nil\n}\n");
        let (found, took) = timed_source(go_source(&[GIN], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_conditional_group_returns,
        many_conditional_returns_of_nested_groups_are_read_in_linear_time,
        timing_many_conditional_returns_of_nested_groups_are_read_in_linear_time,
        3_750,
        30_000,
        |_| 0
    );

    fn scenario_method_lists(count: usize) -> Search {
        let tokens: Vec<String> = (0..count).map(|index| format!("\"M{index:05}\"")).collect();
        let list = tokens.join(", ");
        let body = format!(
            "func routes() {{\n\tr := mux.NewRouter()\n\tr.Methods({list}).Methods({list}).Path(\"/x\").HandlerFunc(list)\n}}\n"
        );
        let (found, took) = timed_source(go_source(&[MUX], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_method_lists,
        two_long_method_lists_are_intersected_in_linear_time,
        timing_two_long_method_lists_are_intersected_in_linear_time,
        12_500,
        100_000,
        |count| count
    );

    fn scenario_nested_gin_groups(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n");
        for index in 1..=count {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"/x\")\n",
                previous = index - 1
            ));
        }
        body.push_str(&format!("\tr{count}.GET(\"/end\", list)\n}}\n"));
        let (found, took) = timed_source(go_source(&[GIN], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_nested_gin_groups,
        nested_gin_groups_join_paths_in_linear_time,
        timing_ten_thousand_nested_gin_groups_join_paths_in_linear_time,
        7_500,
        60_000,
        |_| 1
    );

    fn scenario_alias_chain(count: usize) -> Search {
        let mut body = String::from("type Base struct{ *chi.Mux }\n\ntype A00000 = Base\n");
        for index in 1..=count {
            body.push_str(&format!(
                "type A{index:05} = A{previous:05}\n",
                previous = index - 1
            ));
        }
        body.push_str(&format!(
            "\nfunc New() *A{count:05} {{\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n\treturn &A{count:05}{{Mux: r}}\n}}\n"
        ));
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_alias_chain,
        a_chain_of_aliases_is_resolved_in_linear_time,
        timing_a_chain_of_forty_thousand_aliases_is_resolved_in_linear_time,
        12_500,
        100_000,
        |_| 1
    );

    fn scenario_gorilla_routes(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tr := mux.NewRouter()\n");
        for index in 0..count {
            body.push_str(&format!("\tr.HandleFunc(\"/p{index}/x\", list)\n"));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[MUX], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_gorilla_routes,
        many_gorilla_routes_are_ordered_in_near_linear_time,
        timing_many_gorilla_routes_are_ordered_in_near_linear_time,
        7_500,
        60_000,
        |count| count
    );

    fn scenario_slash_groups(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n");
        for index in 1..=count {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"/\")\n\tr{index}.GET(\"/{index}\", list)\n",
                previous = index - 1
            ));
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[GIN], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_slash_groups,
        nested_gin_groups_with_a_slash_prefix_cost_linear_time,
        timing_ten_thousand_nested_gin_groups_with_a_slash_prefix_cost_linear_time,
        6_250,
        50_000,
        |count| count
    );

    fn scenario_package_variables(count: usize) -> Search {
        let mut body = String::new();
        for index in 0..count {
            body.push_str(&format!("var v{index} = 0\nfunc f{index}() {{}}\n"));
        }
        body.push_str("func routes() {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n}\n");
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_package_variables,
        many_package_variables_and_functions_cost_linear_time,
        timing_many_package_variables_and_functions_cost_linear_time,
        2_500,
        20_000,
        |_| 1
    );

    fn scenario_functions_in_one_file(count: usize) -> Search {
        let mut body = String::new();
        for index in 0..count {
            body.push_str(&format!(
                "func make{index}() *chi.Mux {{\n\tr := chi.NewRouter()\n\treturn r\n}}\n\n"
            ));
        }
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_functions_in_one_file,
        many_functions_in_one_file_are_read_in_linear_time,
        timing_ten_thousand_functions_in_one_file_are_read_in_linear_time,
        1_250,
        10_000,
        |_| 0
    );

    fn scenario_files_that_call_one_factory(count: usize) -> Search {
        let router = factory_file("NewRouter", "/x");
        let mut files: Vec<(String, String)> = vec![("router.go".to_string(), router)];
        for index in 0..count {
            files.push((
                format!("use{index:05}.go"),
                go_file(
                    &["\"net/http\""],
                    &format!(
                        "func serve{index}() {{\n\thttp.ListenAndServe(\":80\", NewRouter())\n}}\n"
                    ),
                ),
            ));
        }
        let (found, took) = timed_search(&files);
        (found.len(), took)
    }

    go_timing!(
        scenario_files_that_call_one_factory,
        many_files_that_call_one_factory_are_read_in_linear_time,
        timing_five_thousand_files_that_call_one_factory_are_read_in_linear_time,
        600,
        5_000,
        |_| 1
    );

    fn scenario_calls_of_one_factory(count: usize) -> Search {
        let mut body = String::from("func newRouter() *gin.Engine { return gin.New() }\n\n");
        body.push_str("func routes() {\n");
        for _ in 0..count {
            body.push_str("\t{ r := newRouter(); r.GET(\"/x\", show) }\n");
        }
        body.push_str("}\n");
        let (found, took) = timed_source(go_source(&[GIN], &body));
        assert!(found.iter().all(|entry| {
            entry.framework == "gin"
                && entry.guards.is_empty()
                && entry
                    .route
                    .as_ref()
                    .is_some_and(|route| route.path_text() == "/x")
        }));
        (found.len(), took)
    }

    go_timing!(
        scenario_calls_of_one_factory,
        many_calls_of_one_factory_are_read_in_linear_time,
        timing_ten_thousand_calls_of_one_factory_are_read_in_linear_time,
        1_250,
        10_000,
        |count| count
    );

    fn scenario_factory_chain(steps: usize) -> Search {
        let mut body = String::from("func f0() *gin.Engine { return gin.New() }\n\n");
        for step in 1..=steps {
            body.push_str(&format!(
                "func f{step}() *gin.Engine {{\n\ta := f{previous}()\n\t_ = f{previous}()\n\treturn a\n}}\n\n",
                previous = step - 1
            ));
        }
        body.push_str(&format!(
            "func routes() {{\n\tr := f{steps}()\n\tr.GET(\"/x\", show)\n}}\n"
        ));
        let (found, took) = timed_source(go_source(&[GIN], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_factory_chain,
        a_chain_of_factories_that_call_each_other_twice_is_read_in_linear_time,
        timing_a_chain_of_factories_that_call_each_other_twice_is_read_in_linear_time,
        10,
        30,
        |_| 1
    );

    fn scenario_nested_blocks(depth: usize) -> Search {
        let mut nested = String::from("func nested(c bool) {\n");
        for index in 0..depth {
            nested.push_str(&format!("if c {{\nhttp.HandleFunc(\"/p{index}\", show)\n"));
        }
        for _ in 0..depth {
            nested.push_str("}\n");
        }
        nested.push_str("}\n");
        let (found, took) = timed_source(go_source(&[], &nested));
        (found.len(), took)
    }

    go_timing!(
        scenario_nested_blocks,
        many_nested_blocks_with_net_http_routes_cost_linear_time,
        timing_many_nested_blocks_with_net_http_routes_cost_linear_time,
        1_000,
        20_000,
        |depth| depth
    );

    fn scenario_deep_expression(count: usize) -> Search {
        let mut body = String::from("func routes() {\n\tx := 1");
        for _ in 0..count {
            body.push_str("+1");
        }
        body.push_str("\n\t_ = x\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n}\n");
        let (found, took) = timed_source(go_source(&[CHI], &body));
        (found.len(), took)
    }

    go_timing!(
        scenario_deep_expression,
        a_deep_expression_does_not_overflow_and_costs_linear_time,
        timing_a_deep_expression_costs_linear_time,
        12_500,
        200_000,
        |_| 1
    );

    fn scenario_method_chain(count: usize) -> Search {
        let mut chain = String::from("func more() {\n\tq := mux.NewRouter()\n\tq.Path(\"/x\")");
        for _ in 0..count {
            chain.push_str(".Methods(\"GET\")");
        }
        chain.push_str("\n}\n");
        let (found, took) = timed_source(go_source(&[MUX], &chain));
        (found.len(), took)
    }

    go_timing!(
        scenario_method_chain,
        a_long_method_chain_costs_linear_time,
        timing_a_long_method_chain_costs_linear_time,
        1_250,
        40_000,
        |_| 0
    );

    #[test]
    fn documented_gap_a_caller_in_another_file_that_registers_on_a_factory_result_gives_no_entry() {
        let router = factory_file("NewRouter", "/x");
        let main = go_file(
            &["\"net/http\"", CHI],
            "func serve() {\n\tr := NewRouter()\n\tr.Get(\"/y\", nil)\n\thttp.ListenAndServe(\":80\", r)\n\thttp.ListenAndServe(\":81\", NewRouter())\n}\n",
        );
        assert_eq!(
            go_routes(&[("router.go", &router), ("main.go", &main)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_call_in_another_file_that_passes_the_result_on_gives_no_entry() {
        let router = factory_file("NewRouter", "/x");
        let other = factory_file("OtherRouter", "/z");
        let main = go_file(
            &[CHI],
            "func setup(r chi.Router) {}\n\nfunc serve() {\n\tr := NewRouter()\n\tsetup(r)\n\to := OtherRouter()\n\t_ = o\n\tgo func() { o.Get(\"/w\", nil) }()\n\tx := OtherRouter()\n\tholder = x\n}\n\nvar holder chi.Router\n",
        );
        assert_eq!(
            go_routes(&[
                ("main.go", &main),
                ("other.go", &other),
                ("router.go", &router)
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_factory_with_a_custom_result_type_follows_the_attach_rule() {
        let alias = go_source(
            &[CHI],
            "type Web = *chi.Mux\n\nfunc NewRouter() Web {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n\treturn r\n}\n",
        );
        let wrapper = go_source(
            &[CHI],
            "type Server struct{ *chi.Mux }\n\nfunc NewServer() *Server {\n\tr := chi.NewRouter()\n\tr.Get(\"/y\", list)\n\treturn &Server{Mux: r}\n}\n",
        );
        let main = go_file(
            &[CHI],
            "func serve() {\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", NewRouter())\n\troot.Mount(\"/web\", NewServer())\n}\n",
        );
        assert_eq!(
            go_routes(&[
                ("a_main.go", &main),
                ("b_alias.go", &alias),
                ("c_wrapper.go", &wrapper)
            ]),
            Vec::<String>::new()
        );
        let served = go_file(
            &["\"net/http\"", CHI],
            "func serve() {\n\thttp.ListenAndServe(\":80\", NewRouter())\n\thttp.ListenAndServe(\":81\", NewServer())\n}\n",
        );
        assert_eq!(
            go_routes(&[
                ("a_main.go", &served),
                ("b_alias.go", &alias),
                ("c_wrapper.go", &wrapper)
            ]),
            ["chi GET /x [] list", "chi GET /y [] list"]
        );
    }

    #[test]
    fn a_mention_of_a_factory_that_is_not_a_call_blocks_its_routes() {
        let router = factory_file("NewRouter", "/x");
        let other = factory_file("OtherRouter", "/z");
        let main = go_file(
            &[CHI],
            "func serve() {\n\tf := NewRouter\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", f())\n\tregister(OtherRouter)\n}\n\nfunc register(make func() *chi.Mux) {}\n",
        );
        assert_eq!(
            go_routes(&[
                ("a_main.go", &main),
                ("b_router.go", &router),
                ("c_other.go", &other)
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_call_through_the_import_of_another_package_attaches_the_factory() {
        let router = factory_file("NewRouter", "/x").replacen("package main", "package api", 1);
        let main = go_file(
            &[CHI, "\"example.com/app/api\""],
            "func serve() {\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", api.NewRouter())\n}\n",
        );
        assert_eq!(
            go_routes(&[("api/router.go", &router), ("main.go", &main)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_factory_without_a_call_in_the_repository_keeps_its_routes() {
        let router = factory_file("NewRouter", "/x");
        let main = go_file(&[], "func serve() {}\n");
        assert_eq!(
            go_routes(&[("router.go", &router), ("main.go", &main)]),
            ["chi GET /x [] list"]
        );
    }

    #[test]
    fn a_method_that_returns_a_router_follows_the_same_rule() {
        let server = go_source(
            &[CHI],
            "type Server struct{}\n\nfunc (s *Server) Routes() *chi.Mux {\n\tr := chi.NewRouter()\n\tr.Get(\"/m\", list)\n\treturn r\n}\n",
        );
        let main = go_file(
            &[CHI],
            "func serve(s *Server) {\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", s.Routes())\n}\n",
        );
        assert_eq!(
            go_routes(&[("server.go", &server), ("main.go", &main)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_routes_of_a_factory_keep_their_paths_when_the_result_is_only_a_root() {
        let source = go_source(
            &[CHI],
            r#"func rootRouter() chi.Router {
	r := chi.NewRouter()
	r.Get("/root", list)
	return r
}

func serve() {
	r := rootRouter()
	r.Get("/extra", show)
	http.ListenAndServe(":8080", r)
	server := &http.Server{Addr: ":9090", Handler: rootRouter()}
	_ = server
}

func never() {
	_ = rootRouter()
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn the_routes_of_a_factory_give_no_entry_when_a_call_attaches_the_result() {
        let source = go_source(
            &[CHI],
            r#"type holder struct{ router chi.Router }

func attach(parent chi.Router, sub chi.Router) {}

func mounted() chi.Router {
	s := chi.NewRouter()
	s.Get("/a", list)
	return s
}

func passed() chi.Router {
	s := chi.NewRouter()
	s.Get("/b", list)
	return s
}

func stored() chi.Router {
	s := chi.NewRouter()
	s.Get("/c", list)
	return s
}

func forwarded() chi.Router {
	s := chi.NewRouter()
	s.Get("/d", list)
	return s
}

func wrapped() chi.Router {
	return forwarded()
}

func grouped() chi.Router {
	s := chi.NewRouter()
	s.Get("/e", list)
	return s
}

func prefixed() chi.Router {
	root := chi.NewRouter()
	root.Get("/f", list)
	return root.Route("/p", func(r chi.Router) {})
}

func routes() {
	root := chi.NewRouter()
	root.Mount("/sub", mounted())
	attach(root, passed())
	h := holder{}
	h.router = stored()
	_ = wrapped()
	g := grouped()
	root.Mount("/g", g)
	_ = prefixed()
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn a_factory_that_is_called_twice_gives_no_entry_when_one_call_mounts_it() {
        let source = go_source(
            &[CHI],
            r#"func api() chi.Router {
	s := chi.NewRouter()
	s.Get("/x", list)
	return s
}

func routes() {
	root := chi.NewRouter()
	root.Mount("/v1", api())
	other := api()
	other.Get("/own", show)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn a_factory_without_a_summary_that_returns_a_router_gives_no_entry() {
        let source = go_source(
            &[CHI],
            r#"var shared = chi.NewRouter()

func impure() chi.Router {
	shared.Use(auth)
	s := chi.NewRouter()
	s.Get("/x", list)
	return s
}

func routes() {
	_ = impure()
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), Vec::<String>::new());
    }

    #[test]
    fn a_function_gives_its_routes_one_time_at_their_own_place() {
        let source = go_source(
            &[MUX],
            r#"func makeRouter() *mux.Router {
	r := mux.NewRouter()
	r.HandleFunc("/x", show)
	return r
}

func main() {
	_ = makeRouter()
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /x [] show"]
        );
    }

    #[test]
    fn a_call_of_a_pure_factory_gives_a_new_router_with_the_summary() {
        let source = go_source(
            &[CHI],
            r#"func api() chi.Router {
	root := chi.NewRouter()
	root.Use(logger)
	sub := root.With(audit)
	return sub
}

func routes() {
	a := api()
	a.Get("/a", show)
	b := api()
	b.Use(auth)
	b.Get("/b", show)
	c := api()
	c.Get("/c", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "chi GET /a [logger,audit] show",
                "chi GET /b [logger,audit,auth] show",
                "chi GET /c [logger,audit] show",
            ]
        );
    }

    #[test]
    fn a_function_that_writes_a_package_variable_gives_no_proof() {
        let source = go_source(
            &[GIN],
            r#"var current *gin.Engine

func makeRouter() *gin.Engine {
	current = gin.New()
	return current
}

func main() {
	first := makeRouter()
	first.Use(auth)
	_ = makeRouter()
	current.GET("/public", show)
	first.GET("/other", show)
}
"#,
        );
        assert!(go_routes(&[("main.go", &source)]).is_empty());
    }

    #[test]
    fn a_package_router_that_a_function_assigns_has_no_proof() {
        let source = go_source(
            &[GIN],
            r#"var shared = gin.New()
var stable = gin.New()

func reset() {
	shared = gin.New()
}

func main() {
	shared.GET("/a", show)
	stable.GET("/b", show)
}
"#,
        );
        assert_eq!(go_routes(&[("main.go", &source)]), ["gin GET /b [] show"]);
    }

    #[test]
    fn a_function_that_changes_a_router_it_did_not_make_has_no_summary() {
        let source = go_source(
            &[MUX],
            r#"var shared = mux.NewRouter()

func factory() *mux.Router {
	shared.Use(auth)
	return mux.NewRouter()
}

func receive(r *mux.Router) *mux.Router {
	r.Use(audit)
	return r
}

func main() {
	r := factory()
	r.HandleFunc("/a", show)
	shared.HandleFunc("/b", show)
	p := receive(shared)
	p.HandleFunc("/c", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gorilla/mux ANY /b [auth] show"]
        );
    }

    #[test]
    fn a_recursive_function_has_no_summary() {
        let source = go_source(
            &[GIN],
            r#"func loop(depth int) *gin.Engine {
	if depth > 0 {
		return loop(depth - 1)
	}
	return gin.New()
}

func main() {
	r := loop(2)
	r.GET("/a", show)
}
"#,
        );
        assert!(go_routes(&[("main.go", &source)]).is_empty());
    }

    #[test]
    fn gin_and_echo_keep_a_custom_method_as_the_source_writes_it() {
        let source = go_source(
            &[GIN, ECHO],
            r#"func routes() {
	g := gin.New()
	g.Match([]string{"get"}, "/g", show)
	g.Handle("Purge", "/h", show)
	e := echo.New()
	e.Add("get", "/e", show)
	e.Match([]string{"Link"}, "/m", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin get /g [] show",
                "gin Purge /h [] show",
                "echo get /e [] show",
                "echo Link /m [] show",
            ]
        );
    }

    #[test]
    fn a_handler_signature_is_proven_from_the_parameter_list() {
        let source = go_source(
            &[],
            r#"func callback(w http.ResponseWriter, r *http.Request, err error) {}

func build(fn func(http.ResponseWriter, *http.Request, error)) http.Handler {
	return http.NotFoundHandler()
}

func unnamed(http.ResponseWriter, *http.Request) {}

func swapped(r *http.Request, w http.ResponseWriter) {}

func routes() {
	http.Handle("/a", build(callback))
	http.Handle("/b", auth(http.HandlerFunc(unnamed)))
	http.Handle("/c", auth(http.HandlerFunc(swapped)))
	http.HandleFunc("/d", authFunc(unnamed))
	http.HandleFunc("/e", authFunc(swapped))
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "net/http ANY /a [] -",
                "net/http ANY /b [auth] unnamed",
                "net/http ANY /c [auth] swapped",
                "net/http ANY /d [authFunc] unnamed",
                "net/http ANY /e [] -",
            ]
        );
    }

    #[test]
    fn an_echo_handler_has_the_context_parameter_and_an_error_result() {
        let source = go_source(
            &[ECHO],
            r#"func echoShow(c echo.Context) error { return nil }

func echoWrong(c echo.Context) {}

func guard(next echo.HandlerFunc) echo.HandlerFunc { return next }

func routes() {
	e := echo.New()
	e.GET("/a", guard(echoShow))
	e.GET("/b", guard(echoWrong))
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["echo GET /a [guard] echoShow", "echo GET /b [] -"]
        );
    }

    #[test]
    fn each_call_of_a_router_factory_gives_a_new_router() {
        let source = go_source(
            &[GIN],
            r#"func newRouter() *gin.Engine { return gin.New() }

func routes() {
	a := newRouter()
	a.Use(auth)
	a.GET("/private", show)
	b := newRouter()
	b.GET("/public", list)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["gin GET /private [auth] show", "gin GET /public [] list"]
        );
    }

    #[test]
    fn a_gin_receiver_must_be_proven_through_the_import() {
        let aliased = go_source(
            &["g \"github.com/gin-gonic/gin\""],
            r#"func routes() {
	r := g.Default()
	r.GET("/a", list)
}

func typed(group *g.RouterGroup) {
	group.POST("/b", show)
}

func engine(e *g.Engine) {
	e.POST("/c", show)
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &aliased)]),
            ["gin GET /a [] list", "gin POST /c [] show"]
        );
        let unproven = go_source(
            &[GIN],
            r#"func routes(store thing) {
	store.GET("/a", list)
	store.Group("/x").GET("/b", list)
	router := newRouter()
	router.GET("/c", list)
}
"#,
        );
        assert!(go_routes(&[("main.go", &unproven)]).is_empty());
    }

    #[test]
    fn net_http_wrapped_handlers_need_a_handler_signature_and_a_known_middleware() {
        let source = go_source(
            &[],
            r#"func routes() {
	http.Handle("/p", auth(http.HandlerFunc(show)))
	http.HandleFunc("/q", authFunc(show))
	http.Handle("/r", audit(auth(http.HandlerFunc(show))))
	http.HandleFunc("/s", combine(list, show))
	http.HandleFunc("/t", makeHandler(onError))
	http.Handle("/u", http.StripPrefix("/x", files))
	http.HandleFunc("/v", unknownWrap(show))
	http.HandleFunc("/y", unknownWrap(helper))
	http.Handle("/z", middleware.Auth(cfg)(http.HandlerFunc(show)))
	http.HandleFunc("GET /w/{id}", authFunc(show))
	http.HandleFunc("/x", http.HandlerFunc(list))
}

func helper(count int) {}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "net/http ANY /p [auth] show",
                "net/http ANY /q [authFunc] show",
                "net/http ANY /r [audit,auth] show",
                "net/http ANY /s [combine] -",
                "net/http ANY /t [] -",
                "net/http ANY /u [] -",
                "net/http ANY /v [] show",
                "net/http ANY /y [] -",
                "net/http ANY /z [] show",
                "net/http GET /w/{id} [authFunc] show",
                "net/http ANY /x [] list",
            ]
        );
    }

    #[test]
    fn a_wrapper_of_a_go_handler_that_is_not_a_known_middleware_makes_the_guards_not_complete() {
        let source = go_source(
            &[CHI],
            r#"func routes() {
	http.Handle("/direct", http.HandlerFunc(show))
	http.HandleFunc("/named", show)
	http.Handle("/known", audit(auth(http.HandlerFunc(show))))
	http.Handle("/z", middleware.Auth(cfg)(http.HandlerFunc(show)))
	http.HandleFunc("/v", unknownWrap(show))
	http.Handle("/r", audit(unknownWrap(http.HandlerFunc(show))))
	http.HandleFunc("/t", makeHandler(onError))
	r := chi.NewRouter()
	r.Get("/chi", unknownWrap(show))
	r.Get("/chi-known", authFunc(show))
	http.ListenAndServe(":8080", r)
}
"#,
        );
        type Row = (
            String,
            Option<String>,
            Vec<String>,
            Option<GuardsUnknownReason>,
        );
        let found: Vec<Row> = entries(Language::Go, &[("main.go", &source)])
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| {
                (
                    entry.route.unwrap().path_text().to_string(),
                    entry
                        .symbol
                        .map(|symbol| symbol.rsplit('#').next().unwrap().to_string()),
                    entry.guards,
                    entry.guards_unknown_reason,
                )
            })
            .collect();
        let unread = Some(GuardsUnknownReason::UnreadMiddleware);
        let show = Some(String::from("show"));
        assert_eq!(
            found,
            [
                (String::from("/direct"), show.clone(), vec![], None),
                (String::from("/named"), show.clone(), vec![], None),
                (
                    String::from("/known"),
                    show.clone(),
                    vec![String::from("audit"), String::from("auth")],
                    None
                ),
                (String::from("/z"), show.clone(), vec![], unread),
                (String::from("/v"), show.clone(), vec![], unread),
                (
                    String::from("/r"),
                    show.clone(),
                    vec![String::from("audit")],
                    unread
                ),
                (String::from("/t"), None, vec![], unread),
                (String::from("/chi"), show.clone(), vec![], unread),
                (
                    String::from("/chi-known"),
                    show,
                    vec![String::from("authFunc")],
                    None
                ),
            ]
        );
    }

    #[test]
    fn framework_handler_adapters_are_not_guards() {
        let source = go_source(
            &[GIN],
            r#"func ginShow(c *gin.Context) {}

func gate(next gin.HandlerFunc) gin.HandlerFunc { return next }

func routes() {
	r := gin.New()
	r.GET("/a", gin.WrapF(show))
	r.GET("/b", gate(ginShow))
	r.GET("/c", gate(show))
}
"#,
        );
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            [
                "gin GET /a [] show",
                "gin GET /b [gate] ginShow",
                "gin GET /c [] -"
            ]
        );
    }

    #[test]
    fn go_registration_calls_and_main_are_entries() {
        let source = "package main\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/gin-gonic/gin\"\n)\n\nfunc health(w http.ResponseWriter, r *http.Request) {}\n\nfunc show(c *gin.Context) {}\n\nfunc main() {\n\thttp.HandleFunc(\"/health\", health)\n\trouter := gin.Default()\n\trouter.GET(\"/users/:id\", auth, show)\n}\n";
        let found = entries(Language::Go, &[("main.go", source)]);
        let frameworks: Vec<(&str, EntryKind)> = found
            .iter()
            .map(|entry| (entry.framework.as_str(), entry.kind))
            .collect();
        assert_eq!(
            frameworks,
            [
                ("go", EntryKind::CliMain),
                ("net/http", EntryKind::HttpHandler),
                ("gin", EntryKind::HttpHandler),
            ]
        );
        assert_eq!(found[2].route, route("GET", "/users/:id", &["id"]));
        assert_eq!(found[2].guards, ["auth"]);
    }

    #[test]
    fn rust_axum_routes_actix_attributes_main_and_exports_are_entries() {
        let source = "use axum::{routing::get, Router};\n\nasync fn list() {}\n\npub fn app() -> Router {\n    Router::new().route(\"/items/{id}\", get(list))\n}\n\n#[get(\"/ping\")]\nasync fn ping() {}\n\n#[no_mangle]\npub extern \"C\" fn exported() {}\n\nfn main() {}\n";
        let found = entries(Language::Rust, &[("main.rs", source)]);
        let summary: Vec<(&str, EntryKind)> = found
            .iter()
            .map(|entry| (entry.framework.as_str(), entry.kind))
            .collect();
        assert_eq!(
            summary,
            [
                ("axum", EntryKind::HttpHandler),
                ("actix", EntryKind::HttpHandler),
                ("ffi", EntryKind::FfiExport),
                ("rust", EntryKind::CliMain)
            ]
        );
        assert_eq!(found[0].route, route("GET", "/items/{id}", &["id"]));
        assert_eq!(found[1].route, route("GET", "/ping", &[]));
    }

    /// The handler symbol of each route, sorted by the path of the route.
    fn handlers_by_route(found: &[EntryPoint]) -> Vec<(String, Option<String>)> {
        let mut handlers: Vec<(String, Option<String>)> = found
            .iter()
            .filter_map(|entry| {
                let route = entry.route.as_ref()?;
                Some((route.path_text().to_string(), entry.symbol.clone()))
            })
            .collect();
        handlers.sort();
        handlers
    }

    const SCOPED_HANDLER_FILES: &[(&str, &str)] = &[
        ("src/lib.rs", "mod routes;\nmod server;\n"),
        ("src/routes/mod.rs", "pub mod health;\npub mod items;\n"),
        ("src/routes/health.rs", "pub async fn health() {}\n"),
        (
            "src/routes/items.rs",
            "pub async fn list() {}\n\npub async fn health() {}\n\npub struct Api;\n\nimpl Api {\n    pub async fn show() {}\n}\n",
        ),
        (
            "src/server.rs",
            "use axum::{routing::get, Router};\nuse crate::routes::{health, items};\n\nmod inline {\n    pub async fn ping() {}\n}\n\npub struct Server;\n\nimpl Server {\n    pub fn router() -> Router {\n        Router::new()\n            .route(\"/health\", get(health::health))\n            .route(\"/items\", get(items::list))\n            .route(\"/full\", get(crate::routes::items::list))\n            .route(\"/super\", get(super::routes::health::health))\n            .route(\"/show\", get(items::Api::show))\n            .route(\"/generic\", get(items::list::<u32>))\n            .route(\"/inline\", get(inline::ping))\n            .route(\"/self\", get(Self::handle))\n            .route(\"/missing\", get(items::missing))\n            .route(\"/external\", get(other_crate::handlers::list))\n    }\n\n    async fn handle() {}\n}\n\npub struct Other;\n\nimpl Other {\n    async fn handle() {}\n}\n",
        ),
    ];

    #[test]
    fn an_axum_handler_with_a_path_resolves_through_the_modules_of_the_index() {
        let found = entries(Language::Rust, SCOPED_HANDLER_FILES);
        let symbol = |key: &str| Some(key.to_string());
        assert_eq!(
            handlers_by_route(&found),
            [
                ("/external".to_string(), None),
                ("/full".to_string(), symbol("src/routes/items.rs#list")),
                ("/generic".to_string(), symbol("src/routes/items.rs#list")),
                ("/health".to_string(), symbol("src/routes/health.rs#health")),
                ("/inline".to_string(), symbol("src/server.rs#inline.ping")),
                ("/items".to_string(), symbol("src/routes/items.rs#list")),
                ("/missing".to_string(), None),
                ("/self".to_string(), symbol("src/server.rs#Server.handle")),
                ("/show".to_string(), symbol("src/routes/items.rs#Api.show")),
                ("/super".to_string(), symbol("src/routes/health.rs#health")),
            ]
        );
    }

    /// `count` modules with a handler of one name, and `count` files that each register
    /// one handler with a module path.
    fn scoped_axum_handlers(count: usize) -> (usize, std::time::Duration) {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut declarations = String::new();
        for index in 0..count {
            declarations.push_str(&format!("mod m{index};\nmod s{index};\n"));
            files.push((
                format!("src/m{index}.rs"),
                "pub async fn handler() {}\n".to_string(),
            ));
            let (import, path) = if index % 2 == 0 {
                (String::new(), format!("crate::m{index}::handler"))
            } else {
                (
                    format!("use crate::m{index};\n"),
                    format!("m{index}::handler"),
                )
            };
            files.push((
                format!("src/s{index}.rs"),
                format!("use axum::{{routing::get, Router}};\n{import}\npub fn app() -> Router {{\n    Router::new().route(\"/{index}\", get({path}))\n}}\n"),
            ));
        }
        files.push(("src/lib.rs".to_string(), declarations));
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let (found, took) = entries_timed(Language::Rust, &borrowed, true);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        (found.len(), took)
    }

    #[test]
    fn scoped_axum_handlers_of_one_name_all_resolve() {
        scoped_axum_handlers(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_axum_handlers_of_one_name_with_module_paths_resolve_in_linear_time() {
        assert_scales_linearly("scoped axum handlers", 10_000, |count| {
            scoped_axum_handlers(count).1
        });
    }

    #[test]
    fn an_axum_handler_path_is_not_resolved_by_its_last_name_alone() {
        let files = [
            ("src/lib.rs", "mod server;\nmod items;\n"),
            ("src/items.rs", "pub async fn other() {}\n"),
            (
                "src/server.rs",
                "use axum::{routing::get, Router};\n\npub fn app() -> Router {\n    Router::new().route(\"/a\", get(items::list)).route(\"/b\", get(crate::unknown::list))\n}\n",
            ),
            ("src/elsewhere.rs", "pub async fn list() {}\n"),
        ];
        let found = entries(Language::Rust, &files);
        assert_eq!(
            handlers_by_route(&found),
            [("/a".to_string(), None), ("/b".to_string(), None)]
        );
    }

    #[test]
    fn a_csharp_minimal_api_handler_with_a_type_resolves_to_the_method_of_the_type() {
        let program = "var app = WebApplication.Create(args);\napp.MapGet(\"/items\", Handlers.List);\napp.MapGet(\"/others\", Others.List);\n";
        let handlers = "public static class Handlers {\n    public static string List() => \"\";\n}\n\npublic static class Others {\n    public static string List() => \"\";\n}\n";
        let found = entries(
            Language::CSharp,
            &[("Program.cs", program), ("Handlers.cs", handlers)],
        );
        assert_eq!(
            handlers_by_route(&found),
            [
                (
                    "/items".to_string(),
                    Some("Handlers.cs#Handlers.List".to_string())
                ),
                (
                    "/others".to_string(),
                    Some("Handlers.cs#Others.List".to_string())
                ),
            ]
        );
    }

    #[test]
    fn a_django_view_of_an_imported_module_resolves_when_another_file_has_the_name() {
        let urls = "from django.urls import path\nimport views\n\nurlpatterns = [\n    path(\"items/\", views.detail),\n]\n";
        let views = "def detail(request):\n    return request\n";
        let admin = "def detail(request):\n    return request\n";
        let found = entries(
            Language::Python,
            &[("urls.py", urls), ("views.py", views), ("admin.py", admin)],
        );
        assert_eq!(found[0].symbol.as_deref(), Some("views.py#detail"));
    }

    #[test]
    fn an_express_handler_of_an_imported_module_resolves_when_another_file_has_the_name() {
        let app = "import express from \"express\";\nimport * as users from \"./users\";\n\nconst app = express();\napp.get(\"/users\", users.list);\n";
        let users = "export function list(request, response) {}\n";
        let posts = "export function list(request, response) {}\n";
        let found = entries(
            Language::JavaScript,
            &[("app.js", app), ("users.js", users), ("posts.js", posts)],
        );
        assert_eq!(found[0].symbol.as_deref(), Some("users.js#list"));
    }

    #[test]
    fn rust_main_under_two_cfg_conditions_is_one_entry_point() {
        let twins = "#[cfg(unix)]\nfn main() {\n    run();\n}\n\n#[cfg(not(unix))]\nfn main() {}\n\nfn run() {}\n";
        let found = entries(Language::Rust, &[("main.rs", twins)]);
        let symbols: Vec<Option<&str>> = found.iter().map(|e| e.symbol.as_deref()).collect();
        assert_eq!(symbols, [Some("main.rs#main")]);
        // Two definitions with one condition are not one item.
        let same = "#[cfg(unix)]\nfn main() {}\n\n#[cfg(unix)]\nfn main() {}\n";
        assert_eq!(entries(Language::Rust, &[("main.rs", same)]).len(), 2);
        let plain = "fn main() {}\n\n#[cfg(unix)]\nfn main() {}\n";
        assert_eq!(entries(Language::Rust, &[("main.rs", plain)]).len(), 2);
    }

    #[test]
    fn a_test_function_named_main_in_a_rust_test_module_is_in_test() {
        let source = "#[cfg(test)]\nmod tests {\n    fn main() {}\n}\n";
        let found = entries(Language::Rust, &[("a.rs", source)]);
        assert!(found.iter().all(|entry| entry.in_test));
    }

    #[test]
    fn a_laravel_route_and_a_rails_route_register_actions() {
        let laravel = "<?php\n\nclass UserController {\n    public function show($id) {}\n}\n\nRoute::get('/users/{id}', [UserController::class, 'show'])->middleware('auth');\n";
        let found = entries(Language::Php, &[("routes/web.php", laravel)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "laravel");
        assert_eq!(found[0].route, route("GET", "/users/{id}", &["id"]));
        assert_eq!(found[0].guards, ["auth"]);
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("routes/web.php#UserController.show")
        );

        let rails = "class UsersController\n  before_action :authenticate_user!\n\n  def show\n  end\nend\n";
        let routes =
            "Rails.application.routes.draw do\n  get \"/users/:id\", to: \"users#show\"\nend\n";
        let found = entries(
            Language::Ruby,
            &[("config/routes.rb", routes), ("users_controller.rb", rails)],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "rails");
        assert_eq!(found[0].route, route("GET", "/users/:id", &["id"]));
        assert_eq!(found[0].guards, ["authenticate_user!"]);
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("users_controller.rb#UsersController.show")
        );
    }

    /// The walks of the Rails routes, callbacks, and configuration stop at a depth of 1,000.
    /// Deeper nesting in each of these files does not overflow a small stack.
    #[test]
    fn rails_files_with_deep_nesting() {
        use crate::parser::deep::{nest, on_small_stack};
        let ifs = nest("if a\n", "x\n", "end\n");
        let parentheses = nest("(", "1", ")");
        let routes = format!(
            "Rails.application.routes.draw do\n{ifs}  resources :users\n  get {parentheses}, to: \"users#show\"\nend\n"
        );
        let controller = format!(
            "class UsersController < ApplicationController\n{ifs}  before_action :auth, only: {parentheses}\n  def show; end\nend\n"
        );
        let application = format!(
            "module App\n  class Application < Rails::Application\n{ifs}    config.api_only = {parentheses}\n  end\nend\n"
        );
        let inflections = format!(
            "ActiveSupport::Inflector.inflections(:en) do |inflect|\n{ifs}  inflect.irregular {parentheses}, \"x\"\nend\n"
        );
        on_small_stack(move || {
            rails_routes(
                &routes,
                &[
                    ("controllers/users_controller.rb", controller.as_str()),
                    ("config/application.rb", application.as_str()),
                    ("config/initializers/inflections.rb", inflections.as_str()),
                ],
            );
        });
    }

    fn rails_routes(routes: &str, controllers: &[(&str, &str)]) -> Vec<EntryPoint> {
        let mut files = vec![("config/routes.rb", routes)];
        files.extend_from_slice(controllers);
        entries(Language::Ruby, &files)
            .into_iter()
            .filter(|entry| entry.framework == "rails")
            .collect()
    }

    fn verbs_and_paths(found: &[EntryPoint]) -> Vec<String> {
        found
            .iter()
            .map(|entry| {
                let route = entry.route.as_ref().unwrap();
                format!("{} {}", route.method, route.path_text())
            })
            .collect()
    }

    #[test]
    fn resources_register_the_seven_rails_routes() {
        let routes = "Rails.application.routes.draw do\n  resources :users\nend\n";
        let controller = "class UsersController\n  def index; end\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            [
                "DELETE /users/:id",
                "GET /users",
                "GET /users/:id",
                "GET /users/:id/edit",
                "GET /users/new",
                "PATCH /users/:id",
                "POST /users",
                "PUT /users/:id",
            ]
        );
        let keys: std::collections::BTreeSet<&str> =
            found.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys.len(), found.len());
        let show = found
            .iter()
            .find(|entry| {
                let route = entry.route.as_ref().unwrap();
                route.method == "GET" && route.path_text() == "/users/:id"
            })
            .unwrap();
        assert_eq!(
            show.symbol.as_deref(),
            Some("users_controller.rb#UsersController.show")
        );
        assert_eq!(show.route.as_ref().unwrap().parameters, ["id"]);
    }

    #[test]
    fn a_singular_resource_has_no_index_and_no_id() {
        let routes = "Rails.application.routes.draw do\n  resource :profile\nend\n";
        let controller = "class ProfilesController\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("profiles_controller.rb", controller)]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            [
                "DELETE /profile",
                "GET /profile",
                "GET /profile/edit",
                "GET /profile/new",
                "PATCH /profile",
                "POST /profile",
                "PUT /profile",
            ]
        );
        let show = found
            .iter()
            .find(|entry| entry.route.as_ref().unwrap().method == "GET")
            .unwrap();
        assert!(found.iter().any(|entry| entry.symbol.as_deref()
            == Some("profiles_controller.rb#ProfilesController.show")));
        assert!(show.route.as_ref().unwrap().parameters.is_empty());
    }

    #[test]
    fn only_and_except_limit_the_resource_routes() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:show, :edit]\n  resources :posts, except: %i[destroy new edit]\n  resources :tags, only: :index\n  resources :none, only: []\nend\n";
        let found = rails_routes(routes, &[]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            [
                "GET /posts",
                "GET /posts/:id",
                "GET /tags",
                "GET /users/:id",
                "GET /users/:id/edit",
                "PATCH /posts/:id",
                "POST /posts",
                "PUT /posts/:id",
            ]
        );
    }

    #[test]
    fn nested_resources_member_and_collection_routes_get_their_paths() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:show] do\n    resources :posts, only: [:show]\n    member do\n      get :preview\n      post \"publish\", to: \"users#publish\"\n    end\n    collection do\n      get :search\n    end\n  end\nend\n";
        let controller = "class UsersController\n  def preview; end\n  def search; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            [
                "GET /users/:id",
                "GET /users/:id/preview",
                "GET /users/:user_id/posts/:id",
                "GET /users/search",
                "POST /users/:id/publish",
            ]
        );
        let preview = found
            .iter()
            .find(|entry| {
                entry
                    .route
                    .as_ref()
                    .unwrap()
                    .path_text()
                    .ends_with("/preview")
            })
            .unwrap();
        assert_eq!(
            preview.symbol.as_deref(),
            Some("users_controller.rb#UsersController.preview")
        );
    }

    #[test]
    fn a_namespace_adds_a_path_prefix_and_a_module_to_the_controller() {
        let routes = "Rails.application.routes.draw do\n  namespace :admin do\n    resources :users, only: [:show]\n    get \"/stats\", to: \"stats#show\"\n  end\nend\n";
        let admin = "module Admin\n  class UsersController\n    def show; end\n  end\n  class StatsController\n    def show; end\n  end\nend\n";
        let other = "class UsersController\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/admin.rb", admin),
                ("users_controller.rb", other),
            ],
        );
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(routes, ["GET /admin/stats", "GET /admin/users/:id"]);
        let symbols: Vec<Option<&str>> =
            found.iter().map(|entry| entry.symbol.as_deref()).collect();
        assert!(symbols.contains(&Some("controllers/admin.rb#Admin.UsersController.show")));
        assert!(symbols.contains(&Some("controllers/admin.rb#Admin.StatsController.show")));
    }

    #[test]
    fn scope_adds_a_path_and_a_module() {
        let routes = "Rails.application.routes.draw do\n  scope \"/api\" do\n    get \"/ping\", to: \"health#ping\"\n  end\n  scope module: \"v1\" do\n    get \"/pong\", to: \"health#pong\"\n  end\n  scope \"/api\", module: \"v2\" do\n    resources :items, only: [:index]\n  end\nend\n";
        let controllers = "module V1\n  class HealthController\n    def pong; end\n  end\nend\nclass HealthController\n  def ping; end\nend\n";
        let found = rails_routes(routes, &[("health.rb", controllers)]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(routes, ["GET /api/items", "GET /api/ping", "GET /pong"]);
        let pong = found
            .iter()
            .find(|entry| entry.route.as_ref().unwrap().path_text() == "/pong")
            .unwrap();
        assert_eq!(
            pong.symbol.as_deref(),
            Some("health.rb#V1.HealthController.pong")
        );
        let ping = found
            .iter()
            .find(|entry| entry.route.as_ref().unwrap().path_text() == "/api/ping")
            .unwrap();
        assert_eq!(
            ping.symbol.as_deref(),
            Some("health.rb#HealthController.ping")
        );
    }

    #[test]
    fn root_match_and_the_controller_action_form_register_routes() {
        let routes = "Rails.application.routes.draw do\n  root \"pages#home\"\n  match \"/x\", to: \"pages#x\", via: [:get, :post]\n  get \"/y\", controller: \"pages\", action: \"y\"\n  get \"/z\" => \"pages#z\"\n  match \"/w\", to: \"pages#w\", via: :all\nend\n";
        let controller = "class PagesController\n  def home; end\n  def x; end\n  def y; end\n  def z; end\nend\n";
        let found = rails_routes(routes, &[("pages_controller.rb", controller)]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            ["ANY /w", "GET /", "GET /x", "GET /y", "GET /z", "POST /x"]
        );
        for entry in &found {
            let path = &entry.route.as_ref().unwrap().path_text();
            let wanted = match *path {
                "/" => "home",
                other => other.trim_start_matches('/'),
            };
            if wanted == "w" {
                continue;
            }
            assert_eq!(
                entry.symbol.as_deref(),
                Some(format!("pages_controller.rb#PagesController.{wanted}").as_str()),
                "{path}"
            );
        }
        let rooted = rails_routes(
            "Rails.application.routes.draw do\n  root to: \"pages#home\"\nend\n",
            &[],
        );
        assert_eq!(verbs_and_paths(&rooted), ["GET /"]);
    }

    fn guards_by_path(found: &[EntryPoint]) -> Vec<(String, Vec<String>)> {
        let mut pairs: Vec<(String, Vec<String>)> = found
            .iter()
            .map(|entry| {
                let route = entry.route.as_ref().unwrap();
                (
                    format!("{} {}", route.method, route.path_text()),
                    entry.guards.clone(),
                )
            })
            .collect();
        pairs.sort();
        pairs
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn only_and_except_make_a_guard_apply_to_some_actions() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:index, :show, :edit]\nend\n";
        let controller = "class UsersController\n  before_action :authenticate_user!, only: [:show, :edit]\n  before_action :load_audit, except: :index\n  before_action(:always, :tracked)\n  before_action :multi,\n    only: %i[\n      index\n    ]\n  before_action :conditional, if: :enabled?\n\n  def index; end\n  def show; end\n  def edit; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        assert_eq!(
            guards_by_path(&found),
            [
                (
                    "GET /users".to_string(),
                    strings(&["always", "tracked", "multi"])
                ),
                (
                    "GET /users/:id".to_string(),
                    strings(&["authenticate_user!", "load_audit", "always", "tracked",])
                ),
                (
                    "GET /users/:id/edit".to_string(),
                    strings(&["authenticate_user!", "load_audit", "always", "tracked",])
                ),
            ]
        );
    }

    #[test]
    fn a_guard_name_in_a_comment_or_a_string_is_not_a_guard() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\nend\n";
        let controller = "class UsersController\n  # before_action :commented\n  before_action :real # and :not_this\n  NOTE = \"before_action :quoted\"\n\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        assert_eq!(found[0].guards, ["real"]);
    }

    #[test]
    fn skip_before_action_removes_a_guard_for_the_listed_actions() {
        let routes =
            "Rails.application.routes.draw do\n  resources :pages, only: [:index, :show]\nend\n";
        let base = "class ApplicationController\n  before_action :authenticate_user!\n  before_action :set_locale\nend\n";
        let controller = "class PagesController < ApplicationController\n  skip_before_action :authenticate_user!, only: [:index]\n  skip_before_action :set_locale, except: [:index]\n\n  def index; end\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/pages_controller.rb", controller),
            ],
        );
        assert_eq!(
            guards_by_path(&found),
            [
                ("GET /pages".to_string(), strings(&["set_locale"])),
                (
                    "GET /pages/:id".to_string(),
                    strings(&["authenticate_user!"])
                ),
            ]
        );
    }

    #[test]
    fn guards_of_a_superclass_in_the_repository_come_first() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"orphans#show\"\n  get \"/c\", to: \"loops#show\"\nend\n";
        let base = "class ApplicationController < ActionController::Base\n  before_action :authenticate_user!\nend\n";
        let users = "class UsersController < ApplicationController\n  before_action :load_user\n  def show; end\nend\n";
        let orphan = "class OrphansController < ExternalController\n  before_action :own\n  def show; end\nend\n";
        let loops = "class LoopsController < LoopsController\n  before_action :once\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/loops_controller.rb", loops),
                ("controllers/orphans_controller.rb", orphan),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(
            guards_by_path(&found),
            [
                (
                    "GET /a".to_string(),
                    strings(&["authenticate_user!", "load_user"])
                ),
                ("GET /b".to_string(), strings(&["own"])),
                ("GET /c".to_string(), strings(&["once"])),
            ]
        );
    }

    #[test]
    fn an_absolute_controller_ignores_the_module_of_the_namespace() {
        let routes = "Rails.application.routes.draw do\n  namespace :admin do\n    get \"/x\", to: \"/users#show\"\n    get \"/y\", controller: \"/users\", action: \"show\"\n    get \"/z\", to: \"users#show\"\n  end\nend\n";
        let admin = "module Admin\n  class UsersController\n    before_action :admin_only\n    def show; end\n  end\nend\n";
        let top = "class UsersController\n  before_action :top_only\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/admin.rb", admin),
                ("users_controller.rb", top),
            ],
        );
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.clone(), entry.guards.clone())
        };
        let top_side = (
            Some("users_controller.rb#UsersController.show".to_string()),
            strings(&["top_only"]),
        );
        assert_eq!(by_path("/admin/x"), top_side);
        assert_eq!(by_path("/admin/y"), top_side);
        assert_eq!(
            by_path("/admin/z"),
            (
                Some("controllers/admin.rb#Admin.UsersController.show".to_string()),
                strings(&["admin_only"])
            )
        );
    }

    #[test]
    fn resources_with_two_names_give_distinct_keys() {
        let routes =
            "Rails.application.routes.draw do\n  resources :posts, :comments, only: :index\nend\n";
        let found = rails_routes(routes, &[]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(routes, ["GET /comments", "GET /posts"]);
        let keys: std::collections::BTreeSet<&str> =
            found.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn a_route_without_a_module_matches_only_a_class_without_a_container() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"reports#show\"\nend\n";
        let admin = "module Admin\n  class UsersController\n    before_action :admin_only\n    def show; end\n  end\n  class ReportsController\n    before_action :admin_only\n    def show; end\n  end\nend\n";
        let top = "class UsersController\n  before_action :top_only\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/admin.rb", admin),
                ("users_controller.rb", top),
            ],
        );
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("users_controller.rb#UsersController.show")
        );
        assert_eq!(found[0].guards, ["top_only"]);
        assert_eq!(found[1].symbol, None);
        assert!(found[1].guards.is_empty());
    }

    #[test]
    fn a_list_that_is_not_a_literal_never_claims_a_guard() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\nend\n";
        let controller = "class UsersController\n  before_action :unknown_list, only: ACTIONS\n  before_action :unknown_except, except: ACTIONS\n  before_action :kept\n  before_action :removed\n  skip_before_action :removed, only: ACTIONS\n\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        assert_eq!(found[0].guards, ["kept"]);
    }

    #[test]
    fn a_condition_never_claims_a_guard() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:index, :show, :edit]\nend\n";
        let controller = "class UsersController\n  before_action :if_guard, if: :enabled?\n  before_action :unless_guard, unless: :disabled?\n  before_action :first\n  before_action :second\n  before_action :third\n  skip_before_action :first, if: :flag?\n  skip_before_action :second, only: [:show], unless: :flag?\n  skip_before_action :third, only: LIST, if: :flag?\n\n  def index; end\n  def show; end\n  def edit; end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        assert_eq!(
            guards_by_path(&found),
            [
                ("GET /users".to_string(), strings(&["second"])),
                ("GET /users/:id".to_string(), Vec::new()),
                ("GET /users/:id/edit".to_string(), strings(&["second"])),
            ]
        );
    }

    #[test]
    fn a_nested_route_uses_the_singular_of_active_support() {
        let routes = "Rails.application.routes.draw do\n  resources :news, only: [:index] do\n    resources :comments, only: [:index]\n  end\n  resources :categories, only: [:index] do\n    resources :items, only: [:index]\n  end\n  resources :movies, only: [:index] do\n    resources :actors, only: [:index]\n  end\n  resources :jeans, only: [:index] do\n    resources :sizes, only: [:index]\n  end\n  resources :people, only: [:index] do\n    resources :notes, only: [:index]\n  end\n  resources :statuses, only: [:index] do\n    resources :logs, only: [:index]\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut routes = verbs_and_paths(&found);
        routes.sort();
        assert_eq!(
            routes,
            [
                "GET /categories",
                "GET /categories/:category_id/items",
                "GET /jeans",
                "GET /jeans/:jeans_id/sizes",
                "GET /movies",
                "GET /movies/:movie_id/actors",
                "GET /news",
                "GET /news/:news_id/comments",
                "GET /people",
                "GET /people/:person_id/notes",
                "GET /statuses",
                "GET /statuses/:status_id/logs",
            ]
        );
    }

    #[test]
    fn custom_inflections_make_omen_skip_nested_routes() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:index] do\n    resources :posts, only: [:index]\n    member do\n      get :preview, to: \"users#preview\"\n    end\n  end\nend\n";
        let custom = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.irregular \"user\", \"users\"\nend\n";
        let comments = "# ActiveSupport::Inflector.inflections(:en) do |inflect|\n#   inflect.irregular \"rule\", \"rules\"\n# end\n";
        let nested = "GET /users/:user_id/posts";
        let found = rails_routes(routes, &[("config/initializers/inflections.rb", custom)]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /users", "GET /users/:id/preview"]);
        let found = rails_routes(routes, &[("config/initializers/inflections.rb", comments)]);
        assert!(verbs_and_paths(&found).contains(&nested.to_string()));
    }

    #[test]
    fn only_a_route_in_a_draw_block_is_registered() {
        let routes = "def unused_routes\n  get \"/ghost\", to: \"users#show\"\nend\n\nget \"/outside\", to: \"users#show\"\n\nhandler = -> { get \"/lambda\", to: \"users#show\" }\nother = lambda do\n  get \"/lambda_block\", to: \"users#show\"\nend\nproc { get \"/proc\", to: \"users#show\" }\n\nclass Holder\n  def self.build\n    post \"/singleton\", to: \"users#show\"\n  end\nend\n\nRails.application.routes.draw do\n  get \"/live\", to: \"users#index\"\nend\n\nMyApp::Application.routes.draw do\n  get \"/live_two\", to: \"users#index\"\nend\n";
        let defined = "module MyApp\n  class Application < Rails::Application\n  end\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", defined)]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /live", "GET /live_two"]);
    }

    #[test]
    fn a_conditional_route_is_registered_unless_the_literal_condition_kills_it() {
        let routes = "Rails.application.routes.draw do\n  if Rails.env.development?\n    get \"/dev\", to: \"users#index\"\n  end\n  unless ENV[\"X\"]\n    get \"/unless_live\", to: \"users#index\"\n  end\n  get \"/modifier_live\", to: \"users#index\" if ready?\n  ready? ? get(\"/ternary\", to: \"users#index\") : nil\n  x = ready? ? get(\"/assigned\", to: \"users#index\") : nil\n  if false\n    get \"/dead_false\", to: \"users#index\"\n  end\n  if nil\n    get \"/dead_nil\", to: \"users#index\"\n  end\n  unless true\n    get \"/dead_unless\", to: \"users#index\"\n  end\n  if true\n    get \"/live_true\", to: \"users#index\"\n  else\n    get \"/dead_else\", to: \"users#index\"\n  end\n  if false\n    get \"/dead_if\", to: \"users#index\"\n  elsif ready?\n    get \"/live_elsif\", to: \"users#index\"\n  else\n    get \"/live_else\", to: \"users#index\"\n  end\n  get \"/dead_modifier\", to: \"users#index\" if false\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "GET /dev",
                "GET /live_else",
                "GET /live_elsif",
                "GET /live_true",
                "GET /modifier_live",
                "GET /ternary",
                "GET /unless_live",
            ]
        );
    }

    #[test]
    fn a_reopened_class_counts_a_method_one_time() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :login\nend\n";
        let controller = "class UsersController < ApplicationController\nend\n\nclass UsersController\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/users_controller.rb", controller),
            ],
        );
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("controllers/users_controller.rb#UsersController.show")
        );
        assert_eq!(found[0].guards, ["login"]);
    }

    #[test]
    fn a_reopened_superclass_keeps_the_guards_of_each_opening() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\n  get \"/v\", to: \"users#index\"\nend\n";
        let first =
            "class ApplicationController\n  before_action :login\n  before_action :audit\nend\n";
        let second = "class ApplicationController\n  before_action :locale\n  skip_before_action :audit, only: [:index]\nend\n";
        let users = "class UsersController < ApplicationController\n  before_action :owner\nend\n\nclass UsersController\n  skip_before_action :owner, only: [:index]\n  def show; end\n  def index; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", first),
                ("controllers/application_more.rb", second),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(
            guards_by_path(&found),
            [
                (
                    "GET /u".to_string(),
                    strings(&["login", "audit", "locale", "owner"])
                ),
                ("GET /v".to_string(), strings(&["login", "locale"])),
            ]
        );
    }

    #[test]
    fn a_callback_with_the_same_name_replaces_the_earlier_one() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [:index, :show]\n  resources :posts, only: [:index, :show]\nend\n";
        let base =
            "class ApplicationController\n  before_action :auth\n  before_action :other\nend\n";
        let users = "class UsersController < ApplicationController\n  before_action :auth, only: :show\n  before_action :auth, only: :index\n\n  def show; end\n  def index; end\nend\n";
        let posts = "class PostsController < ApplicationController\n  before_action :auth, only: :index\n\n  def show; end\n  def index; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/posts_controller.rb", posts),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(
            guards_by_path(&found),
            [
                ("GET /posts".to_string(), strings(&["other", "auth"])),
                ("GET /posts/:id".to_string(), strings(&["other"])),
                ("GET /users".to_string(), strings(&["other", "auth"])),
                ("GET /users/:id".to_string(), strings(&["other"])),
            ]
        );
    }

    #[test]
    fn a_callback_in_a_dead_or_a_conditional_branch_of_a_class_body() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :a\n  before_action :b\n  before_action :c\nend\n";
        let users = "class UsersController < ApplicationController\n  if false\n    before_action :dead_false\n  end\n  unless true\n    before_action :dead_unless\n  end\n  if true\n    before_action :live_true\n  else\n    before_action :dead_else\n  end\n  before_action :dead_modifier if nil\n  if Rails.env.production?\n    before_action :maybe\n    skip_before_action :a\n  end\n  case mode\n  when :x\n    skip_before_action :b\n  end\n  skip_before_action :c if false\n\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(found[0].guards, ["c", "live_true"]);
    }

    fn many_openings_of_a_superclass_resolve_in_linear_time_at(
        controller_count: usize,
        opening_count: usize,
    ) -> std::time::Duration {
        let mut routes =
            String::from("Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\n");
        let mut controllers = String::new();
        let mut openings = Vec::new();
        for number in 0..controller_count {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
            controllers.push_str(&format!(
                "class C{number}Controller < ApplicationController\n  def show; end\nend\n"
            ));
        }
        for number in 0..opening_count {
            openings.push((
                format!("controllers/a{number:06}.rb"),
                format!(
                    "class ApplicationController\n  before_action :g{number}\nend\n\nclass UsersController < ApplicationController\n  before_action :u{number}\n{}end\n",
                    if number == 0 { "  def show; end\n" } else { "" }
                ),
            ));
        }
        routes.push_str("end\n");
        let mut files: Vec<(&str, &str)> = vec![
            ("config/routes.rb", routes.as_str()),
            ("controllers/classes.rb", controllers.as_str()),
        ];
        files.extend(
            openings
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str())),
        );
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), controller_count + 1);
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        let users = found
            .iter()
            .find(|entry| entry.route.as_ref().unwrap().path_text() == "/u")
            .unwrap();
        assert_eq!(users.guards.len(), 2 * opening_count);
        elapsed
    }

    #[test]
    fn many_openings_of_a_superclass_give_each_guard() {
        many_openings_of_a_superclass_resolve_in_linear_time_at(20, 20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_openings_of_a_superclass_resolve_in_linear_time() {
        let many_routes = many_openings_of_a_superclass_resolve_in_linear_time_at(10_000, 1_000);
        let many_openings = many_openings_of_a_superclass_resolve_in_linear_time_at(10, 10_000);
        assert!(many_routes.as_secs() < 30, "{many_routes:?}");
        assert!(many_openings.as_secs() < 30, "{many_openings:?}");
    }

    #[test]
    fn a_callback_in_a_block_is_conditional_and_one_in_a_method_is_ignored() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :a\n  before_action :b\nend\n";
        let users = "class UsersController < ApplicationController\n  0.times do\n    before_action :ghost\n  end\n  [1].each { before_action :ghost_two }\n  3.times do\n    skip_before_action :a\n  end\n\n  def self.configure\n    skip_before_action :b\n    before_action :in_method\n  end\n\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(found[0].guards, ["b"]);
    }

    #[test]
    fn prepend_puts_a_callback_at_the_front_of_the_chain() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\n  get \"/p\", to: \"posts#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :base\n  before_action :x\nend\n";
        let users = "class UsersController < ApplicationController\n  before_action :child, prepend: true\n  prepend_before_action :first\n  before_action :x, prepend: true\n  def show; end\nend\n";
        let posts = "class PostsController < ApplicationController\n  before_action :late, prepend: LATE\n  before_action :plain, prepend: false\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", base),
                ("controllers/posts_controller.rb", posts),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(
            guards_by_path(&found),
            [
                ("GET /p".to_string(), strings(&["base", "x", "plain"])),
                (
                    "GET /u".to_string(),
                    strings(&["x", "first", "child", "base"])
                ),
            ]
        );
    }

    fn many_actions_with_many_empty_openings_resolve_in_linear_time_at(
        count: usize,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut methods = String::from("class UsersController\n");
        let mut openings = Vec::new();
        for number in 0..count {
            routes.push_str(&format!("  get \"/r{number}\", to: \"users#a{number}\"\n"));
            methods.push_str(&format!("  def a{number}; end\n"));
            openings.push((
                format!("controllers/o{number:06}.rb"),
                String::from("class UsersController\nend\n"),
            ));
        }
        routes.push_str("end\n");
        methods.push_str("end\n");
        let mut files: Vec<(&str, &str)> = vec![
            ("config/routes.rb", routes.as_str()),
            ("controllers/methods.rb", methods.as_str()),
        ];
        files.extend(
            openings
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str())),
        );
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        elapsed
    }

    #[test]
    fn many_actions_with_many_empty_openings_resolve_each_action() {
        many_actions_with_many_empty_openings_resolve_in_linear_time_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_actions_with_many_empty_openings_resolve_in_linear_time() {
        let elapsed = many_actions_with_many_empty_openings_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn a_deep_chain_of_superclasses_resolves_in_linear_time_at(
        depth: usize,
    ) -> std::time::Duration {
        let routes = format!(
            "Rails.application.routes.draw do\n  get \"/deep\", to: \"k{}#show\"\nend\n",
            depth - 1
        );
        let mut classes = String::new();
        for number in 0..depth {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < K{}Controller", number - 1)
            };
            classes.push_str(&format!(
                "class K{number}Controller{parent}\n  before_action :g{number}\n  def show; end\nend\n"
            ));
        }
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].guards.len(), depth);
        elapsed
    }

    #[test]
    fn a_long_chain_of_superclasses_does_not_overflow_the_stack() {
        a_deep_chain_of_superclasses_resolves_in_linear_time_at(4_000);
    }

    #[test]
    fn a_chain_of_superclasses_gives_each_guard() {
        a_deep_chain_of_superclasses_resolves_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_deep_chain_of_superclasses_resolves_in_linear_time() {
        let elapsed = a_deep_chain_of_superclasses_resolves_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn only_a_rails_or_an_engine_routes_object_draws_routes() {
        let routes = "fake_routes = Object.new\ndef fake_routes.draw; end\nfake_routes.draw do\n  get \"/ghost\", to: \"users#show\"\nend\n\nroutes.draw do\n  get \"/bare\", to: \"users#show\"\nend\n\nOther.routes.draw do\n  get \"/other\", to: \"users#show\"\nend\n\nRails.application.routes.draw do\n  get \"/app\", to: \"users#show\"\nend\n\nMyApp::Application.routes.draw do\n  get \"/namespaced\", to: \"users#show\"\nend\n\nMyEngine::Engine.routes.draw do\n  get \"/engine\", to: \"users#show\"\nend\n";
        let defined = "module MyApp\n  class Application < Rails::Application\n  end\nend\n\nmodule MyEngine\n  class Engine < ::Rails::Engine\n  end\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", defined)]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /app", "GET /namespaced"]);
    }

    #[test]
    fn only_a_public_instance_method_is_an_action() {
        let actions = [
            "pub", "single", "meta", "hidden", "guarded", "again", "later", "inline", "listed",
        ];
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for action in actions {
            routes.push_str(&format!("  get \"/{action}\", to: \"users#{action}\"\n"));
        }
        routes.push_str(
            "  get \"/odd\", to: \"odd#show\"\n  get \"/plain\", to: \"plain#show\"\nend\n",
        );
        let controller = "class UsersController\n  before_action :auth\n  def pub; end\n  def self.single; end\n  class << self\n    def meta; end\n  end\n  private\n  def hidden; end\n  protected\n  def guarded; end\n  public\n  def again; end\n  def later; end\n  private :later\n  private def inline; end\n  def listed; end\nend\n\nclass UsersController\n  public :hidden\nend\n\nclass OddController\n  private(*LIST)\n  def show; end\nend\n\nclass PlainController\n  def show; end\nend\n";
        let found = rails_routes(&routes, &[("controllers/users.rb", controller)]);
        let symbol_of = |action: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == format!("/{action}"))
                .unwrap()
                .symbol
                .clone()
        };
        let has_symbol = |action: &str| symbol_of(action).is_some();
        assert!(has_symbol("pub"));
        assert!(has_symbol("again"));
        assert!(has_symbol("listed"));
        assert!(!has_symbol("single"));
        assert!(!has_symbol("meta"));
        assert!(has_symbol("hidden"));
        assert!(!has_symbol("guarded"));
        assert!(!has_symbol("later"));
        assert!(!has_symbol("inline"));
        assert_eq!(symbol_of("odd"), None);
        assert!(has_symbol("plain"));
    }

    #[test]
    fn an_acronym_after_an_inflections_block_of_another_locale_is_read() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let inflections = "ActiveSupport::Inflector.inflections(:fr) do |inflect|\n  inflect.plural \"x\", \"y\"\nend\nActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.acronym \"API\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", inflections),
            ],
        );
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("controllers/upper.rb#API.UsersController.show")
        );
    }

    #[test]
    fn a_registered_acronym_changes_the_controller_class() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let acronym = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.acronym \"API\"\nend\n";
        let both = [
            ("controllers/upper.rb", upper),
            ("controllers/lower.rb", lower),
        ];
        let found = rails_routes(
            routes,
            &[
                both[0],
                both[1],
                ("config/initializers/inflections.rb", acronym),
            ],
        );
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("controllers/upper.rb#API.UsersController.show")
        );
        assert_eq!(found[0].guards, ["upper"]);
        let found = rails_routes(routes, &both);
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("controllers/lower.rb#Api.UsersController.show")
        );
        assert_eq!(found[0].guards, ["lower"]);
        let unreadable =
            "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.acronym NAME\nend\n";
        let found = rails_routes(
            routes,
            &[
                both[0],
                both[1],
                ("config/initializers/inflections.rb", unreadable),
            ],
        );
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());
    }

    fn many_callbacks_for_other_actions_resolve_in_linear_time_at(
        count: usize,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut controller = String::from("class UsersController\n");
        for number in 0..count {
            routes.push_str(&format!("  get \"/r{number}\", to: \"users#a{number}\"\n"));
            controller.push_str(&format!(
                "  before_action :g{number}, only: :b{number}\n  def a{number}; end\n"
            ));
        }
        routes.push_str("end\n");
        controller.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("users_controller.rb", controller.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found
            .iter()
            .all(|entry| entry.symbol.is_some() && entry.guards.is_empty()));
        elapsed
    }

    #[test]
    fn many_callbacks_for_other_actions_add_no_guard() {
        many_callbacks_for_other_actions_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_callbacks_for_other_actions_resolve_in_linear_time() {
        let elapsed = many_callbacks_for_other_actions_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn many_engines_resolve_in_linear_time_at(count: usize) -> std::time::Duration {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut controllers = String::new();
        for number in 0..count {
            files.push((
                format!("engines/e{number:06}/config/routes.rb"),
                format!(
                    "E{number}::Engine.routes.draw do\n  get \"/x\", to: \"c{number}#show\"\nend\n"
                ),
            ));
            controllers.push_str(&format!(
                "module E{number}\n  class Engine < ::Rails::Engine\n  end\nend\nclass C{number}Controller\n  before_action :g\n  def show; end\nend\n"
            ));
        }
        files.push((String::from("controllers.rb"), controllers));
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let (found, elapsed) = entries_lean(Language::Ruby, &borrowed);
        assert!(found.is_empty());
        elapsed
    }

    #[test]
    fn many_engines_give_no_entry() {
        many_engines_resolve_in_linear_time_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_engines_resolve_in_linear_time() {
        let elapsed = many_engines_resolve_in_linear_time_at(5_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn a_namespace_or_scope_option_that_omen_does_not_model_skips_the_block() {
        let routes = "Rails.application.routes.draw do\n  namespace :admin, only: :index do\n    resources :users\n  end\n  scope \"/s\", except: :index do\n    get \"/in_scope\", to: \"a#b\"\n  end\n  namespace :adm, path: \"p\", as: \"x\", module: \"m\" do\n    get \"/ok\", to: \"a#b\"\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /p/ok"]);
    }

    #[test]
    fn a_non_public_action_has_no_symbol_and_no_guard() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"users#edit\"\n  get \"/c\", to: \"posts#show\"\n  get \"/d\", to: \"inherits#hidden\"\n  get \"/e\", to: \"implicit#page\"\nend\n";
        let users = "class UsersController\n  before_action :auth\n  def edit; end\n  private\n  def show; end\nend\n\nclass PostsController < ApplicationController\n  before_action :auth\n  def show; end\nend\n\nclass InheritsController < ParentController\n  before_action :child\nend\n\nclass ParentController\n  before_action :base\n  private\n  def hidden; end\nend\n\nclass ImplicitController\n  before_action :implicit\nend\n";
        let found = rails_routes(routes, &[("controllers/users.rb", users)]);
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.is_some(), entry.guards.clone())
        };
        assert_eq!(by_path("/a"), (false, Vec::new()));
        assert_eq!(by_path("/b"), (true, strings(&["auth"])));
        assert_eq!(by_path("/c"), (true, strings(&["auth"])));
        assert_eq!(by_path("/d"), (false, Vec::new()));
        assert_eq!(by_path("/e"), (false, strings(&["implicit"])));
    }

    #[test]
    fn visibility_applies_across_the_openings_of_a_class() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"posts#show\"\n  get \"/c\", to: \"pages#show\"\nend\n";
        let users = "class UsersController\n  before_action :auth\n  def show; end\nend\n\nclass UsersController\n  private :show\nend\n";
        let posts_first = "class PostsController\n  before_action :auth\n  def show; end\nend\n";
        let posts_second = "class PostsController\n  private :show\nend\n";
        let pages = "class PagesController\n  before_action :auth\n  def show; end\nend\n\nclass PagesController\n  private :show\n  public :show\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/a_users.rb", users),
                ("controllers/b_posts.rb", posts_first),
                ("controllers/c_posts.rb", posts_second),
                ("controllers/d_pages.rb", pages),
            ],
        );
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.is_some(), entry.guards.clone())
        };
        assert_eq!(by_path("/a"), (false, Vec::new()));
        assert_eq!(by_path("/b"), (false, Vec::new()));
        assert_eq!(by_path("/c"), (true, strings(&["auth"])));
    }

    #[test]
    fn an_acronym_in_a_dead_branch_or_a_conditional_is_handled() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let files = |initializer: &'static str| {
            vec![
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", initializer),
            ]
        };
        let dead = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  if false\n    inflect.acronym \"API\"\n  end\nend\n";
        let found = rails_routes(routes, &files(dead));
        assert_eq!(found[0].guards, ["lower"]);
        let live = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  if true\n    inflect.acronym \"API\"\n  end\nend\n";
        let found = rails_routes(routes, &files(live));
        assert_eq!(found[0].guards, ["upper"]);
        let conditional = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  if ENV[\"X\"]\n    inflect.acronym \"API\"\n  end\nend\n";
        let found = rails_routes(routes, &files(conditional));
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());
        let looped = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  %w[API].each do |word|\n    inflect.acronym word\n  end\nend\n";
        let found = rails_routes(routes, &files(looped));
        assert_eq!(found[0].symbol, None);
    }

    #[test]
    fn a_routes_object_needs_a_class_that_the_repository_defines() {
        let routes = "Fake::Engine.routes.draw do\n  get \"/ghost\", to: \"a#b\"\nend\n\nMyEngine::Engine.routes.draw do\n  get \"/engine\", to: \"a#b\"\nend\n\nCompact::Engine.routes.draw do\n  get \"/compact\", to: \"a#b\"\nend\n\nWrong::Engine.routes.draw do\n  get \"/wrong\", to: \"a#b\"\nend\n\nMyApp::Application.routes.draw do\n  get \"/app\", to: \"a#b\"\nend\n\nNoApp::Application.routes.draw do\n  get \"/no_app\", to: \"a#b\"\nend\n";
        let classes = "module MyEngine\n  class Engine < ::Rails::Engine\n  end\nend\n\nclass Compact::Engine < Rails::Engine\nend\n\nclass Wrong::Engine < Object\nend\n\nmodule MyApp\n  class Application < Rails::Application\n  end\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", classes)]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /app"]);
    }

    #[test]
    fn a_singular_resource_uses_the_plural_of_active_support() {
        let routes = "Rails.application.routes.draw do\n  resource :wife, only: :show\n  resource :fish, only: :show\n  resource :child, only: :show\n  resource :category, only: :show\nend\n";
        let controllers = "class WivesController\n  def show; end\nend\nclass WifesController\n  def show; end\nend\nclass FishController\n  def show; end\nend\nclass ChildrenController\n  def show; end\nend\nclass CategoriesController\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/c.rb", controllers)]);
        let symbols: Vec<String> = found
            .iter()
            .map(|entry| entry.symbol.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            symbols,
            [
                "controllers/c.rb#WivesController.show",
                "controllers/c.rb#FishController.show",
                "controllers/c.rb#ChildrenController.show",
                "controllers/c.rb#CategoriesController.show",
            ]
        );
        let custom = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.irregular \"wife\", \"wifes\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/c.rb", controllers),
                ("config/initializers/inflections.rb", custom),
            ],
        );
        assert!(found.iter().all(|entry| entry.symbol.is_none()));
    }

    #[test]
    fn a_path_is_normalized_as_journey_does() {
        let routes = "Rails.application.routes.draw do\n  get \"/health/\", to: \"a#b\"\n  get \"//a//b/\", to: \"a#b\"\n  get \"plain\", to: \"a#b\"\n  scope \"/api/\" do\n    get \"/v1/\", to: \"a#b\"\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            ["GET /a/b", "GET /api/v1", "GET /health", "GET /plain"]
        );
    }

    #[test]
    fn nesting_and_canonical_actions_follow_the_rails_mapper() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [] do\n    scope \"x\" do\n      get :y\n    end\n    root \"pages#home\"\n    member do\n      get :show\n      get :custom\n      get \"my-action\"\n    end\n    collection do\n      get :index\n    end\n  end\n  resources :posts, only: [] do\n    get :search\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "GET /posts/:post_id/search",
                "GET /users",
                "GET /users",
                "GET /users/:id",
                "GET /users/:id/custom",
                "GET /users/:id/my-action",
                "GET /x/users/:user_id/y",
            ]
        );
    }

    fn many_children_of_one_parent_resolve_in_linear_time_at(count: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut base = String::from("class ApplicationController\n");
        let mut children = String::new();
        for number in 0..count {
            base.push_str(&format!("  before_action :p{number}, only: :never\n"));
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
            children.push_str(&format!(
                "class C{number}Controller < ApplicationController\n  before_action :own{number}, only: :never\n  def show; end\nend\n"
            ));
        }
        base.push_str("end\n");
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/application.rb", base.as_str()),
            ("controllers/children.rb", children.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found
            .iter()
            .all(|entry| entry.symbol.is_some() && entry.guards.is_empty()));
        elapsed
    }

    #[test]
    fn many_children_of_one_parent_share_its_callbacks() {
        many_children_of_one_parent_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_children_of_one_parent_resolve_in_linear_time() {
        let elapsed = many_children_of_one_parent_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn as_changes_the_name_of_the_nested_parameter() {
        let routes = "Rails.application.routes.draw do\n  resources :posts, as: :articles, only: [] do\n    resources :comments, only: :index\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /posts/:article_id/comments"]);
    }

    #[test]
    fn a_path_is_escaped_as_the_mapper_escapes_it() {
        let routes = "Rails.application.routes.draw do\n  get \"/%ab/x\", to: \"a#b\"\n  get \"/with space\", to: \"a#b\"\n  get \"/caf\u{e9}\", to: \"a#b\"\n  get \"/a{b}\", to: \"a#b\"\n  scope \"/p%\" do\n    get \"/q\", to: \"a#b\"\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "GET /%25ab/x",
                "GET /a%7Bb%7D",
                "GET /caf%C3%A9",
                "GET /p%25/q",
                "GET /with%20space",
            ]
        );
    }

    #[test]
    fn api_only_removes_the_new_and_edit_routes() {
        let routes = "Rails.application.routes.draw do\n  resources :users\n  resource :profile\n  resources :posts, only: [:new, :show]\nend\n";
        let api = "module App\n  class Application < Rails::Application\n    config.api_only = true\n  end\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", api)]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "DELETE /profile",
                "DELETE /users/:id",
                "GET /posts/:id",
                "GET /posts/new",
                "GET /profile",
                "GET /users",
                "GET /users/:id",
                "PATCH /profile",
                "PATCH /users/:id",
                "POST /profile",
                "POST /users",
                "PUT /profile",
                "PUT /users/:id",
            ]
        );
        let off = "config.api_only = false\nif false\n  config.api_only = true\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", off)]);
        assert!(verbs_and_paths(&found).contains(&"GET /users/new".to_string()));
        let unknown = "config.api_only = ENV[\"API\"]\n";
        let found = rails_routes(routes, &[("config/application.rb", unknown)]);
        assert!(found.is_empty());
        let conditional = "if ENV[\"API\"]\n  config.api_only = true\nend\n";
        let found = rails_routes(routes, &[("config/application.rb", conditional)]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_visibility_call_in_a_child_hides_an_inherited_action() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"posts#show\"\n  get \"/c\", to: \"pages#show\"\n  get \"/d\", to: \"tags#mixin\"\nend\n";
        let controllers = "class ParentController\n  def show; end\n  private\n  def hidden_show; end\nend\n\nclass UsersController < ParentController\n  before_action :auth\n  private :show\nend\n\nclass PostsController < ParentController\n  before_action :auth\n  def show; end\nend\n\nclass PagesController < ParentController\n  before_action :auth\n  public :hidden_show\nend\n\nclass TagsController\n  before_action :auth\n  protected :mixin\nend\n";
        let found = rails_routes(routes, &[("controllers/c.rb", controllers)]);
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.is_some(), entry.guards.clone())
        };
        assert_eq!(by_path("/a"), (false, Vec::new()));
        assert_eq!(by_path("/b"), (true, strings(&["auth"])));
        assert_eq!(by_path("/c"), (false, strings(&["auth"])));
        assert_eq!(by_path("/d"), (false, Vec::new()));
    }

    #[test]
    fn only_the_inflection_object_registers_an_acronym() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let other = "Other.acronym \"API\"\nOther.irregular \"a\", \"b\"\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/other.rb", other),
            ],
        );
        assert_eq!(found[0].guards, ["lower"]);
        let direct = "ActiveSupport::Inflector.inflections.acronym \"API\"\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/direct.rb", direct),
            ],
        );
        assert_eq!(found[0].guards, ["upper"]);
    }

    fn many_nested_blocks_resolve_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for number in 0..depth {
            routes.push_str(&format!(
                "constraints(host: /./) do\n  get \"/r{number}\", to: \"a#b\"\n"
            ));
        }
        for _ in 0..depth {
            routes.push_str("end\n");
        }
        routes.push_str("end\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        elapsed
    }

    #[test]
    fn many_nested_blocks_each_give_their_route() {
        many_nested_blocks_resolve_in_linear_time_at(30);
    }

    #[test]
    fn deeply_nested_blocks_do_not_overflow_the_stack() {
        many_nested_blocks_resolve_in_linear_time_at(1_500);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_nested_blocks_resolve_in_linear_time() {
        let elapsed = many_nested_blocks_resolve_in_linear_time_at(2_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn a_chain_without_the_action_resolves_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in 0..depth {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!("class C{number}Controller{parent}\nend\n"));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        elapsed
    }

    #[test]
    fn a_chain_without_the_action_gives_each_route() {
        a_chain_without_the_action_resolves_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_without_the_action_resolves_in_linear_time() {
        let elapsed = a_chain_without_the_action_resolves_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn api_only_belongs_to_the_route_set_that_owns_it() {
        let app_routes =
            "Rails.application.routes.draw do\n  resources :users, only: [:new]\nend\n";
        let engine_routes =
            "Blog::Engine.routes.draw do\n  resources :posts, only: [:new, :index]\nend\n";
        let engine =
            "module Blog\n  class Engine < Rails::Engine\n    config.api_only = true\n  end\nend\n";
        let paths = |files: &[(&str, &str)]| {
            let mut paths = verbs_and_paths(
                &entries(Language::Ruby, files)
                    .into_iter()
                    .filter(|entry| entry.framework == "rails")
                    .collect::<Vec<_>>(),
            );
            paths.sort();
            paths
        };
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("lib/blog/engine.rb", engine),
        ]);
        assert!(found.is_empty());
        let stray = "config.api_only = true\n";
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("lib/blog/engine.rb", engine),
            ("lib/stray.rb", stray),
        ]);
        assert!(found.is_empty());
    }

    #[test]
    fn only_the_inflector_of_active_support_registers_an_acronym() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let fake = "module Fake\n  module Inflector\n    def self.inflections\n      yield Object.new\n    end\n  end\nend\n\nFake::Inflector.inflections do |inflect|\n  inflect.acronym \"API\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/fake.rb", fake),
            ],
        );
        assert_eq!(found[0].guards, ["lower"]);
        let real = "ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.acronym \"API\"\nend\n";
        let shadow = "module ActiveSupport\n  module Inflector\n    def self.inflections\n      yield Object.new\n    end\n  end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/real.rb", real),
                ("lib/shadow.rb", shadow),
            ],
        );
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());
        let rooted = "::ActiveSupport::Inflector.inflections(:en) do |inflect|\n  inflect.acronym \"API\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/real.rb", rooted),
            ],
        );
        assert_eq!(found[0].guards, ["upper"]);
    }

    #[test]
    fn a_ruby_string_is_evaluated_before_the_path_is_escaped() {
        let routes = "Rails.application.routes.draw do\n  get \"/caf\\u00E9\", to: \"a#b\"\n  get 'q\\nr', to: \"a#b\"\n  get \"/t\\tab\", to: \"a#b\"\n  get \"/x\\x41\", to: \"a#b\"\n  get \"/o\\101\", to: \"a#b\"\n  get \"/u\\u{41 42}\", to: \"a#b\"\n  get '/s\\\\t', to: \"a#b\"\n  get \"/i#{1}\", to: \"a#b\"\n  get \"/m\\M-a\", to: \"a#b\"\n  get %q(/pct), to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "GET /caf%C3%A9",
                "GET /oA",
                "GET /q%5Cnr",
                "GET /s%5Ct",
                "GET /t%09ab",
                "GET /uAB",
                "GET /xA",
            ]
        );
    }

    fn many_nested_conditions_resolve_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for number in 0..depth {
            routes.push_str(&format!("if true\n  get \"/r{number}\", to: \"a#b\"\n"));
        }
        for _ in 0..depth {
            routes.push_str("end\n");
        }
        routes.push_str("end\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        elapsed
    }

    #[test]
    fn many_nested_conditions_each_give_their_route() {
        many_nested_conditions_resolve_in_linear_time_at(1_500);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_nested_conditions_resolve_in_linear_time() {
        let elapsed = many_nested_conditions_resolve_in_linear_time_at(2_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn api_only_with_a_constant_receiver_belongs_to_that_route_set() {
        let app_routes =
            "Rails.application.routes.draw do\n  resources :users, except: [:index, :create, :show, :update, :destroy]\nend\n";
        let engine_routes = "Blog::Engine.routes.draw do\n  resources :posts, except: [:index, :create, :show, :update, :destroy]\nend\n";
        let classes = "module Blog\n  class Engine < Rails::Engine\n  end\nend\n";
        let paths = |files: &[(&str, &str)]| {
            let mut paths = verbs_and_paths(
                &entries(Language::Ruby, files)
                    .into_iter()
                    .filter(|entry| entry.framework == "rails")
                    .collect::<Vec<_>>(),
            );
            paths.sort();
            paths
        };
        let top = "Blog::Engine.config.api_only = true\n";
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("config/application.rb", classes),
            ("config/engine.rb", top),
        ]);
        assert!(found.is_empty());
        let application = "Rails.application.config.api_only = true\n";
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("config/application.rb", classes),
            ("config/app.rb", application),
        ]);
        assert!(found.is_empty());
        let unknown = "something.config.api_only = true\n";
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("config/application.rb", classes),
            ("config/other.rb", unknown),
        ]);
        assert!(found.is_empty());
    }

    #[test]
    fn only_a_plain_statement_makes_a_route() {
        let routes = "Rails.application.routes.draw do\n  false && get(\"/and\", to: \"a#b\")\n  true || get(\"/or\", to: \"a#b\")\n  x = get(\"/assigned\", to: \"a#b\")\n  foo(get(\"/argument\", to: \"a#b\"))\n  begin\n    get \"/begin\", to: \"a#b\"\n  end\n  (get \"/paren\", to: \"a#b\")\n  get \"/live\", to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /live"]);
    }

    #[test]
    fn a_rebound_inflection_parameter_makes_the_inflections_unreadable() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let reassigned = "ActiveSupport::Inflector.inflections do |inflect|\n  inflect = Fake.new\n  inflect.acronym \"API\"\nend\n";
        let shadowed = "ActiveSupport::Inflector.inflections do |inflect|\n  [1].each do |inflect|\n    inflect.acronym \"API\"\n  end\nend\n";
        let pattern = "ActiveSupport::Inflector.inflections do |inflect|\n  case Fake.new\n  in inflect\n  end\n  inflect.acronym \"API\"\nend\n";
        let argument = "ActiveSupport::Inflector.inflections do |inflect|\n  helper(inflect)\n  inflect.acronym \"API\"\nend\n";
        for source in [reassigned, shadowed, pattern, argument] {
            let found = rails_routes(
                routes,
                &[
                    ("controllers/upper.rb", upper),
                    ("controllers/lower.rb", lower),
                    ("config/initializers/inflections.rb", source),
                ],
            );
            assert_eq!(found[0].symbol, None);
            assert!(found[0].guards.is_empty());
        }
    }

    #[test]
    fn a_callback_that_is_not_a_plain_statement_makes_the_guards_unknown() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"users#show\"\n  get \"/b\", to: \"posts#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :base\nend\n";
        let users = "class UsersController < ApplicationController\n  x = (skip_before_action :base)\n  def show; end\nend\n";
        let posts = "class PostsController < ApplicationController\n  before_action :own\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/a.rb", base),
                ("controllers/b.rb", users),
                ("controllers/c.rb", posts),
            ],
        );
        assert_eq!(found[0].guards, ["base"]);
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::UnreadableCallback)
        );
        assert!(!found[0].guards_complete);
        assert_eq!(found[1].guards, ["base", "own"]);
        assert!(found[1].guards_complete);
    }

    #[test]
    fn a_hash_rocket_key_keeps_its_last_value() {
        let routes = "Rails.application.routes.draw do\n  get \"/x\" => \"users#show\", \"/x\" => \"posts#show\"\nend\n";
        let users = "class UsersController\n  before_action :users\n  def show; end\nend\n";
        let posts = "class PostsController\n  before_action :posts\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[("controllers/u.rb", users), ("controllers/p.rb", posts)],
        );
        assert_eq!(found[0].guards, ["posts"]);
    }

    #[test]
    fn an_octal_escape_is_a_byte_modulo_256() {
        let routes = "Rails.application.routes.draw do\n  get \"/\\400\", to: \"a#b\"\n  get \"/b\\177\", to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /%00", "GET /b%7F"]);
    }

    fn many_literal_when_branches_resolve_in_linear_time_at(count: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n  case true\n");
        for number in 0..count {
            routes.push_str(&format!(
                "  when false\n    get \"/r{number}\", to: \"a#b\"\n"
            ));
        }
        routes.push_str("  when true\n    get \"/live\", to: \"a#b\"\n  end\nend\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), 1);
        elapsed
    }

    #[test]
    fn many_literal_when_branches_keep_the_branch_that_runs() {
        many_literal_when_branches_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_literal_when_branches_resolve_in_linear_time() {
        let elapsed = many_literal_when_branches_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn a_chain_of_never_callbacks_resolves_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in (0..depth).rev() {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
        }
        for number in 0..depth {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  before_action :g{number}, only: :never\n  def show; end\nend\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        assert!(found.iter().all(|entry| entry.guards.is_empty()));
        elapsed
    }

    #[test]
    fn a_chain_of_never_callbacks_gives_no_guard() {
        a_chain_of_never_callbacks_resolves_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_never_callbacks_resolves_in_linear_time() {
        let elapsed = a_chain_of_never_callbacks_resolves_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn a_chain_of_show_callbacks_resolves_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in (0..depth).rev() {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
        }
        for number in 0..depth {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  before_action :auth, only: :show\n  def show; end\nend\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        assert!(found.iter().all(|entry| entry.guards == ["auth"]));
        elapsed
    }

    #[test]
    fn a_chain_of_show_callbacks_gives_one_guard_to_each_route() {
        a_chain_of_show_callbacks_resolves_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_show_callbacks_resolves_in_linear_time() {
        let elapsed = a_chain_of_show_callbacks_resolves_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn a_controller_string_with_an_empty_or_unknown_segment_gives_no_controller() {
        let routes = "Rails.application.routes.draw do\n  get \"/ok\", to: \"users#show\"\n  get \"/a\", to: \"users/#show\"\n  get \"/b\", to: \"/users/#show\"\n  get \"/c\", to: \"users//x#show\"\n  get \"/d\", to: \"Users#show\"\n  get \"/e\", controller: \"users/\", action: \"show\"\n  scope module: \"m//n\" do\n    get \"/f\", to: \"users#show\"\n  end\nend\n";
        let users = "class UsersController\n  before_action :auth\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/users.rb", users)]);
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.is_some(), entry.guards.clone())
        };
        assert_eq!(by_path("/ok"), (true, strings(&["auth"])));
        for path in ["/a", "/b", "/c", "/d", "/e", "/f"] {
            assert_eq!(by_path(path), (false, Vec::new()), "{path}");
        }
    }

    #[test]
    fn only_a_modelled_statement_makes_the_inflections_readable() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let others = [
            "binding.local_variable_set(:inflect, Fake.new)",
            "send(:instance_variable_set, :@x, 1)",
            "eval(\"inflect = Fake.new\")",
            "def helper; end",
            "puts 1",
            "if ENV[\"X\"]\n    puts 2\n  end",
        ];
        for other in others {
            let source = format!(
                "ActiveSupport::Inflector.inflections do |inflect|\n  {other}\n  inflect.acronym \"API\"\nend\n"
            );
            let found = rails_routes(
                routes,
                &[
                    ("controllers/upper.rb", upper),
                    ("controllers/lower.rb", lower),
                    ("config/initializers/inflections.rb", source.as_str()),
                ],
            );
            assert_eq!(found[0].symbol, None, "{other}");
        }
        let fine = "ActiveSupport::Inflector.inflections do |inflect|\n  # a comment\n  inflect.acronym \"API\"\n  if true\n    inflect.irregular \"a\", \"b\"\n  end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", fine),
            ],
        );
        assert_eq!(found[0].guards, ["upper"]);
        let stored = "inflections = ActiveSupport::Inflector.inflections(:en)\ninflections.acronym \"API\"\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", stored),
            ],
        );
        assert_eq!(found[0].symbol, None);
    }

    fn many_prepend_callbacks_resolve_in_linear_time_at(count: usize) -> std::time::Duration {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let mut controller = String::from("class UsersController\n");
        for number in 0..count {
            controller.push_str(&format!(
                "  prepend_before_action :g{number}, only: :show\n"
            ));
        }
        controller.push_str("  def show; end\nend\n");
        let files = [
            ("config/routes.rb", routes),
            ("controllers/users.rb", controller.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), 1);
        let expected: Vec<String> = (0..count).rev().map(|n| format!("g{n}")).collect();
        assert_eq!(found[0].guards, expected);
        elapsed
    }

    #[test]
    fn many_prepend_callbacks_give_the_guards_in_reverse_order() {
        many_prepend_callbacks_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_prepend_callbacks_resolve_in_linear_time() {
        let elapsed = many_prepend_callbacks_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn only_the_default_locale_of_the_inflections_gives_class_names() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let guards_of = |source: &str| {
            rails_routes(
                routes,
                &[
                    ("controllers/upper.rb", upper),
                    ("controllers/lower.rb", lower),
                    ("config/initializers/inflections.rb", source),
                ],
            )
            .remove(0)
        };
        let other = "ActiveSupport::Inflector.inflections(:es) do |i|\n  i.acronym \"API\"\nend\n";
        assert_eq!(guards_of(other).guards, ["lower"]);
        let chained = "ActiveSupport::Inflector.inflections(:es).acronym \"API\"\n";
        assert_eq!(guards_of(chained).guards, ["lower"]);
        for locale in ["", "(:en)", "(\"en\")"] {
            let source = format!(
                "ActiveSupport::Inflector.inflections{locale} do |i|\n  i.acronym \"API\"\nend\n"
            );
            assert_eq!(guards_of(&source).guards, ["upper"], "{locale}");
        }
        let unknown =
            "ActiveSupport::Inflector.inflections(LOCALE) do |i|\n  i.acronym \"API\"\nend\n";
        assert_eq!(guards_of(unknown).symbol, None);
    }

    fn an_alternating_chain_resolves_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in (0..depth).rev() {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
        }
        for number in 0..depth {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            let name = if number % 2 == 0 { "a" } else { "b" };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  before_action :{name}, only: :show\n  def show; end\nend\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        assert!(found.iter().all(|entry| entry.guards.len() <= 2));
        elapsed
    }

    #[test]
    fn an_alternating_chain_gives_at_most_two_guards() {
        an_alternating_chain_resolves_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_an_alternating_chain_resolves_in_linear_time() {
        let elapsed = an_alternating_chain_resolves_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn nested_scopes_resolve_in_linear_time_at(depth: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for _ in 0..depth {
            routes.push_str("scope \"/a\" do\n");
        }
        routes.push_str("get \"/x\", to: \"a#b\"\n");
        for _ in 0..depth {
            routes.push_str("end\n");
        }
        routes.push_str("end\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), 1);
        let path = &found[0].route.as_ref().unwrap().path_text();
        assert_eq!(path.len(), 2 * depth + 2);
        assert!(path.ends_with("/a/x"));
        elapsed
    }

    #[test]
    fn nested_scopes_give_the_whole_path() {
        nested_scopes_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_nested_scopes_resolve_in_linear_time() {
        let elapsed = nested_scopes_resolve_in_linear_time_at(2_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn deeply_nested_callbacks_and_literals_do_not_overflow_the_stack() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let depth = 3_000;
        let mut base = String::from("class ApplicationController\n");
        for _ in 0..depth {
            base.push_str("if c\n");
        }
        base.push_str("before_action :auth\n");
        for _ in 0..depth {
            base.push_str("end\n");
        }
        base.push_str("end\n");
        let users = "class UsersController < ApplicationController\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/a.rb", base.as_str()),
                ("controllers/u.rb", users),
            ],
        );
        assert!(found[0].guards.is_empty());
        let nested = format!("{}\"x\"{}", "[".repeat(depth), "]".repeat(depth));
        let initializer = format!(
            "ActiveSupport::Inflector.inflections do |inflect|\n  inflect.uncountable {nested}\nend\n"
        );
        let found = rails_routes(
            routes,
            &[
                ("controllers/u.rb", users),
                ("config/initializers/i.rb", initializer.as_str()),
            ],
        );
        assert_eq!(found[0].symbol, None);
    }

    #[test]
    fn a_skip_or_callback_that_omen_cannot_read_makes_the_guards_unknown() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"a#show\"\n  get \"/b\", to: \"b#show\"\n  get \"/c\", to: \"c#show\"\n  get \"/d\", to: \"d#show\"\n  get \"/e\", to: \"e#show\"\n";
        let routes = format!("{routes}end\n");
        let base = "class ApplicationController\n  before_action :auth\nend\n";
        let classes = "class AController < ApplicationController\n  skip_before_action NAME\n  def show; end\nend\nclass BController < ApplicationController\n  self.skip_before_action :auth\n  def show; end\nend\nclass CController < ApplicationController\n  skip_callback :process_action, :before, :auth\n  def show; end\nend\nclass DController < ApplicationController\n  before_action AUTH, only: :index\n  def show; end\nend\nclass EController < ApplicationController\n  def show; end\nend\n";
        let found = rails_routes(
            &routes,
            &[("controllers/a.rb", base), ("controllers/b.rb", classes)],
        );
        let by_path = |path: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap()
        };
        for path in ["/a", "/c", "/d"] {
            assert_eq!(by_path(path).guards, ["auth"], "{path}");
            assert_eq!(
                by_path(path).guards_unknown_reason,
                Some(GuardsUnknownReason::UnreadableCallback),
                "{path}"
            );
        }
        assert_eq!(by_path("/e").guards, ["auth"]);
        assert!(by_path("/e").guards_complete);
        // `self.skip_before_action` is the call of the class itself.
        assert!(by_path("/b").guards.is_empty());
        assert!(by_path("/b").guards_complete);
    }

    #[test]
    fn the_target_of_a_route_is_read_exactly() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\" => \"users#show\", to: \"admin/users#show\"\n  get \"/b\", to: \"users#show#x\"\n  controller :users do\n    get \"x\", action: ACTION\n    get \"/c\", controller: CONTROLLER, action: \"show\"\n    get \"/d\", to: TARGET, action: \"show\"\n    get \"/e\", action: \"show\"\n  end\nend\n";
        let users = "class UsersController\n  before_action :auth, except: :show\n  before_action :shown, only: :show\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/u.rb", users)]);
        let by_path = |path: &str| {
            let entry = found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap();
            (entry.symbol.is_some(), entry.guards.clone())
        };
        for path in ["/a", "/b", "/x", "/c", "/d"] {
            assert_eq!(by_path(path), (false, Vec::new()), "{path}");
        }
        assert_eq!(by_path("/e"), (true, strings(&["shown"])));
    }

    #[test]
    fn a_namespace_in_a_member_keeps_the_canonical_action_rule() {
        let routes = "Rails.application.routes.draw do\n  resources :users, only: [] do\n    member do\n      namespace :admin do\n        get :show\n      end\n    end\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /users/:id/admin"]);
    }

    #[test]
    fn api_only_of_an_engine_makes_the_value_of_each_engine_unknown() {
        let engine_a = "module A\n  class Engine < Rails::Engine\n    config.api_only = true\n  end\nend\nmodule B\n  class Engine < Rails::Engine\n  end\nend\n";
        let routes_b = "B::Engine.routes.draw do\n  resources :posts, except: [:index, :create, :show, :update, :destroy]\nend\n";
        let found = entries(
            Language::Ruby,
            &[
                ("b/config/routes.rb", routes_b),
                ("lib/engines.rb", engine_a),
            ],
        );
        assert!(found.iter().all(|entry| entry.framework != "rails"));
    }

    #[test]
    fn an_inflections_instance_makes_the_inflections_unreadable() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/x\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let initializer = "ActiveSupport::Inflector::Inflections.instance(:en).acronym(\"API\")\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/i.rb", initializer),
            ],
        );
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());
    }

    #[test]
    fn a_route_after_a_jump_in_the_block_is_not_registered() {
        let routes = "Rails.application.routes.draw do\n  get \"/live\", to: \"a#b\"\n  next\n  get \"/ghost\", to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /live"]);
    }

    fn deep_blocks_and_chains_resolve_in_linear_time(
        kind: &str,
        depth: usize,
    ) -> std::time::Duration {
        match kind {
            "constraints" | "module" => {
                let open = if kind == "constraints" {
                    "constraints(host: /./) do\n"
                } else {
                    "scope module: \"m\" do\n"
                };
                let mut routes = String::from("Rails.application.routes.draw do\n");
                for _ in 0..depth {
                    routes.push_str(open);
                }
                routes.push_str("get \"/x\", to: \"u#s\"\n");
                for _ in 0..depth {
                    routes.push_str("end\n");
                }
                routes.push_str("end\n");
                let files = [("config/routes.rb", routes.as_str())];
                let (found, elapsed) = entries_lean(Language::Ruby, &files);
                assert_eq!(found.len(), 1);
                elapsed
            }
            _ => {
                let routes = format!(
                    "Rails.application.routes.draw do\n  get \"/u\", to: \"c{}#show\"\nend\n",
                    depth - 1
                );
                let mut classes = String::new();
                for number in 0..depth {
                    let parent = if number == 0 {
                        String::new()
                    } else {
                        format!(" < C{}Controller", number - 1)
                    };
                    let line = if kind == "unique" {
                        format!("  before_action :g{number}, only: :show\n")
                    } else if kind == "plain" {
                        format!("  before_action :g{number}\n")
                    } else if number % 2 == 0 {
                        String::from("  before_action :a\n")
                    } else {
                        String::from("  before_action :b\n")
                    };
                    classes.push_str(&format!(
                        "class C{number}Controller{parent}\n{line}  def show; end\nend\n"
                    ));
                }
                let files = [
                    ("config/routes.rb", routes.as_str()),
                    ("controllers/chain.rb", classes.as_str()),
                ];
                let (found, elapsed) = entries_lean(Language::Ruby, &files);
                assert_eq!(found.len(), 1);
                if kind == "unique" || kind == "plain" {
                    assert_eq!(found[0].guards.len(), depth);
                }
                elapsed
            }
        }
    }

    fn nested_scopes_without_routes_at(depth: usize, segment: &str) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for _ in 0..depth {
            routes.push_str(&format!("scope \"{segment}\" do\n"));
        }
        for _ in 0..depth {
            routes.push_str("end\n");
        }
        routes.push_str("end\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert!(found.is_empty());
        elapsed
    }

    #[test]
    fn nested_scopes_without_routes_give_no_route() {
        nested_scopes_without_routes_at(30, "/a");
        nested_scopes_without_routes_at(30, "/a(x)");
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_nested_scopes_without_routes_resolve_in_linear_time() {
        for segment in ["/a", "/a(x)"] {
            assert_scales_linearly(segment, 15_000, |size| {
                nested_scopes_without_routes_at(size, segment)
            });
        }
    }

    fn nested_blocks_with_one_route_at(depth: usize, open: &str) -> (usize, std::time::Duration) {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for _ in 0..depth {
            routes.push_str(open);
            routes.push('\n');
        }
        routes.push_str("get \"/x\", to: \"u#s\"\n");
        for _ in 0..depth {
            routes.push_str("end\n");
        }
        routes.push_str("end\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        (found.len(), elapsed)
    }

    const NESTING_OPENERS: [&str; 4] = [
        "namespace :a do",
        "scope module: :a do",
        "scope as: :a do",
        "resources :a, only: [] do",
    ];

    #[test]
    fn nested_blocks_of_each_kind_give_their_route() {
        for open in NESTING_OPENERS {
            assert_eq!(nested_blocks_with_one_route_at(30, open).0, 1, "{open}");
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_nested_blocks_of_each_kind_resolve_in_linear_time() {
        for open in NESTING_OPENERS {
            assert_scales_linearly(open, 15_000, |size| {
                nested_blocks_with_one_route_at(size, open).1
            });
        }
    }

    #[test]
    fn a_scope_path_with_a_parenthesis_makes_the_block_unknown() {
        let routes = "Rails.application.routes.draw do\n  scope \"/a(:x)\" do\n    get \"/in\", to: \"a#b\"\n  end\n  get \"/out\", to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /out"]);
    }

    #[test]
    fn deep_blocks_and_chains_give_their_route() {
        for kind in ["constraints", "module", "unique", "plain", "alternating"] {
            deep_blocks_and_chains_resolve_in_linear_time(kind, 30);
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deep_blocks_and_chains_resolve_in_linear_time() {
        for (kind, depth) in [
            ("constraints", 2_000),
            ("module", 2_000),
            ("unique", 4_000),
            ("plain", 4_000),
            ("alternating", 4_000),
        ] {
            assert_scales_linearly(kind, depth, |size| {
                deep_blocks_and_chains_resolve_in_linear_time(kind, size)
            });
        }
    }

    #[test]
    fn a_quoted_symbol_key_is_a_symbol_key() {
        let routes = "Rails.application.routes.draw do\n  get \"/x\", to: \"users#show\"\nend\n";
        let users = "class ApplicationController\n  before_action :auth\nend\nclass UsersController < ApplicationController\n  before_action :extra, \"only\": :index\n  before_action :cond, \"if\": :ok\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/u.rb", users)]);
        assert_eq!(found[0].guards, ["auth"]);

        let scoped = "Rails.application.routes.draw do\n  scope \"module\": \"admin\" do\n    get \"/x\", to: \"users#show\"\n  end\nend\n";
        let found = rails_routes(scoped, &[("controllers/u.rb", users)]);
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());

        let namespaced = "Rails.application.routes.draw do\n  namespace :admin, \"path\": \"x\" do\n    get \"/y\", to: \"users#show\"\n  end\nend\n";
        let found = rails_routes(namespaced, &[]);
        assert_eq!(verbs_and_paths(&found), ["GET /x/y"]);
    }

    #[test]
    fn a_module_or_a_call_outside_the_class_makes_the_guards_unknown() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"a#show\"\n  get \"/b\", to: \"b#show\"\n  get \"/c\", to: \"c#show\"\n  get \"/d\", to: \"d#show\"\n  get \"/e\", to: \"e#show\"\nend\n";
        let base = "class ApplicationController\n  before_action :auth\nend\nmodule PublicAccess\n  extend ActiveSupport::Concern\n  included do\n    skip_before_action :auth\n  end\nend\n";
        let classes = "class AController < ApplicationController\n  include PublicAccess\n  def show; end\nend\nclass BController < ApplicationController\n  def show; end\nend\nBController.skip_before_action :auth\nclass CController < ApplicationController\n  def show; end\nend\nCController.class_eval do\n  skip_before_action :auth\nend\nclass DController < ApplicationController\n  prepend Wrapper\n  def show; end\nend\nclass EController < ApplicationController\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[("controllers/a.rb", base), ("controllers/b.rb", classes)],
        );
        let by_path = |path: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap()
        };
        let module = Some(GuardsUnknownReason::IncludedModule);
        let outside = Some(GuardsUnknownReason::CallbackOutsideClass);
        for (path, reason) in [
            ("/a", module),
            ("/b", outside),
            ("/c", outside),
            ("/d", module),
        ] {
            assert_eq!(by_path(path).guards, ["auth"], "{path}");
            assert!(!by_path(path).guards_complete, "{path}");
            assert_eq!(by_path(path).guards_unknown_reason, reason, "{path}");
        }
        assert_eq!(by_path("/e").guards, ["auth"]);
        assert!(by_path("/e").guards_complete);
        assert_eq!(by_path("/e").guards_unknown_reason, None);
    }

    #[test]
    fn a_forgery_or_basic_authentication_call_outside_the_class_makes_the_guards_unknown() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"a#show\"\nend\n";
        for call in [
            "AController.skip_forgery_protection",
            "AController.protect_from_forgery with: :null_session",
            "AController.http_basic_authenticate_with name: \"x\", password: \"y\"",
        ] {
            let classes = format!(
                "class AController < ActionController::Base\n  protect_from_forgery\n  def show; end\nend\n{call}\n"
            );
            let found = rails_routes(routes, &[("controllers/a.rb", classes.as_str())]);
            assert_eq!(found[0].guards, ["verify_authenticity_token"], "{call}");
            assert_eq!(
                found[0].guards_unknown_reason,
                Some(GuardsUnknownReason::CallbackOutsideClass),
                "{call}"
            );
        }
    }

    #[test]
    fn an_include_in_the_superclass_makes_the_guards_of_each_child_unknown() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"a#show\"\nend\n";
        let classes = "class ApplicationController\n  include Pundit\n  before_action :auth\nend\nclass AController < ApplicationController\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/a.rb", classes)]);
        assert!(found[0].symbol.is_some());
        assert_eq!(found[0].guards, ["auth"]);
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::IncludedModule)
        );
    }

    #[test]
    fn an_operator_assignment_of_api_only_makes_the_value_unknown() {
        let routes = "Rails.application.routes.draw do\n  resources :posts\nend\n";
        for assignment in [
            "config.api_only ||= true",
            "config.api_only &&= true",
            "config.api_only += 1",
        ] {
            let application = format!(
                "module App\n  class Application < Rails::Application\n    {assignment}\n  end\nend\n"
            );
            let found = rails_routes(routes, &[("config/application.rb", application.as_str())]);
            assert!(found.is_empty(), "{assignment}");
        }
    }

    #[test]
    fn a_repeated_name_in_one_callback_call_follows_the_order_of_rails() {
        let routes = "Rails.application.routes.draw do\n  get \"/a\", to: \"a#show\"\n  get \"/b\", to: \"b#show\"\nend\n";
        let classes = "class AController\n  before_action :a, :b, :a\n  def show; end\nend\nclass BController\n  before_action :c\n  prepend_before_action :a, :b, :a\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/a.rb", classes)]);
        let by_path = |path: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap()
                .guards
                .clone()
        };
        assert_eq!(by_path("/a"), ["b", "a"]);
        assert_eq!(by_path("/b"), ["a", "b", "c"]);
        let listed = "class AController\n  before_action :a, :b, :a, only: :show\n  def show; end\nend\nclass BController\n  before_action :c\n  prepend_before_action :a, :b, :a, only: :show\n  def show; end\nend\n";
        let found = rails_routes(routes, &[("controllers/a.rb", listed)]);
        let by_path = |path: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .unwrap()
                .guards
                .clone()
        };
        assert_eq!(by_path("/a"), ["b", "a"]);
        assert_eq!(by_path("/b"), ["a", "b", "c"]);
    }

    #[test]
    fn only_a_symbol_via_has_a_meaning_that_omen_knows() {
        let routes = "Rails.application.routes.draw do\n  match \"/s\", to: \"a#b\", via: \"all\"\n  match \"/y\", to: \"a#b\", via: :all\n  match \"/m\", to: \"a#b\", via: :m_search\n  match \"/mixed\", to: \"a#b\", via: [:get, \"post\"]\n  match \"/two\", to: \"a#b\", via: [:get, :post]\n  match \"/pct\", to: \"a#b\", via: %i[get post]\n  match \"/words\", to: \"a#b\", via: %w[get post]\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "ANY /y",
                "GET /pct",
                "GET /two",
                "M-SEARCH /m",
                "POST /pct",
                "POST /two"
            ]
        );
    }

    #[test]
    fn an_engine_route_set_gives_no_entry() {
        let routes = "Blog::Engine.routes.draw do\n  resources :users, only: :show\nend\n";
        let engine = "module Blog\n  class Engine < ::Rails::Engine\n  end\nend\n";
        let found = rails_routes(routes, &[("lib/engine.rb", engine)]);
        assert!(found.is_empty());
    }

    fn chain_of_empty_classes_with_many_actions(count: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        for number in 0..count {
            routes.push_str(&format!(
                "  get \"/a{number}\", to: \"c{}#a{number}\"\n",
                count - 1
            ));
        }
        routes.push_str("end\n");
        let mut classes = String::new();
        for number in 0..count {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!("class C{number}Controller{parent}\nend\n"));
        }
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        elapsed
    }

    fn listed_callbacks_at(count: usize, chained: bool) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        let leaf = if chained { count - 1 } else { 0 };
        for number in 0..count {
            routes.push_str(&format!(
                "  get \"/a{number}\", to: \"c{leaf}#a{number}\"\n"
            ));
        }
        routes.push_str("end\n");
        if chained {
            for number in 0..count {
                let parent = if number == 0 {
                    String::new()
                } else {
                    format!(" < C{}Controller", number - 1)
                };
                classes.push_str(&format!(
                    "class C{number}Controller{parent}\n  before_action :g{number}, only: :a{number}\n  def a{number}; end\nend\n"
                ));
            }
        } else {
            classes.push_str("class C0Controller\n");
            for number in 0..count {
                classes.push_str(&format!(
                    "  before_action :g{number}, only: :a{number}\n  def a{number}; end\n"
                ));
            }
            classes.push_str("end\n");
        }
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.guards.len() == 1));
        elapsed
    }

    fn chain_with_one_listed_action_at(count: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in 0..count {
            routes.push_str(&format!("  get \"/a{number}\", to: \"c{number}#show\"\n"));
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  before_action :g, only: :show\n  def show; end\nend\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.guards == ["g"]));
        elapsed
    }

    #[test]
    fn a_chain_with_one_listed_action_gives_the_guard_of_each_class() {
        chain_with_one_listed_action_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_with_one_listed_action_resolves_in_linear_time() {
        assert_scales_linearly("one listed action", 30_000, chain_with_one_listed_action_at);
    }

    fn chain_of_skips_that_do_not_apply_at(count: usize, skip: &str) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in 0..count {
            routes.push_str(&format!("  get \"/a{number}\", to: \"c{number}#show\"\n"));
            if number == 0 {
                classes.push_str(
                    "class C0Controller\n  before_action :g, only: :show\n  def show; end\nend\n",
                );
            } else {
                classes.push_str(&format!(
                    "class C{number}Controller < C{}Controller\n  {skip}\nend\n",
                    number - 1
                ));
            }
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.guards == ["g"]));
        elapsed
    }

    #[test]
    fn a_skip_that_does_not_apply_leaves_the_guard_of_the_root() {
        for skip in [
            "skip_before_action :g, only: :other",
            "skip_before_action :g, except: :show",
            "skip_before_action :g, only: :other, except: :show",
        ] {
            chain_of_skips_that_do_not_apply_at(30, skip);
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_skips_that_do_not_apply_resolves_in_linear_time() {
        for skip in [
            "skip_before_action :g, only: :other",
            "skip_before_action :g, except: :show",
            "skip_before_action :g, only: :other, except: :show",
        ] {
            assert_scales_linearly(skip, 15_000, |size| {
                chain_of_skips_that_do_not_apply_at(size, skip)
            });
        }
    }

    #[test]
    fn the_guards_of_random_chains_match_a_replay_of_the_callbacks() {
        let kinds = [
            "before_action",
            "skip_before_action",
            "prepend_before_action",
        ];
        type List<'a> = (&'a str, Option<&'a [&'a str]>, Option<&'a [&'a str]>);
        let lists: [List<'_>; 7] = [
            ("", None, None),
            (", only: :x", Some(&["x"]), None),
            (", only: :y", Some(&["y"]), None),
            (", except: :x", None, Some(&["x"])),
            (", except: :y", None, Some(&["y"])),
            (
                ", only: [:x, :y], except: :y",
                Some(&["x", "y"]),
                Some(&["y"]),
            ),
            (", except: [:x, :z]", None, Some(&["x", "z"])),
        ];
        let names = ["a", "b", "c"];
        let mut state = 20_241_117_u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as usize) % bound
        };
        for round in 0..200 {
            let mut classes = String::new();
            let mut replay: Vec<(usize, usize, usize)> = Vec::new();
            let depth = 1 + next(6);
            let mut calls: Vec<(usize, usize, usize)> = Vec::new();
            for level in 0..depth {
                let parent = if level == 0 {
                    String::new()
                } else {
                    format!(" < C{}Controller", level - 1)
                };
                classes.push_str(&format!("class C{level}Controller{parent}\n"));
                for _ in 0..(1 + next(3)) {
                    let (kind, list, name) = (next(3), next(lists.len()), next(3));
                    let skip_only = kind == 1 && list == 0;
                    let list = if skip_only && next(2) == 0 { 1 } else { list };
                    classes.push_str(&format!(
                        "  {} :{}{}\n",
                        kinds[kind], names[name], lists[list].0
                    ));
                    calls.push((kind, list, name));
                }
                classes.push_str("  def x; end\n  def y; end\n  def z; end\nend\n");
            }
            replay.extend(calls);
            let leaf = depth - 1;
            let routes = format!(
                "Rails.application.routes.draw do\n  get \"/x\", to: \"c{leaf}#x\"\n  get \"/y\", to: \"c{leaf}#y\"\n  get \"/z\", to: \"c{leaf}#z\"\nend\n"
            );
            let found = rails_routes(&routes, &[("controllers/chain.rb", classes.as_str())]);
            for entry in &found {
                let action = entry
                    .route
                    .as_ref()
                    .unwrap()
                    .path_text()
                    .trim_start_matches('/');
                let mut expected: Vec<&str> = Vec::new();
                for (kind, list, name) in &replay {
                    let (_, only, except) = lists[*list];
                    let applies = only.is_none_or(|only| only.contains(&action))
                        && except.is_none_or(|except| !except.contains(&action));
                    let name = names[*name];
                    if *kind == 1 {
                        if applies {
                            expected.retain(|existing| *existing != name);
                        }
                        continue;
                    }
                    expected.retain(|existing| *existing != name);
                    if applies && *kind == 2 {
                        expected.insert(0, name);
                    } else if applies {
                        expected.push(name);
                    }
                }
                assert_eq!(
                    entry.guards, expected,
                    "round {round} action {action}\n{classes}"
                );
            }
        }
    }

    fn chain_of_distinct_names_at(
        count: usize,
        shape: usize,
        reversed: bool,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in 0..count {
            let line = match shape {
                0 => format!("skip_before_action :g{number}, only: :show"),
                1 => format!(
                    "before_action :g{number}, only: :show\n  skip_before_action :g{number}, only: :show"
                ),
                _ => format!(
                    "skip_before_action :g{number}, only: [:show, :other], except: :other"
                ),
            };
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            let body = if number == 0 { "  def show; end\n" } else { "" };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  {line}\n{body}end\n"
            ));
        }
        let order: Vec<usize> = if reversed {
            (0..count).rev().collect()
        } else {
            (0..count).collect()
        };
        for number in order {
            routes.push_str(&format!("  get \"/a{number}\", to: \"c{number}#show\"\n"));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.guards.is_empty()));
        elapsed
    }

    #[test]
    fn a_chain_of_distinct_names_for_one_action_gives_the_guards_of_each_class() {
        for shape in 0..3 {
            for reversed in [false, true] {
                chain_of_distinct_names_at(30, shape, reversed);
            }
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_distinct_names_for_one_action_resolves_in_linear_time() {
        for shape in 0..3 {
            for reversed in [false, true] {
                assert_scales_linearly(
                    &format!("shape {shape} reversed {reversed}"),
                    15_000,
                    |size| chain_of_distinct_names_at(size, shape, reversed),
                );
            }
        }
    }

    fn one_big_callback_at(size: usize, shape: usize) -> std::time::Duration {
        let names: Vec<String> = (0..size).map(|number| format!(":n{number}")).collect();
        let actions: Vec<String> = (0..size).map(|number| format!(":a{number}")).collect();
        let (names, actions) = (names.join(", "), actions.join(", "));
        let (include, call) = match shape {
            0 => (
                "  include Helpers\n",
                format!("before_action {names}, only: [{actions}]"),
            ),
            1 => ("", format!("before_action {names}, only: [{actions}]")),
            _ => (
                "",
                format!("skip_before_action {names}, only: [{actions}], except: [{actions}]"),
            ),
        };
        let classes = format!(
            "class C0Controller\n{include}  {call}\n  def a1; end\n  def a2; end\n  def a3; end\nend\n"
        );
        let routes = "Rails.application.routes.draw do\n  get \"/x1\", to: \"c0#a1\"\n  get \"/x2\", to: \"c0#a2\"\n  get \"/x3\", to: \"c0#a3\"\nend\n";
        let files = [
            ("config/routes.rb", routes),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), 3);
        let expected = if shape == 2 { 0 } else { size };
        assert!(found.iter().all(|entry| entry.guards.len() == expected));
        assert!(found
            .iter()
            .all(|entry| entry.guards_complete == (shape != 0)));
        elapsed
    }

    #[test]
    fn one_callback_with_many_names_and_actions_gives_its_guards() {
        for shape in 0..3 {
            one_big_callback_at(30, shape);
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_one_callback_with_many_names_and_actions_resolves_in_linear_time() {
        for shape in 0..3 {
            assert_scales_linearly(&format!("shape {shape}"), 10_000, |size| {
                one_big_callback_at(size, shape)
            });
        }
    }

    fn chain_under_a_heavy_callback_at(
        size: usize,
        middle: bool,
        form: usize,
    ) -> std::time::Duration {
        let names: Vec<String> = (0..size).map(|number| format!(":n{number}")).collect();
        let names = names.join(", ");
        let call = match form {
            0 => format!("skip_before_action {names}, only: :show"),
            _ => format!("skip_before_action {names}, only: :other, except: :show"),
        };
        let heavy = if middle { size / 2 } else { 0 };
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in 0..size {
            routes.push_str(&format!("  get \"/a{number}\", to: \"c{number}#show\"\n"));
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            let own = format!("before_action :own{number}, only: :other");
            let line = if number == heavy {
                call.as_str()
            } else {
                own.as_str()
            };
            let body = if number == 0 { "  def show; end\n" } else { "" };
            classes.push_str(&format!(
                "class C{number}Controller{parent}\n  {line}\n{body}end\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), size);
        assert!(found.iter().all(|entry| entry.guards.is_empty()));
        elapsed
    }

    #[test]
    fn a_chain_under_a_callback_with_many_names_gives_the_guards_of_each_class() {
        for middle in [false, true] {
            for form in 0..2 {
                chain_under_a_heavy_callback_at(30, middle, form);
            }
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_under_a_callback_with_many_names_resolves_in_linear_time() {
        for middle in [false, true] {
            for form in 0..2 {
                assert_scales_linearly(&format!("middle {middle} form {form}"), 4_000, |size| {
                    chain_under_a_heavy_callback_at(size, middle, form)
                });
            }
        }
    }

    fn long_names_at(count: usize, length: usize, in_module: bool) -> std::time::Duration {
        let name = "a".repeat(length);
        let open = if in_module {
            format!("scope module: \"{name}\" do\n")
        } else {
            format!("controller :{name} do\n")
        };
        let mut routes = String::from("Rails.application.routes.draw do\n");
        routes.push_str(&open);
        for number in 0..count {
            if in_module {
                routes.push_str(&format!("get \"x{number}\", to: \"u#s\"\n"));
            } else {
                routes.push_str(&format!("get \"x{number}\"\n"));
            }
        }
        routes.push_str("end\nend\n");
        let files = [("config/routes.rb", routes.as_str())];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        elapsed
    }

    #[test]
    fn many_actions_and_long_names_give_their_routes() {
        chain_of_empty_classes_with_many_actions(30);
        listed_callbacks_at(30, true);
        listed_callbacks_at(30, false);
        long_names_at(30, 50, true);
        long_names_at(30, 50, false);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_actions_over_a_chain_resolve_in_linear_time() {
        assert_scales_linearly(
            "empty chain",
            4_000,
            chain_of_empty_classes_with_many_actions,
        );
        assert_scales_linearly("one class", 4_000, |size| listed_callbacks_at(size, false));
        assert_scales_linearly("chain", 4_000, |size| listed_callbacks_at(size, true));
        for in_module in [true, false] {
            assert_scales_linearly("long names", 2_000, |size| {
                long_names_at(size, 400_000, in_module)
            });
        }
    }

    #[test]
    fn a_literal_case_keeps_only_the_branch_that_runs() {
        let routes = "Rails.application.routes.draw do\n  case false\n  when true\n    get \"/ghost\", to: \"a#b\"\n  when false\n    get \"/live\", to: \"a#b\"\n  else\n    get \"/else\", to: \"a#b\"\n  end\n  case nil\n  when true, false\n    get \"/ghost_two\", to: \"a#b\"\n  else\n    get \"/else_two\", to: \"a#b\"\n  end\n  case name\n  when :x\n    get \"/unknown\", to: \"a#b\"\n  end\n  case true\n  when name\n    get \"/pattern\", to: \"a#b\"\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(paths, ["GET /else_two", "GET /live"]);
    }

    #[test]
    fn the_last_value_of_a_repeated_option_wins() {
        let routes = "Rails.application.routes.draw do\n  get \"/x\", to: \"users#show\", to: \"posts#show\"\n  resources :a, only: [:show], only: [:index]\n  namespace :n, path: \"p1\", path: \"p2\" do\n    get \"/in\", to: \"a#b\"\n  end\nend\n";
        let users = "class UsersController\n  before_action :users\n  def show; end\nend\n";
        let posts = "class PostsController\n  before_action :posts\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[("controllers/u.rb", users), ("controllers/p.rb", posts)],
        );
        let by_path = |path: &str| {
            found
                .iter()
                .find(|entry| entry.route.as_ref().unwrap().path_text() == path)
                .map(|entry| entry.guards.clone())
        };
        assert_eq!(by_path("/x"), Some(strings(&["posts"])));
        assert!(by_path("/a").is_some());
        assert!(by_path("/a/:id").is_none());
        assert!(by_path("/p2/in").is_some());
        assert!(by_path("/p1/in").is_none());
    }

    #[test]
    fn an_acronym_string_is_evaluated_as_ruby_does() {
        let routes =
            "Rails.application.routes.draw do\n  get \"/u\", to: \"api/users#show\"\nend\n";
        let upper = "module API\n  class UsersController\n    before_action :upper\n    def show; end\n  end\nend\n";
        let lower = "module Api\n  class UsersController\n    before_action :lower\n    def show; end\n  end\nend\n";
        let escaped = "ActiveSupport::Inflector.inflections do |inflect|\n  inflect.acronym \"\\x41PI\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", escaped),
            ],
        );
        assert_eq!(found[0].guards, ["upper"]);
        let unknown = "ActiveSupport::Inflector.inflections do |inflect|\n  inflect.acronym \"\\M-aPI\"\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/upper.rb", upper),
                ("controllers/lower.rb", lower),
                ("config/initializers/inflections.rb", unknown),
            ],
        );
        assert_eq!(found[0].symbol, None);
    }

    fn routes_from_the_deepest_class_resolve_in_linear_time_at(
        depth: usize,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut classes = String::new();
        for number in (0..depth).rev() {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
        }
        for number in 0..depth {
            let parent = if number == 0 {
                String::new()
            } else {
                format!(" < C{}Controller", number - 1)
            };
            classes.push_str(&format!("class C{number}Controller{parent}\nend\n"));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers/chain.rb", classes.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), depth);
        elapsed
    }

    #[test]
    fn routes_from_the_deepest_class_give_each_route() {
        routes_from_the_deepest_class_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_routes_from_the_deepest_class_resolve_in_linear_time() {
        let elapsed = routes_from_the_deepest_class_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn a_superclass_name_with_two_identities_is_ambiguous() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let top = "class ApplicationController\n  before_action :top\nend\n";
        let admin =
            "module Admin\n  class ApplicationController\n    before_action :admin\n  end\nend\n";
        let users = "class UsersController < ApplicationController\n  before_action :owner\n  def show; end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/application_controller.rb", top),
                ("controllers/admin.rb", admin),
                ("controllers/users_controller.rb", users),
            ],
        );
        // Ruby finds the class at the top level before a class in another module.
        assert_eq!(found[0].guards, ["top", "owner"]);
        assert!(found[0].guards_complete);
        let other =
            "module Other\n  class ApplicationController\n    before_action :top\n  end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/other.rb", other),
                ("controllers/admin.rb", admin),
                ("controllers/users_controller.rb", users),
            ],
        );
        assert_eq!(found[0].guards, ["owner"]);
        assert_eq!(
            found[0].guards_unknown_reason,
            Some(GuardsUnknownReason::UnresolvedSuperclass)
        );
    }

    #[test]
    fn a_method_belongs_to_the_class_that_contains_it() {
        let routes = "Rails.application.routes.draw do\n  get \"/u\", to: \"users#show\"\nend\n";
        let controller = "class UsersController < ApplicationController\n  class UsersController\n    def show; end\n  end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", controller)]);
        assert_eq!(found[0].symbol, None);
        let direct = "class UsersController\n  def show; end\n  class Helper\n    def show; end\n  end\nend\n";
        let found = rails_routes(routes, &[("users_controller.rb", direct)]);
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("users_controller.rb#UsersController.show")
        );
    }

    fn same_named_controllers_in_one_file_resolve_in_linear_time_at(
        count: usize,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut controllers = String::new();
        for number in 0..count {
            routes.push_str(&format!(
                "  get \"/r{number}\", to: \"m{number}/users#show\"\n"
            ));
            controllers.push_str(&format!(
                "module M{number}\n  class UsersController\n    def show; end\n  end\nend\n"
            ));
        }
        routes.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("controllers.rb", controllers.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        elapsed
    }

    #[test]
    fn same_named_controllers_in_one_file_resolve_to_their_own_module() {
        same_named_controllers_in_one_file_resolve_in_linear_time_at(30);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_same_named_controllers_in_one_file_resolve_in_linear_time() {
        let elapsed = same_named_controllers_in_one_file_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn nested_namespaces_match_the_whole_module_path() {
        let routes = "Rails.application.routes.draw do\n  namespace :admin do\n    namespace :v1 do\n      get \"/u\", to: \"users#show\"\n    end\n  end\nend\n";
        let right = "module Admin\n  module V1\n    class UsersController\n      before_action :right\n      def show; end\n    end\n  end\nend\n";
        let wrong = "module Other\n  module V1\n    class UsersController\n      before_action :wrong\n      def show; end\n    end\n  end\nend\n";
        let found = rails_routes(
            routes,
            &[
                ("controllers/admin.rb", right),
                ("controllers/other.rb", wrong),
            ],
        );
        assert_eq!(
            found[0].symbol.as_deref(),
            Some("controllers/admin.rb#Admin.V1.UsersController.show")
        );
        assert_eq!(found[0].guards, ["right"]);
        let found = rails_routes(routes, &[("controllers/other.rb", wrong)]);
        assert_eq!(found[0].symbol, None);
        assert!(found[0].guards.is_empty());
    }

    #[test]
    fn no_two_routes_of_a_file_share_a_key() {
        let routes = "Rails.application.routes.draw do\n  resources :posts, :comments, path: \"items\", only: :index\n  resources :users, only: [:index] do\n    member { get :a }\n    collection { get :a }\n  end\n  resource :profile, only: [:show, :edit]\n  namespace :admin do\n    root \"home#index\"\n  end\nend\n";
        let found = rails_routes(routes, &[]);
        let mut paths = verbs_and_paths(&found);
        paths.sort();
        assert_eq!(
            paths,
            [
                "GET /admin",
                "GET /items",
                "GET /items",
                "GET /profile",
                "GET /profile/edit",
                "GET /users",
                "GET /users/:id/a",
                "GET /users/a",
            ]
        );
        let keys: std::collections::BTreeSet<&str> =
            found.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys.len(), found.len());
    }

    fn many_methods_of_one_controller_resolve_in_linear_time_at(
        count: usize,
    ) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut controller = String::from("class UsersController\n  before_action :login\n");
        for number in 0..count {
            routes.push_str(&format!("  get \"/r{number}\", to: \"users#m{number}\"\n"));
            controller.push_str(&format!("  def m{number}; end\n"));
        }
        routes.push_str("end\n");
        controller.push_str("end\n");
        let files = [
            ("config/routes.rb", routes.as_str()),
            ("users_controller.rb", controller.as_str()),
        ];
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found
            .iter()
            .all(|entry| entry.symbol.is_some() && entry.guards == ["login"]));
        elapsed
    }

    #[test]
    fn many_methods_of_one_controller_resolve_with_their_guards() {
        many_methods_of_one_controller_resolve_in_linear_time_at(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_methods_of_one_controller_resolve_in_linear_time() {
        let elapsed = many_methods_of_one_controller_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    fn many_routes_and_controllers_resolve_in_linear_time_at(count: usize) -> std::time::Duration {
        let mut routes = String::from("Rails.application.routes.draw do\n");
        let mut controllers = Vec::new();
        for number in 0..count {
            routes.push_str(&format!("  get \"/r{number}\", to: \"c{number}#show\"\n"));
            controllers.push((
                format!("controllers/c{number:06}_controller.rb"),
                format!(
                    "class C{number}Controller < ApplicationController\n  before_action :guard{number}, only: [:show]\n  def show; end\nend\n"
                ),
            ));
        }
        routes.push_str("end\n");
        controllers.push((
            "controllers/application_controller.rb".to_string(),
            "class ApplicationController\n  before_action :login\nend\n".to_string(),
        ));
        controllers.sort();
        let mut files: Vec<(&str, &str)> = vec![("config/routes.rb", routes.as_str())];
        files.extend(
            controllers
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str())),
        );
        let (found, elapsed) = entries_lean(Language::Ruby, &files);
        assert_eq!(found.len(), count);
        assert!(found
            .iter()
            .all(|entry| entry.symbol.is_some() && entry.guards.len() == 2));
        elapsed
    }

    #[test]
    fn many_routes_and_controllers_resolve_with_their_guards() {
        many_routes_and_controllers_resolve_in_linear_time_at(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_routes_and_controllers_resolve_in_linear_time() {
        let elapsed = many_routes_and_controllers_resolve_in_linear_time_at(10_000);
        assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    }

    #[test]
    fn routes_that_omen_cannot_read_exactly_are_skipped() {
        let routes = "Rails.application.routes.draw do\n  name = \"dynamic\"\n  get \"/#{name}\", to: \"a#b\"\n  PARTS.each do |part|\n    get \"/#{part}\", to: \"a#b\"\n  end\n  [:c, other].each do |part|\n    resources part\n  end\n  concern :commentable do\n    get \"/comments\", to: \"comments#index\"\n  end\n  resources :things, concerns: :commentable, only: [:index]\n  draw(:admin)\n  mount Sidekiq::Web => \"/sidekiq\"\n  resources :users, only: only_actions\n  resources :slugs, param: :slug\n  resources name\n  get \"/kept\", to: \"a#b\"\nend\n";
        let found = rails_routes(routes, &[]);
        assert_eq!(
            verbs_and_paths(&found),
            ["GET /things", "GET /kept"]
                .iter()
                .map(|text| text.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn c_main_and_a_bash_script_are_cli_entries() {
        let c = "int main(int argc, char **argv) {\n    return 0;\n}\n";
        let found = entries(Language::C, &[("main.c", c)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].framework, "c");
        let bash = "#!/bin/bash\n\nrun() {\n    echo \"$1\"\n}\n\nrun \"$@\"\n";
        let found = entries(Language::Bash, &[("run.sh", bash)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, EntryKind::CliMain);
        assert_eq!(found[0].framework, "bash");
    }

    #[test]
    fn the_key_names_the_file_the_position_and_the_kind() {
        let source = "int main(void) {\n    return 0;\n}\n";
        let found = entries(Language::C, &[("main.c", source)]);
        assert_eq!(found[0].key, "main.c:1:0:cli_main");
        assert_eq!(found[0].kind.name(), "cli_main");
        assert_eq!(found[0].basis.name(), "convention");
    }
}
