//! Symbol extraction. One walk of the syntax tree gives the symbols of a file.
//!
//! A [`LanguageSpec`] supplies a table of definition node kinds. The hook
//! methods of the specification are only for syntax that the table cannot
//! describe.

use std::collections::{BTreeMap, HashMap};

use tree_sitter::Node;

use super::lang;
use super::model::{symbol_key, Exported, Span, Symbol, SymbolKind};
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
}

pub struct Context<'a, 'tree> {
    /// The path of the file, as the keys have it.
    pub file: &'a str,
    pub source: &'a [u8],
    pub scopes: &'a [Scope<'tree>],
}

impl<'tree> Context<'_, 'tree> {
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

    /// Corrects the draft that the table gave. If the result is `false`, the
    /// node is not a definition.
    fn refine<'tree>(&self, _draft: &mut Draft<'tree>, _context: &Context<'_, 'tree>) -> bool {
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported;

    /// Gives the scope that a node opens when the node is not a definition.
    fn scope<'tree>(
        &self,
        _node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        None
    }
}

/// Gives the specification of a language.
pub fn language_spec(language: Language) -> Option<&'static dyn LanguageSpec> {
    match language {
        Language::Go => Some(&lang::go::Go),
        Language::Rust => Some(&lang::rust::Rust),
        Language::Python => Some(&lang::python::Python),
        Language::JavaScript | Language::Jsx => Some(&lang::ecma::JAVASCRIPT),
        Language::TypeScript | Language::Tsx => Some(&lang::ecma::TYPESCRIPT),
        Language::Java => Some(&lang::java::Java),
        _ => None,
    }
}

/// The first descendant of `node`, or `node` itself, that has one of the kinds.
pub fn find_descendant<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'tree>> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find_map(|child| find_descendant(child, kinds))
}

/// The nearest nodes below `node` that have one of the kinds. The search does
/// not go into a node that it collects.
pub fn collect_outermost<'tree>(node: Node<'tree>, kinds: &[&str], out: &mut Vec<Node<'tree>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            out.push(child);
        } else {
            collect_outermost(child, kinds, out);
        }
    }
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn signature(draft: &Draft<'_>, source: &[u8]) -> String {
    let start = draft.node.start_byte();
    let end = match draft.body {
        Some(body) if body.start_byte() > start => body.start_byte(),
        _ => draft.node.end_byte(),
    };
    let text = String::from_utf8_lossy(&source[start..end]);
    match draft.body {
        Some(_) => collapse_whitespace(&text),
        None => collapse_whitespace(text.lines().next().unwrap_or("")),
    }
}

/// The comments that end on the line before the definition, with no line between them.
fn doc(node: Node<'_>, comment_kinds: &[&str], source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut next_start_row = node.start_position().row;
    let mut previous = node.prev_sibling();
    while let Some(comment) = previous {
        if !comment_kinds.contains(&comment.kind())
            || comment.end_position().row + 1 != next_start_row
        {
            break;
        }
        lines.push(comment.utf8_text(source).unwrap_or("").to_string());
        next_start_row = comment.start_position().row;
        previous = comment.prev_sibling();
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

struct Extractor<'a, 'tree> {
    spec: &'a dyn LanguageSpec,
    rules: HashMap<&'static str, DefinitionRule>,
    file: &'a str,
    source: &'a [u8],
    scopes: Vec<Scope<'tree>>,
    scope_depths: Vec<usize>,
    found: Vec<(Draft<'tree>, Exported)>,
}

impl<'tree> Extractor<'_, 'tree> {
    fn draft(&self, node: Node<'tree>, rule: &DefinitionRule) -> Option<(Draft<'tree>, Exported)> {
        let context = Context {
            file: self.file,
            source: self.source,
            scopes: &self.scopes,
        };
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
        let mut draft = Draft {
            node,
            name_node,
            name: context.text(name_node),
            kind,
            container: container.map(|scope| scope.name.clone()),
            container_path: context.path(),
            receiver_type: None,
            supertypes: Vec::new(),
            span: Span::of(&node),
            body: node.child_by_field_name("body"),
            role: ScopeRole::Function,
        };
        if draft.kind == SymbolKind::Module {
            draft.role = ScopeRole::Module;
        } else if draft.kind.is_type() {
            draft.role = ScopeRole::Type;
        }
        if !self.spec.refine(&mut draft, &context) || draft.name.is_empty() {
            return None;
        }
        let exported = self.spec.exported(&draft, &context);
        Some((draft, exported))
    }

    fn enter(&mut self, node: Node<'tree>, depth: usize) {
        // A keyword token can have the same kind string as a named node, for example `class`.
        if !node.is_named() {
            return;
        }
        let scope = match self.rules.get(node.kind()).copied() {
            Some(rule) => self.draft(node, &rule).map(|(draft, exported)| {
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
                scope
            }),
            None => self.spec.scope(
                node,
                &Context {
                    file: self.file,
                    source: self.source,
                    scopes: &self.scopes,
                },
            ),
        };
        if let Some(scope) = scope {
            self.scopes.push(scope);
            self.scope_depths.push(depth);
        }
    }

    fn leave(&mut self, depth: usize) {
        if self.scope_depths.last() == Some(&depth) {
            self.scope_depths.pop();
            self.scopes.pop();
        }
    }

    /// The walk does not use recursion, because generated source can have a very deep tree.
    fn walk(&mut self, root: Node<'tree>) {
        let mut cursor = root.walk();
        let mut depth = 0;
        loop {
            self.enter(cursor.node(), depth);
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                self.leave(depth);
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
                depth -= 1;
            }
        }
    }
}

/// Extracts the symbols of one file in source order.
///
/// `file` is the path that the keys use. If the language has no
/// specification, the result is empty.
pub fn extract_symbols(parsed: &ParseResult, file: &str) -> Vec<Symbol> {
    let Some(spec) = language_spec(parsed.language) else {
        return Vec::new();
    };
    let mut extractor = Extractor {
        spec,
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
    };
    extractor.walk(parsed.root_node());

    let mut found = extractor.found;
    found.sort_by_key(|(draft, _)| (draft.span.start_byte, draft.span.end_byte));
    let mut ordinals: BTreeMap<(Vec<String>, String), usize> = BTreeMap::new();
    found
        .into_iter()
        .map(|(draft, exported)| {
            let ordinal = ordinals
                .entry((draft.container_path.clone(), draft.name.clone()))
                .or_insert(0);
            *ordinal += 1;
            Symbol {
                key: symbol_key(file, &draft.container_path, &draft.name, *ordinal),
                signature: signature(&draft, &parsed.source),
                doc: doc(draft.node, spec.comment_kinds(), &parsed.source),
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
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use super::*;
    use crate::parser::Parser;

    pub(crate) fn symbols(language: Language, source: &str) -> Vec<Symbol> {
        let parsed = Parser::new()
            .parse(source.as_bytes(), language, Path::new("file"))
            .unwrap();
        extract_symbols(&parsed, "file")
    }

    /// One line for each symbol: `kind name container start-end exported`.
    pub(crate) fn rows(language: Language, source: &str) -> Vec<String> {
        symbols(language, source)
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
            .map(|symbol| symbol.key)
            .collect()
    }
}
