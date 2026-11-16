//! Entry points: the places where a program receives input from outside.
//!
//! The table in this module names each framework by the syntax that declares a
//! handler: a decorator, an annotation, a registration call, a file convention,
//! or the name `main`. A guard is a decorator, an annotation, or a middleware
//! name that the syntax attaches to the handler. A guard is a declared fact. It
//! never means that the handler is authenticated.
//!
//! The public API is [`find_entry_points`]. A different analyzer can call it
//! with the parsed files and the index of the same files.

use std::ops::Range;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::spec::syntax_of;
use super::syntax;
use crate::core::{is_test_file, Language};
use crate::index::extract::FileSymbols;
use crate::index::model::{Span, Symbol, SymbolKind};
use crate::parser::{cfg_test_module_ranges, ParseResult};

mod rails;
mod go_routes;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// The method that the framework registers, or `ANY`. gin and echo keep a custom method as the source writes it.
    pub method: String,
    /// The path as the source writes it, with a leading `/`.
    pub path: String,
    /// The names of the parameters in the path, in order.
    pub parameters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPoint {
    /// `file:line:column:kind`.
    pub key: String,
    pub file: String,
    pub kind: EntryKind,
    /// The range of the declaration, the registration call, or the file.
    pub range: Span,
    /// The key of the handler symbol, when the index has it.
    pub symbol: Option<String>,
    pub framework: String,
    pub route: Option<Route>,
    /// The decorators, annotations, and middleware names that the syntax attaches to the handler.
    pub guards: Vec<String>,
    pub basis: EntryBasis,
    pub in_test: bool,
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
    go_call_sites: std::sync::OnceLock<go_routes::CallSites>,
    callables: std::sync::OnceLock<std::collections::HashMap<String, Vec<(usize, usize)>>>,
    go_callables: std::sync::OnceLock<go_routes::PackageCallables>,
    go_packages: std::sync::OnceLock<go_routes::PackageNames>,
}

/// The syntax trees that the entry point rules ask for in other files. The source
/// parses a file when a rule asks, and it keeps the tree for the next question.
pub struct OnDemandTrees<'a> {
    parse_file: &'a (dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    error: parking_lot::Mutex<Option<crate::core::Error>>,
    parsed: parking_lot::Mutex<std::collections::HashMap<usize, Cell>>,
    shared: EntryShared,
}

/// The one parse of a file. The first thread that asks parses the file, and the other
/// threads wait for the result. A file that cannot be parsed stays without a tree.
type Cell = std::sync::Arc<std::sync::OnceLock<Option<std::sync::Arc<ParseResult>>>>;

