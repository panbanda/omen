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
    /// The method in upper case, or `ANY`.
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

/// Like [`find_entry_points`], with the rules of the user.
pub fn find_entry_points_with(
    parsed: &[ParseResult],
    files: &[FileSymbols],
    rules: &[EntryRule],
) -> Vec<EntryPoint> {
    assert_eq!(parsed.len(), files.len(), "each file needs its syntax tree");
    let per_file: Vec<Vec<EntryPoint>> = (0..files.len())
        .into_par_iter()
        .map(|position| {
            let context = Context::new(&parsed[position], &files[position], files, parsed, rules);
            let mut found = context.detect();
            found.sort_by(|left, right| {
                (left.range.start_byte, &left.key).cmp(&(right.range.start_byte, &right.key))
            });
            found.dedup_by(|later, earlier| later.key == earlier.key);
            found
        })
        .collect();
    per_file.into_iter().flatten().collect()
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

struct Context<'a> {
    parsed: &'a ParseResult,
    file: &'a FileSymbols,
    all: &'a [FileSymbols],
    all_parsed: &'a [ParseResult],
    rules: &'a [EntryRule],
    source: &'a [u8],
    test_ranges: Vec<Range<usize>>,
}

struct Draft {
    range: Span,
    kind: EntryKind,
    symbol: Option<String>,
    framework: &'static str,
    route: Option<Route>,
    guards: Vec<String>,
    basis: EntryBasis,
}