impl<'a> OnDemandTrees<'a> {
    pub fn new(
        parse_file: &'a (dyn Fn(usize) -> crate::core::Result<Option<ParseResult>> + Sync),
    ) -> Self {
        Self {
            parse_file,
            error: parking_lot::Mutex::new(None),
            parsed: parking_lot::Mutex::new(std::collections::HashMap::new()),
            shared: EntryShared::default(),
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
        let cell = std::sync::Arc::clone(self.parsed.lock().entry(position).or_default());
        let parse = || match (self.parse_file)(position) {
            Ok(tree) => tree.map(std::sync::Arc::new),
            Err(error) => {
                self.error.lock().get_or_insert(error);
                None
            }
        };
        cell.get_or_init(parse)
            .as_ref()
            .map(|tree| TreeRef::Shared(std::sync::Arc::clone(tree)))
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
    let context = Context::new(own, &files[position], files, trees, rules);
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

fn route(method: &str, path: String) -> Route {
    Route {
        method: method.to_uppercase(),
        parameters: route_parameters(&path),
        path,
    }
}

/// Tells if a declaration or a typed parameter binds `name` to a text with a marker.
fn bound_to_marker(node: Node<'_>, source: &[u8], name: &str, markers: &[&str]) -> bool {
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
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| bound_to_marker(child, source, name, markers))
}

/// The local bindings of a Go file, by name.
type GoLocals = std::collections::HashMap<String, (Vec<usize>, Vec<usize>)>;

struct Context<'a> {
    parsed: &'a ParseResult,
    file: &'a FileSymbols,
    all: &'a [FileSymbols],
    all_parsed: &'a dyn TreeSource,
    rules: &'a [EntryRule],
    source: &'a [u8],
    test_ranges: Vec<Range<usize>>,
    /// The scope inside each block call of the routes, by the id of the node.
    rails_scopes:
        std::cell::RefCell<std::collections::HashMap<usize, Option<std::rc::Rc<rails::Scope>>>>,
    /// The ids of the children of a `case` that can run, by the id of the `case` node.
    rails_cases:
        std::cell::RefCell<std::collections::HashMap<usize, Option<std::rc::Rc<Vec<usize>>>>>,
    /// The position of the first symbol of the file for each start of a name.
    symbol_starts: std::collections::HashMap<usize, usize>,
    /// The positions of the functions, the methods, and the constructors of the file, by name.
    callables_here: std::collections::HashMap<String, Vec<usize>>,
    /// The local bindings of a Go file, by name: the starts of the bindings in order, and
    /// for each the highest end of a function that has a binding up to it.
    go_locals: std::sync::OnceLock<GoLocals>,
    /// The names of the variables that hold a `net/http` multiplexer, in a Go file.
    go_servemuxes: std::sync::OnceLock<std::collections::HashSet<String>>,
    /// The names that the package of a Go file declares at the top level.
    go_declared: std::sync::OnceLock<std::sync::Arc<std::collections::HashSet<String>>>,
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
}

impl<'a> Context<'a> {
    fn new(
        parsed: &'a ParseResult,
        file: &'a FileSymbols,
        all: &'a [FileSymbols],
        all_parsed: &'a dyn TreeSource,
        rules: &'a [EntryRule],
    ) -> Self {
        let test_ranges = if file.language == Language::Rust {
            let text = std::str::from_utf8(&parsed.source).unwrap_or("");
            cfg_test_module_ranges(&parsed.root_node(), text)
        } else {
            Vec::new()
        };
        let mut callables_here: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (position, symbol) in file.symbols.iter().enumerate() {
            if is_callable_kind(symbol) {
                callables_here
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(position);
            }
        }
        let mut symbol_starts = std::collections::HashMap::new();
        for (position, symbol) in file.symbols.iter().enumerate() {
            symbol_starts
                .entry(symbol.name_range.start_byte)
                .or_insert(position);
        }
        Self {
            parsed,
            file,
            all,
            all_parsed,
            rules,
            source: &parsed.source,
            test_ranges,
            rails_scopes: std::cell::RefCell::new(std::collections::HashMap::new()),
            rails_cases: std::cell::RefCell::new(std::collections::HashMap::new()),
            symbol_starts,
            callables_here,
            go_locals: std::sync::OnceLock::new(),
            go_servemuxes: std::sync::OnceLock::new(),
            go_declared: std::sync::OnceLock::new(),
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
        if self.is_string(node) {
            return Some(self.string_of(node));
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        children
            .into_iter()
            .find_map(|child| self.first_string(child))
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

    fn symbol_at(&self, name_node: Node<'_>) -> Option<String> {
        let position = self.symbol_starts.get(&name_node.start_byte())?;
        Some(self.file.symbols[*position].key.to_string())
    }

    /// The one function or method with the name. A definition in the file wins over a definition in another file.
    fn symbol_named(&self, name: &str, container: Option<&str>) -> Option<String> {
        let fits = |symbol: &Symbol| {
            container.is_none_or(|wanted| symbol.container.as_deref() == Some(wanted))
        };
        let here: Vec<&Symbol> = self
            .callables_here
            .get(name)
            .into_iter()
            .flatten()
            .map(|position| &self.file.symbols[*position])
            .filter(|symbol| fits(symbol))
            .collect();
        if here.len() == 1 {
            return Some(here[0].key.to_string());
        }
        let everywhere: Vec<&Symbol> = match self.all_parsed.shared() {
            Some(shared) => shared
                .callables
                .get_or_init(|| callables_by_name(self.all))
                .get(name)
                .into_iter()
                .flatten()
                .map(|(file, position)| &self.all[*file].symbols[*position])
                .filter(|symbol| fits(symbol))
                .collect(),
            None => callables_by_name(self.all)
                .get(name)
                .into_iter()
                .flatten()
                .map(|(file, position)| &self.all[*file].symbols[*position])
                .filter(|symbol| fits(symbol))
                .collect(),
        };
        (everywhere.len() == 1).then(|| everywhere[0].key.to_string())
    }

    fn handler_symbol(&self, handler: Node<'_>) -> Option<String> {
        if self.file.language == Language::Go {
            return self.go_handler_symbol(handler);
        }
        let name = match handler.kind() {
            "identifier" => self.text(handler),
            "attribute" | "member_expression" | "selector_expression" | "field_expression" => {
                self.text(handler).rsplit(['.', ':']).next().unwrap_or("")
            }
            _ => return None,
        };
        self.symbol_named(name, None)
    }

    fn arguments_of(call: Node<'_>) -> Vec<Node<'_>> {
        let Some(list) = call.child_by_field_name("arguments") else {
            return Vec::new();
        };
        let mut cursor = list.walk();
        let nodes: Vec<Node<'_>> = list.named_children(&mut cursor).collect();
        nodes
            .into_iter()
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
                    basis: draft.basis,
                    in_test: is_test_file(&self.file.file)
                        || self
                            .test_ranges
                            .iter()
                            .any(|range| range.contains(&draft.range.start_byte)),
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
                if let Some(route) = &entry.route {
                    entry.key = format!("{}:{}", entry.key, route.path);
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
    fn walk(&self, root: Node<'_>, out: &mut Vec<Draft>) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            self.visit(node, out);
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
    }

    fn visit(&self, node: Node<'_>, out: &mut Vec<Draft>) {
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
            Language::Ruby => self.ruby(node, out),
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
        for (position, symbol) in self.file.symbols.iter().enumerate() {
            if symbol.name != main_name
                || !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            {
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
        let guards: Vec<String> = decorators
            .iter()
            .filter_map(|decorator| decorator.named_child(0))
            .filter_map(|expression| {
                let callee = if expression.kind() == "call" {
                    expression.child_by_field_name("function")?
                } else {
                    expression
                };
                let name = self.text(callee).rsplit('.').next()?.to_string();
                PYTHON_GUARDS.contains(&name.as_str()).then_some(name)
            })
            .collect();
        for decorator in &decorators {
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
            if !self.is_framework_value(receiver, markers, &[(framework_module, exports)]) {
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
            out.push(Draft {
                range: Span::of(decorator),
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework,
                route: Some(route(&methods.join(","), path)),
                guards: guards.clone(),
                basis: EntryBasis::Declared,
                variant: None,
            });
        }
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
        let Some(symbol) = self.handler_symbol(*view) else {
            return;
        };
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: Some(symbol),
            framework: "django",
            route: Some(route("ANY", join_path("", &self.string_of(*first)))),
            guards: Vec::new(),
            basis: EntryBasis::Registered,
            variant: None,
        });
    }

    fn ecma(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() != "call_expression" {
            return;
        }
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
        if !HTTP_VERBS.contains(&verb) || !self.mentions("express") {
            return;
        }
        let Some(receiver) = callee.child_by_field_name("object") else {
            return;
        };
        if !self.is_framework_value(
            receiver,
            &["express(", "express.Router(", "Router("],
            &[("express", &["Router"])],
        ) {
            return;
        }
        let arguments = Self::arguments_of(node);
        let Some(first) = arguments.first() else {
            return;
        };
        if !self.is_string(*first) {
            return;
        }
        let path = self.string_of(*first);
        if !path.starts_with('/') {
            return;
        }
        let Some(handler) = arguments.last().filter(|_| arguments.len() > 1) else {
            return;
        };
        let guards: Vec<String> = arguments[1..arguments.len() - 1]
            .iter()
            .filter(|argument| matches!(argument.kind(), "identifier" | "member_expression"))
            .map(|argument| self.text(*argument).to_string())
            .collect();
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: self.handler_symbol(*handler),
            framework: "express",
            route: Some(route(if verb == "all" { "ANY" } else { verb }, path)),
            guards,
            basis: EntryBasis::Registered,
            variant: None,
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
        });
    }

    fn go(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() == "source_file" {
            go_routes::collect(self, node, out);
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
        let wrapped = self.go_handler(*handler, go_routes::HandlerKind::Http);
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: wrapped.symbol,
            framework: "net/http",
            route: Some(Route {
                method,
                parameters: go_routes::servemux_parameters(&path),
                path,
            }),
            guards: wrapped.guards,
            basis: EntryBasis::Registered,
            variant: None,
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
            });
        }
    }

    /// `.route("/path", get(handler))` of axum. The method router can chain more methods.
    /// The `net/http` package itself, or a value of `http.NewServeMux()`.
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
        if node.kind() != "call_expression" {
            return;
        }
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        let handler = Self::arguments_of(node).first().copied();
        match function.kind() {
            "identifier" if HTTP_VERBS.contains(&self.text(function)) => {
                out.push((self.text(function).to_uppercase(), handler));
            }
            "field_expression" => {
                if let Some(field) = function.child_by_field_name("field") {
                    if HTTP_VERBS.contains(&self.text(field)) {
                        out.push((self.text(field).to_uppercase(), handler));
                    }
                }
                if let Some(value) = function.child_by_field_name("value") {
                    self.method_router(value, out);
                }
            }
            _ => {}
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
    if context.is_string(node) {
        out.push(context.string_of(node));
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    for child in children {
        collect_strings(child, context, out);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::core::Language;
    use crate::index::extract::{extract_file, link_files};
    use crate::index::imports::{resolve_imports, Manifests};
    use crate::index::resolve::resolve_references;
    use crate::parser::Parser;

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
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert!(trees.tree(0).is_some()));
            }
        });
        assert_eq!(parses.load(Ordering::SeqCst), 1);
    }

    fn entries(language: Language, files: &[(&str, &str)]) -> Vec<EntryPoint> {
        entries_timed(language, files, true).0
    }

    /// Like `entries`, without the resolution of the references. The search for Rails entry points
    /// does not use them, and the resolution of a large input takes much more time than the search.
    fn entries_lean(
        language: Language,
        files: &[(&str, &str)],
    ) -> (Vec<EntryPoint>, std::time::Duration) {
        entries_timed(language, files, false)
    }

    /// The entry points, and the time of the search for entry points alone.
    fn entries_timed(
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
        let started = std::time::Instant::now();
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
                Context::new(&parsed[position], &index[position], &index, &trees, &[])
                    .detect()
                    .into_iter()
                    .map(|entry| entry.key)
            })
            .collect()
    }