impl<'a> Context<'a> {
    fn new(
        parsed: &'a ParseResult,
        file: &'a FileSymbols,
        all: &'a [FileSymbols],
        all_parsed: &'a [ParseResult],
        rules: &'a [EntryRule],
    ) -> Self {
        let test_ranges = if file.language == Language::Rust {
            let text = std::str::from_utf8(&parsed.source).unwrap_or("");
            cfg_test_module_ranges(&parsed.root_node(), text)
        } else {
            Vec::new()
        };
        Self {
            parsed,
            file,
            all,
            all_parsed,
            rules,
            source: &parsed.source,
            test_ranges,
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
                .is_some_and(|position| {
                    let parsed = &self.all_parsed[position];
                    bound_to_marker(parsed.root_node(), &parsed.source, name, markers)
                })
        })
    }

    fn symbol_at(&self, name_node: Node<'_>) -> Option<String> {
        self.file
            .symbols
            .iter()
            .find(|symbol| symbol.name_range.start_byte == name_node.start_byte())
            .map(|symbol| symbol.key.clone())
    }

    /// The one function or method with the name. A definition in the file wins over a definition in another file.
    fn symbol_named(&self, name: &str, container: Option<&str>) -> Option<String> {
        let matching = |file: &'a FileSymbols| -> Vec<&'a Symbol> {
            file.symbols
                .iter()
                .filter(|symbol| {
                    matches!(
                        symbol.kind,
                        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                    ) && symbol.name == name
                        && container
                            .is_none_or(|wanted| symbol.container.as_deref() == Some(wanted))
                })
                .collect()
        };
        let here = matching(self.file);
        if here.len() == 1 {
            return Some(here[0].key.clone());
        }
        let everywhere: Vec<&Symbol> = self.all.iter().flat_map(matching).collect();
        (everywhere.len() == 1).then(|| everywhere[0].key.clone())
    }

    fn handler_symbol(&self, handler: Node<'_>) -> Option<String> {
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
        drafts
            .into_iter()
            .map(|draft| {
                let method = draft
                    .route
                    .as_ref()
                    .map(|route| format!(":{}", route.method))
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
            .collect()
    }

    fn walk(&self, node: Node<'_>, out: &mut Vec<Draft>) {
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
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
        for child in children {
            self.walk(child, out);
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
                        symbol: Some(symbol.key.clone()),
                        framework: rule.framework,
                        route: None,
                        guards: Vec::new(),
                        basis: EntryBasis::Convention,
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
                    symbol: Some(symbol.key.clone()),
                    framework,
                    route: None,
                    guards: Vec::new(),
                    basis: EntryBasis::Convention,
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
                    symbol: Some(symbol.key.clone()),
                    framework: "next",
                    route: Some(route(verb, path.clone())),
                    guards: Vec::new(),
                    basis: EntryBasis::Convention,
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
        });
    }

    fn go(&self, node: Node<'_>, out: &mut Vec<Draft>) {
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
        let path = self.string_of(*first);
        let (framework, method, guards) = if matches!(name, "HandleFunc" | "Handle")
            && self.imports("net.http")
            && self.is_go_http_receiver(receiver)
        {
            let method = match path.split_once(' ') {
                Some((method, _)) if method.chars().all(|c| c.is_ascii_uppercase()) => {
                    method.to_string()
                }
                _ => "ANY".to_string(),
            };
            ("net/http", method, Vec::new())
        } else if matches!(
            name,
            "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
        ) && self.imports("github.com.gin-gonic.gin")
            && self.is_framework_value(
                receiver,
                &["gin.", ".Group("],
                &[("github.com.gin-gonic.gin", &["Default", "New"])],
            )
        {
            let guards = arguments[1..arguments.len() - 1]
                .iter()
                .filter(|argument| matches!(argument.kind(), "identifier" | "selector_expression"))
                .map(|argument| self.text(*argument).to_string())
                .collect();
            ("gin", name.to_string(), guards)
        } else {
            return;
        };
        let path = path
            .split_once(' ')
            .map_or(path.clone(), |(_, rest)| rest.to_string());
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol: self.handler_symbol(*handler),
            framework,
            route: Some(route(&method, path)),
            guards,
            basis: EntryBasis::Registered,
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
            });
        }
    }

    /// `.route("/path", get(handler))` of axum. The method router can chain more methods.
    /// The `net/http` package itself, or a value of `http.NewServeMux()`.
    fn is_go_http_receiver(&self, receiver: Node<'_>) -> bool {
        let text = self.text(receiver);
        self.file.imports.iter().any(|binding| {
            binding.module_path == "net/http"
                && binding
                    .local_name
                    .as_deref()
                    .map_or(text == "http", |local| local == text)
        }) || (receiver.kind() == "identifier"
            && bound_to_marker(
                self.parsed.root_node(),
                self.source,
                text,
                &["NewServeMux("],
            ))
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

    fn ruby(&self, node: Node<'_>, out: &mut Vec<Draft>) {
        if node.kind() != "call" || !self.file.file.ends_with("routes.rb") {
            return;
        }
        let Some(method) = node.child_by_field_name("method") else {
            return;
        };
        let verb = self.text(method);
        if node.child_by_field_name("receiver").is_some()
            || !matches!(verb, "get" | "post" | "put" | "patch" | "delete")
        {
            return;
        }
        let arguments = Self::arguments_of(node);
        let Some(path) = arguments
            .iter()
            .find(|argument| argument.kind() == "string")
            .map(|argument| self.string_of(*argument))
        else {
            return;
        };
        let target = arguments
            .iter()
            .filter(|argument| argument.kind() == "pair")
            .find(|pair| {
                pair.child_by_field_name("key")
                    .is_some_and(|key| self.text(key).trim_end_matches(':') == "to")
            })
            .and_then(|pair| pair.child_by_field_name("value"))
            .map(|value| self.string_of(value));
        let mut symbol = None;
        let mut guards = Vec::new();
        if let Some((controller, action)) = target.as_deref().and_then(|text| text.split_once('#'))
        {
            let class = format!(
                "{}Controller",
                controller
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .split('_')
                    .map(|part| {
                        let mut characters = part.chars();
                        characters
                            .next()
                            .map(|first| {
                                first.to_uppercase().collect::<String>() + characters.as_str()
                            })
                            .unwrap_or_default()
                    })
                    .collect::<String>()
            );
            symbol = self.symbol_named(action, Some(&class));
            guards = self.rails_guards(&class);
        }
        out.push(Draft {
            range: Span::of(&node),
            kind: EntryKind::HttpHandler,
            symbol,
            framework: "rails",
            route: Some(route(verb, join_path("", &path))),
            guards,
            basis: EntryBasis::Registered,
        });
    }

    /// The `before_action` callbacks in the body of a controller class, in any file.
    fn rails_guards(&self, class: &str) -> Vec<String> {
        let mut guards = Vec::new();
        for (file, parsed) in self.all.iter().zip(self.all_parsed) {
            if file.language != Language::Ruby {
                continue;
            }
            let Some(symbol) = file
                .symbols
                .iter()
                .find(|symbol| symbol.name == class && symbol.kind.is_type())
            else {
                continue;
            };
            let source = String::from_utf8_lossy(&parsed.source);
            for reference in &file.references {
                let inside = reference.name_range.start_byte >= symbol.range.start_byte
                    && reference.name_range.end_byte <= symbol.range.end_byte;
                if !inside || !matches!(reference.name.as_str(), "before_action" | "before_filter")
                {
                    continue;
                }
                let line = source
                    .lines()
                    .nth(reference.name_range.start_line as usize - 1);
                let rest = line
                    .and_then(|line| line.trim().split_once(' '))
                    .map(|pair| pair.1);
                guards.extend(
                    rest.unwrap_or("")
                        .split(',')
                        .map(str::trim)
                        .filter_map(|part| part.strip_prefix(':'))
                        .map(str::to_string),
                );
            }
        }
        guards
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

    fn entries(language: Language, files: &[(&str, &str)]) -> Vec<EntryPoint> {
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
        resolve_imports(&mut index, &Manifests::default());
        resolve_references(&mut index);
        find_entry_points(&parsed, &index)
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