    fn route(method: &str, path: &str, parameters: &[&str]) -> Option<Route> {
        Some(Route {
            method: method.to_string(),
            path: path.to_string(),
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
        let from_express = "import { Router } from \"express\";\n\nRouter.get(\"/x\", handler);\n";
        assert!(entries(Language::JavaScript, &[("a.js", from_express)])
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
                    route.path,
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

    #[test]
    fn timing_ten_thousand_handlers_and_registrations_are_read_in_linear_time() {
        let mut body = String::new();
        for index in 0..10_000 {
            body.push_str(&format!("func h{index}(c *gin.Context) {{}}\n\n"));
        }
        body.push_str("func routes() {\n\tr := gin.New()\n");
        for index in 0..10_000 {
            body.push_str(&format!("\tr.GET(\"/{index}\", h{index})\n"));
        }
        body.push_str("}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 10_000);
        assert!(found.iter().all(|entry| entry.symbol.is_some()));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
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
    fn timing_a_function_with_many_returned_routers_is_read_in_linear_time() {
        let mut body = String::from("func pick(n int) *chi.Mux {\n");
        for index in 0..5_000 {
            body.push_str(&format!(
                "\tif n == {index} {{\n\t\tr{index} := chi.NewRouter()\n\t\treturn r{index}\n\t}}\n"
            ));
        }
        body.push_str("\treturn nil\n}\n");
        let source = go_source(&[CHI], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_a_chain_of_ten_thousand_mounts_is_read_in_linear_time() {
        let mut body = String::from("func chain() {\n");
        for index in 0..=10_000 {
            body.push_str(&format!("\tr{index} := chi.NewRouter()\n"));
        }
        for index in 0..10_000 {
            body.push_str(&format!("\tr{index}.Mount(\"/x\", r{})\n", index + 1));
        }
        body.push_str("}\n");
        let source = go_source(&[CHI], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_many_packages_with_one_handler_name_are_read_in_linear_time() {
        let count = 2_000;
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
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &borrowed);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry
            .symbol
            .as_deref()
            .is_some_and(|symbol| symbol.ends_with("/show.go#Show"))));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "took {:?}",
            started.elapsed()
        );
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

    #[test]
    fn timing_ten_thousand_wrapped_handlers_are_read_in_linear_time() {
        let mut body =
            String::from("func wrap(next http.HandlerFunc) http.HandlerFunc { return next }\n\n");
        for index in 0..10_000 {
            body.push_str(&format!(
                "func h{index}(w http.ResponseWriter, r *http.Request) {{}}\n\n"
            ));
        }
        body.push_str("func routes() {\n");
        for index in 0..10_000 {
            body.push_str(&format!(
                "\thttp.HandleFunc(\"/{index}\", wrap(h{index}))\n"
            ));
        }
        body.push_str("}\n");
        let source = go_source(&[], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 10_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_many_factories_with_one_name_that_are_mentioned_are_read_in_linear_time() {
        let count = 5_000;
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
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &borrowed);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_one_router_that_many_parents_mount_is_read_in_linear_time() {
        let count = 5_000;
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
        let source = go_source(&[CHI], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
    }
    fn go_parameters(files: &[(&str, &str)]) -> Vec<String> {
        entries(Language::Go, files)
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .map(|entry| {
                let route = entry.route.unwrap();
                format!("{} {} {:?}", route.method, route.path, route.parameters)
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
    fn timing_ten_thousand_servemux_variables_are_read_in_linear_time() {
        let mut body = String::from("func routes() {\n");
        for index in 0..10_000 {
            body.push_str(&format!(
                "\tm{index} := http.NewServeMux()\n\tm{index}.HandleFunc(\"/{index}\", list)\n"
            ));
        }
        body.push_str("}\n");
        let source = go_source(&[], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 10_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_a_chain_of_twenty_thousand_groups_is_read_in_linear_time() {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n\tr0.GET(\"/0\", list)\n");
        for index in 1..20_000 {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"\")\n\tr{index}.GET(\"/{index}\", list)\n",
                previous = index - 1
            ));
        }
        body.push_str("}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 20_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_a_pattern_with_a_hundred_thousand_wildcards_is_read_in_linear_time() {
        let mut pattern = String::new();
        for index in 0..100_000 {
            pattern.push_str(&format!("/{{p{index}}}"));
        }
        let started = std::time::Instant::now();
        assert!(go_routes::servemux_pattern(&pattern).is_some());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_many_conditional_returns_of_nested_groups_are_read_in_linear_time() {
        let mut body = String::from("func pick(n int) *gin.RouterGroup {\n\tr0 := gin.New()\n");
        for index in 1..30_000 {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"\")\n\tif n == {index} {{\n\t\treturn r{index}\n\t}}\n",
                previous = index - 1
            ));
        }
        body.push_str("\treturn nil\n}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "took {:?}",
            started.elapsed()
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
    fn timing_two_long_method_lists_are_intersected_in_linear_time() {
        let tokens: Vec<String> = (0..100_000)
            .map(|index| format!("\"M{index:05}\""))
            .collect();
        let list = tokens.join(", ");
        let body = format!(
            "func routes() {{\n\tr := mux.NewRouter()\n\tr.Methods({list}).Methods({list}).Path(\"/x\").HandlerFunc(list)\n}}\n"
        );
        let source = go_source(&[MUX], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 100_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "took {:?}",
            started.elapsed()
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
    fn timing_ten_thousand_nested_gin_groups_join_paths_in_linear_time() {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n");
        for index in 1..=60_000 {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"/x\")\n",
                previous = index - 1
            ));
        }
        body.push_str("\tr60000.GET(\"/end\", list)\n}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 1);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_a_chain_of_forty_thousand_aliases_is_resolved_in_linear_time() {
        let mut body = String::from("type Base struct{ *chi.Mux }\n\ntype A00000 = Base\n");
        for index in 1..=100_000 {
            body.push_str(&format!(
                "type A{index:05} = A{previous:05}\n",
                previous = index - 1
            ));
        }
        body.push_str(
            "\nfunc New() *A100000 {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", list)\n\treturn &A100000{Mux: r}\n}\n",
        );
        let source = go_source(&[CHI], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 1);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(6),
            "took {:?}",
            started.elapsed()
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
    fn timing_many_gorilla_routes_are_ordered_in_near_linear_time() {
        let mut body = String::from("func routes() {\n\tr := mux.NewRouter()\n");
        for index in 0..60_000 {
            body.push_str(&format!("\tr.HandleFunc(\"/p{index}/x\", list)\n"));
        }
        body.push_str("}\n");
        let source = go_source(&[MUX], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 60_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "took {:?}",
            started.elapsed()
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
    fn timing_ten_thousand_nested_gin_groups_with_a_slash_prefix_cost_linear_time() {
        let mut body = String::from("func routes() {\n\tr0 := gin.New()\n");
        for index in 1..=50_000 {
            body.push_str(&format!(
                "\tr{index} := r{previous}.Group(\"/\")\n\tr{index}.GET(\"/{index}\", list)\n",
                previous = index - 1
            ));
        }
        body.push_str("}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert_eq!(found.len(), 50_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "took {:?}",
            started.elapsed()
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
    fn documented_gap_a_caller_in_another_file_that_registers_on_a_factory_result_gives_no_entry() {
        let router = factory_file("NewRouter", "/x");
        let main = go_file(
            &["\"net/http\"", CHI],
            "func serve() {\n\tr := NewRouter()\n\tr.Get(\"/y\", nil)\n\thttp.ListenAndServe(\":80\", r)\n\thttp.ListenAndServe(\":81\", NewRouter())\n}\n",
        );
        assert_eq!(
            go_routes(&[("router.go", &router), ("main.go", &main)]),
            ["chi GET /x [] list"]
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
    fn timing_ten_thousand_functions_in_one_file_are_read_in_linear_time() {
        let mut body = String::new();
        for index in 0..10_000 {
            body.push_str(&format!(
                "func make{index}() *chi.Mux {{\n\tr := chi.NewRouter()\n\treturn r\n}}\n\n"
            ));
        }
        let source = go_source(&[CHI], &body);
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &[("main.go", &source)]);
        assert!(found.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
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
    fn timing_five_thousand_files_that_call_one_factory_are_read_in_linear_time() {
        let router = factory_file("NewRouter", "/x");
        let mut files: Vec<(String, String)> = vec![("router.go".to_string(), router)];
        for index in 0..5_000 {
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
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let started = std::time::Instant::now();
        let found = entries(Language::Go, &borrowed);
        assert_eq!(found.len(), 1);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "took {:?}",
            started.elapsed()
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
        assert_eq!(
            go_routes(&[("main.go", &source)]),
            ["chi GET /root [] list", "chi GET /extra [] show"]
        );
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
        assert_eq!(go_routes(&[("main.go", &source)]), ["chi GET /own [] show"]);
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
    fn timing_ten_thousand_calls_of_one_factory_are_read_in_linear_time() {
        let mut body = String::from("func newRouter() *gin.Engine { return gin.New() }\n\n");
        body.push_str("func routes() {\n");
        for _ in 0..10_000 {
            body.push_str("\t{ r := newRouter(); r.GET(\"/x\", show) }\n");
        }
        body.push_str("}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = go_routes(&[("main.go", &source)]);
        assert_eq!(found.len(), 10_000);
        assert!(found.iter().all(|line| line == "gin GET /x [] show"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timing_a_chain_of_factories_that_call_each_other_twice_is_read_in_linear_time() {
        let mut body = String::from("func f0() *gin.Engine { return gin.New() }\n\n");
        for step in 1..=30 {
            body.push_str(&format!(
                "func f{step}() *gin.Engine {{\n\ta := f{previous}()\n\t_ = f{previous}()\n\treturn a\n}}\n\n",
                previous = step - 1
            ));
        }
        body.push_str("func routes() {\n\tr := f30()\n\tr.GET(\"/x\", show)\n}\n");
        let source = go_source(&[GIN], &body);
        let started = std::time::Instant::now();
        let found = go_routes(&[("main.go", &source)]);
        assert_eq!(found, ["gin GET /x [] show"]);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "took {:?}",
            started.elapsed()
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
                format!("{} {}", route.method, route.path)
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
                route.method == "GET" && route.path == "/users/:id"
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
            .find(|entry| entry.route.as_ref().unwrap().path.ends_with("/preview"))
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
            .find(|entry| entry.route.as_ref().unwrap().path == "/pong")
            .unwrap();
        assert_eq!(
            pong.symbol.as_deref(),
            Some("health.rb#V1.HealthController.pong")
        );
        let ping = found
            .iter()
            .find(|entry| entry.route.as_ref().unwrap().path == "/api/ping")
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
            let path = &entry.route.as_ref().unwrap().path;
            let wanted = match path.as_str() {
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
                    format!("{} {}", route.method, route.path),
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
            .find(|entry| entry.route.as_ref().unwrap().path == "/u")
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
        assert_eq!(paths, ["GET /app", "GET /engine", "GET /namespaced"]);
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
                .find(|entry| entry.route.as_ref().unwrap().path == format!("/{action}"))
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
        assert_eq!(found.len(), count);
        assert!(found
            .iter()
            .all(|entry| entry.symbol.is_some() && entry.guards == ["g"]));
        elapsed
    }

    #[test]
    fn many_engines_each_resolve_their_controller() {
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
        assert_eq!(paths, ["GET /app", "GET /compact", "GET /engine"]);
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
        assert_eq!(found, ["GET /posts", "GET /posts/new", "GET /users/new"]);
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
        let shadow = "module ActiveSupport\n  module Inflector\n  end\nend\n";
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
        assert_eq!(found, ["GET /users/:id/edit", "GET /users/new"]);
        let application = "Rails.application.config.api_only = true\n";
        let found = paths(&[
            ("config/routes.rb", app_routes),
            ("blog/config/routes.rb", engine_routes),
            ("config/application.rb", classes),
            ("config/app.rb", application),
        ]);
        assert_eq!(found, ["GET /posts/:id/edit", "GET /posts/new"]);
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
        assert_eq!(found[0].guards, Vec::<String>::new());
        assert_eq!(found[1].guards, ["base", "own"]);
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
        let path = &found[0].route.as_ref().unwrap().path;
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
                .find(|entry| entry.route.as_ref().unwrap().path == path)
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
        assert_eq!(found[0].guards, ["owner"]);
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
        let routes = "Rails.application.routes.draw do\n  name = \"dynamic\"\n  get \"/#{name}\", to: \"a#b\"\n  %w[a b].each do |part|\n    get \"/#{part}\", to: \"a#b\"\n  end\n  [:c].each do |part|\n    resources part\n  end\n  concern :commentable do\n    get \"/comments\", to: \"comments#index\"\n  end\n  resources :things, concerns: :commentable, only: [:index]\n  draw(:admin)\n  mount Sidekiq::Web => \"/sidekiq\"\n  resources :users, only: only_actions\n  resources :slugs, param: :slug\n  resources name\n  get \"/kept\", to: \"a#b\"\nend\n";
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
