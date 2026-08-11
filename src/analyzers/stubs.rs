//! Stubs analyzer - detect incomplete / placeholder implementations.
//!
//! "Stubs" are code that parses successfully but was never finished: explicit
//! not-implemented idioms (`todo!()`, `raise NotImplementedError`), comments
//! that admit skipped work ("...rest of implementation", "for brevity"), and
//! function bodies that are empty when they can't legitimately be.
//!
//! Distinct from SATD (`src/analyzers/satd.rs`): SATD is acknowledged debt a
//! developer chose to keep (`// TODO: rename this`); a stub is unfinished
//! work. All detection is AST-based (tree-sitter node kinds and comment
//! nodes), never string-matching over whole file contents, so string/char
//! literals that happen to contain trigger words never false-positive.
//!
//! Precision notes (see the analyzer's design report for the full
//! rationale):
//! - Comment-based ("elision") detection only fires for comments inside an
//!   executable function/method *body* -- module docs, item docs, and any
//!   comment outside a body are structurally out of scope, which also
//!   excludes every language's doc-comment convention (they precede the
//!   declaration, outside its body).
//! - Ambiguous single-word markers ("placeholder", bare "stub") only count
//!   when paired with independent evidence of unfinished work in the same
//!   function (an empty body, or a `not_implemented` finding in that
//!   function); unambiguous phrases ("for brevity", "rest of the
//!   implementation", ...) count unconditionally.
//! - `not_implemented` message matching inspects only string-*literal* AST
//!   nodes (never identifiers, format-call results, or raw argument-list
//!   text), so a variable or function named `stub_count`/`todoMessage` can
//!   never trigger a match.
//! - A single unfinished site (e.g. an empty function whose only content is
//!   an elision comment) is reported once, with every matching category
//!   attached as metadata, rather than once per category.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result, SourceFile};
use crate::parser::queries::get_comment_node_types;
use crate::parser::Parser as TsParser;

/// Unambiguous "skipped work" phrasing: always treated as a stub when found
/// in a comment inside a function body, regardless of what else is in that
/// function.
///
/// Deliberately narrower than SATD's generic TODO/FIXME markers -- a bare
/// `// TODO: rename var` is SATD, not a stub. Only "unfinished work"
/// phrasing matches here.
static ELISION_STRONG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(",
        r"rest of (the )?(code|implementation|method|function|file)",
        r"|for brevity",
        r"|implementation omitted",
        r"|omitted for",
        r"|\.\.\.\s?(existing|unchanged)",
        r"|\(unchanged\)",
        r"|keep (the )?existing",
        r"|your code here",
        r"|fill (this )?in",
        r"|implement (this|me)",
        r"|\bnot (yet )?implemented\b",
        r"|\btodo:?\s*implement",
        r"|\bfixme:?\s*implement",
        r")"
    ))
    .expect("valid strong elision pattern")
});

/// Ambiguous single-word/short markers: common in ordinary explanatory
/// comments ("use a placeholder root", "generic non-nil placeholder") as
/// well as genuine stubs, so these only count when paired with independent
/// evidence of unfinished work in the same function (see `finalize`).
static ELISION_WEAK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bplaceholder\b|\bstub(bed)?( out)?\b").expect("valid weak elision pattern")
});

/// Message regex for not-implemented idioms that carry a free-form string.
/// Applied ONLY to decoded string-literal AST node text (see
/// `first_string_literal_text`), never raw argument-list/token-tree text, so
/// identifiers and format-call results can never match. Bounded with `\b` so
/// e.g. `Stubs` (no trailing boundary after "stub") or `TODO_CONST` never
/// match, while a bare `"stub"` message still does.
static NOT_IMPLEMENTED_MSG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bnot yet implemented\b|\bnot implemented\b|\bunimplemented\b|\btodo\b|\bfixme\b|\bstub\b",
    )
    .expect("valid not-implemented message pattern")
});

/// Stubs analyzer.
pub struct Analyzer;

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new stubs analyzer.
    pub fn new() -> Self {
        Self
    }

    /// Analyze a single file for stubs.
    pub fn analyze_file(&self, file: &SourceFile) -> Vec<Stub> {
        let parser = TsParser::new();
        let Ok(parsed) = parser.parse(&file.content, file.language, &file.path) else {
            return Vec::new();
        };

        let mut raws = Vec::new();
        walk(
            parsed.root_node(),
            &parsed.source,
            file.language,
            &file.path,
            &mut raws,
        );
        finalize(raws, &parsed.source, file.language, &file.path)
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "stubs"
    }

    fn description(&self) -> &'static str {
        "Detect incomplete/placeholder implementations (todo!(), NotImplementedError, empty bodies)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        let files: Vec<_> = ctx.files.iter().collect();
        let mut stubs: Vec<Stub> = files
            .par_iter()
            .filter_map(|path| {
                let content = ctx.read_file(path).ok()?;
                let language = Language::detect(path)?;
                Some(SourceFile::from_content(*path, language, content))
            })
            .map(|file| self.analyze_file(&file))
            .reduce(Vec::new, |mut acc, items| {
                acc.extend(items);
                acc
            });

        stubs.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

        // One increment per stub (already deduplicated to one finding per
        // site by `finalize`), keyed by its primary category, so counts here
        // reflect unique sites rather than raw pattern matches.
        let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
        for stub in &stubs {
            *by_category
                .entry(stub.category.as_str().to_string())
                .or_insert(0) += 1;
        }

        let summary = StubSummary {
            total_stubs: stubs.len(),
            high_severity: stubs
                .iter()
                .filter(|s| s.severity == Severity::High)
                .count(),
            medium_severity: stubs
                .iter()
                .filter(|s| s.severity == Severity::Medium)
                .count(),
            low_severity: stubs.iter().filter(|s| s.severity == Severity::Low).count(),
        };

        tracing::info!(
            "Stubs analysis completed in {:?}: {} stubs ({} high)",
            start.elapsed(),
            summary.total_stubs,
            summary.high_severity
        );

        Ok(Analysis {
            stubs,
            by_category,
            summary,
        })
    }
}

/// Full stubs analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// All stubs found (one entry per unique site).
    pub stubs: Vec<Stub>,
    /// Count of sites by primary category (deterministic key order).
    pub by_category: BTreeMap<String, usize>,
    /// Summary statistics.
    pub summary: StubSummary,
}

/// A single stub finding -- one unique unfinished-work site. When more than
/// one category applies to the same site (e.g. an empty function whose only
/// content is an elision comment), they are merged into a single `Stub`
/// rather than reported once per category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stub {
    /// File path (repo-relative).
    pub file: String,
    /// Primary line number (1-indexed) -- the function/declaration line for
    /// `empty_body` (and merged) findings, or the triggering node's own line
    /// otherwise.
    pub line: u32,
    /// Every line contributing to this finding (includes `line`), sorted and
    /// deduplicated. Used to reconcile against other analyzers (e.g. SATD)
    /// that may flag the same site under a different line.
    pub lines: Vec<u32>,
    /// Primary/canonical category, used for severity and gating.
    pub category: Category,
    /// Every category that matched at this site (metadata; includes
    /// `category`), sorted deterministically.
    pub categories: Vec<Category>,
    /// Severity level (max across `categories`).
    pub severity: Severity,
    /// Source snippet (first line, truncated).
    pub snippet: String,
    /// Language the stub was found in.
    pub language: Language,
}

/// Stub pattern category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Explicit not-implemented idiom (`todo!()`, `raise NotImplementedError`, ...).
    NotImplemented,
    /// Placeholder/"skipped work" comment marker.
    Elision,
    /// Empty-but-implementable function/method body.
    EmptyBody,
}

impl Category {
    fn as_str(self) -> &'static str {
        match self {
            Category::NotImplemented => "not_implemented",
            Category::Elision => "elision",
            Category::EmptyBody => "empty_body",
        }
    }

    /// Stable ordering for the `categories` metadata list.
    fn rank(self) -> u8 {
        match self {
            Category::NotImplemented => 0,
            Category::Elision => 1,
            Category::EmptyBody => 2,
        }
    }
}

/// Stub severity level.
///
/// Declaration order (Low < Medium < High) backs the derived `Ord`, used by
/// the CLI `--gate-severity` threshold comparison.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Ok(Severity::Low),
            "medium" => Ok(Severity::Medium),
            "high" => Ok(Severity::High),
            other => Err(format!("invalid severity: {other}")),
        }
    }
}

/// Analysis summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StubSummary {
    /// Total stubs found (unique sites).
    pub total_stubs: usize,
    /// Count at High severity.
    pub high_severity: usize,
    /// Count at Medium severity.
    pub medium_severity: usize,
    /// Count at Low severity.
    pub low_severity: usize,
}

// ---------------------------------------------------------------------------
// Pass 1: AST walk -> raw findings
// ---------------------------------------------------------------------------

/// A category match before site-level merging/gating has been applied.
enum RawKind {
    NotImplemented,
    /// `strong` distinguishes unambiguous phrasing (always kept) from
    /// ambiguous single-word markers (kept only if paired -- see `finalize`).
    Elision {
        strong: bool,
    },
    EmptyBody,
}

struct RawFinding<'a> {
    node: Node<'a>,
    kind: RawKind,
}

/// Depth-first walk checking every node against all three category matchers.
///
/// A single walk (rather than one per category) keeps this linear in AST
/// size; each matcher does a cheap `node.kind()` check before doing any real
/// work, so the extra checks per node are negligible. Results are raw
/// (possibly over-inclusive for ambiguous elision markers) and are resolved
/// by `finalize`.
fn walk<'a>(
    node: Node<'a>,
    source: &[u8],
    lang: Language,
    path: &Path,
    out: &mut Vec<RawFinding<'a>>,
) {
    if match_not_implemented(node, source, lang, path) {
        out.push(RawFinding {
            node,
            kind: RawKind::NotImplemented,
        });
    } else if match_empty_body(node, source, lang, path) {
        out.push(RawFinding {
            node,
            kind: RawKind::EmptyBody,
        });
    } else if is_comment_kind(node.kind(), lang) {
        if let Ok(text) = node.utf8_text(source) {
            if !is_doc_comment_text(text, lang) {
                if ELISION_STRONG_RE.is_match(text) {
                    out.push(RawFinding {
                        node,
                        kind: RawKind::Elision { strong: true },
                    });
                } else if ELISION_WEAK_RE.is_match(text) {
                    out.push(RawFinding {
                        node,
                        kind: RawKind::Elision { strong: false },
                    });
                }
            }
        }
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            walk(cursor.node(), source, lang, path, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 2: resolve raw findings into deduplicated, merged `Stub`s
// ---------------------------------------------------------------------------

/// Resolve raw findings into final stubs:
/// - Ambiguous ("weak") elision markers are dropped unless the comment's
///   enclosing function also has an `empty_body` or `not_implemented`
///   finding (independent evidence the function really is unfinished).
/// - An elision finding co-located with an `empty_body` finding at the same
///   function is merged into that function's single `Stub` (as metadata)
///   rather than reported separately.
fn finalize(raws: Vec<RawFinding>, source: &[u8], lang: Language, path: &Path) -> Vec<Stub> {
    let empty_body_keys: HashSet<usize> = raws
        .iter()
        .filter(|r| matches!(r.kind, RawKind::EmptyBody))
        .map(|r| r.node.start_byte())
        .collect();

    let not_implemented_keys: HashSet<usize> = raws
        .iter()
        .filter(|r| matches!(r.kind, RawKind::NotImplemented))
        .filter_map(|r| enclosing_function_body(r.node, lang))
        .map(|f| f.start_byte())
        .collect();

    let mut stubs: Vec<Stub> = Vec::new();
    let mut empty_body_index: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();

    for raw in &raws {
        match raw.kind {
            RawKind::NotImplemented => {
                stubs.push(make_stub(
                    raw.node,
                    source,
                    path,
                    lang,
                    Category::NotImplemented,
                    Severity::High,
                ));
            }
            RawKind::EmptyBody => {
                let key = raw.node.start_byte();
                empty_body_index.insert(key, stubs.len());
                stubs.push(make_stub(
                    raw.node,
                    source,
                    path,
                    lang,
                    Category::EmptyBody,
                    Severity::Medium,
                ));
            }
            RawKind::Elision { strong } => {
                let Some(func) = enclosing_function_body(raw.node, lang) else {
                    continue; // not inside any function body -- module/item-level comment
                };
                let key = func.start_byte();
                if !strong
                    && !empty_body_keys.contains(&key)
                    && !not_implemented_keys.contains(&key)
                {
                    continue; // ambiguous marker with no independent evidence of unfinished work
                }
                if let Some(&idx) = empty_body_index.get(&key) {
                    merge_elision_into(&mut stubs[idx], raw.node);
                } else {
                    stubs.push(make_stub(
                        raw.node,
                        source,
                        path,
                        lang,
                        Category::Elision,
                        Severity::Medium,
                    ));
                }
            }
        }
    }

    stubs.sort_by_key(|s| s.line);
    stubs
}

fn merge_elision_into(stub: &mut Stub, comment: Node) {
    if !stub.categories.contains(&Category::Elision) {
        stub.categories.push(Category::Elision);
        stub.categories.sort_by_key(|c| c.rank());
        stub.severity = stub.severity.max(Severity::Medium);
    }
    let line = comment.start_position().row as u32 + 1;
    if !stub.lines.contains(&line) {
        stub.lines.push(line);
        stub.lines.sort_unstable();
    }
}

fn make_stub(
    node: Node,
    source: &[u8],
    path: &Path,
    lang: Language,
    category: Category,
    severity: Severity,
) -> Stub {
    let text = node.utf8_text(source).unwrap_or("");
    let snippet: String = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect();
    let line = node.start_position().row as u32 + 1;
    Stub {
        file: path.to_string_lossy().to_string(),
        line,
        lines: vec![line],
        category,
        categories: vec![category],
        severity,
        snippet,
        language: lang,
    }
}

// ---------------------------------------------------------------------------
// Shared AST helpers
// ---------------------------------------------------------------------------

fn is_comment_kind(kind: &str, lang: Language) -> bool {
    get_comment_node_types(lang).contains(&kind)
}

/// Whether comment text uses a documentation-comment marker for `lang`.
///
/// This is a secondary safety net: doc comments conventionally precede a
/// declaration (outside its body), so `enclosing_function_body` already
/// excludes them structurally. This catches the rare case of doc-style
/// comment syntax written inside a body.
fn is_doc_comment_text(text: &str, lang: Language) -> bool {
    let t = text.trim_start();
    match lang {
        Language::Rust => {
            t.starts_with("///")
                || t.starts_with("//!")
                || t.starts_with("/**")
                || t.starts_with("/*!")
        }
        Language::CSharp => t.starts_with("///"),
        Language::Java
        | Language::TypeScript
        | Language::Tsx
        | Language::JavaScript
        | Language::Jsx
        | Language::Php
        | Language::C
        | Language::Cpp => t.starts_with("/**"),
        Language::Go | Language::Python | Language::Ruby | Language::Bash => false,
    }
}

/// Whether `kind` is a function/method declaration node for `lang`.
fn is_function_like(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => kind == "function_item",
        Language::Go => matches!(kind, "function_declaration" | "method_declaration"),
        Language::Python => kind == "function_definition",
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            matches!(kind, "function_declaration" | "method_definition")
        }
        Language::Java | Language::CSharp => kind == "method_declaration",
        Language::C | Language::Cpp => kind == "function_definition",
        Language::Php => matches!(kind, "function_definition" | "method_declaration"),
        Language::Ruby => kind == "method",
        Language::Bash => kind == "function_definition",
    }
}

/// Find the nearest enclosing function/method whose `body` field contains
/// `node` (directly or transitively). Returns `None` if `node` is not inside
/// any function body -- e.g. a module-level comment, or a comment sitting in
/// a function's signature rather than its body.
///
/// Two language-specific quirks:
/// - Ruby: tree-sitter-ruby omits the `body` field entirely when a method's
///   body is empty (a lone comment becomes a direct, unfielded child of the
///   `method` node), so there is no field to match against in that case.
/// - Python: a comment on the body's very first line is sometimes attached
///   as an extra child of `function_definition` itself, *before* the `body`
///   field, rather than nested inside `body`. Anything positioned after the
///   parameter list is still part of the body region regardless of how
///   tree-sitter parented it.
fn enclosing_function_body(node: Node, lang: Language) -> Option<Node> {
    let mut current = node;
    loop {
        let parent = current.parent()?;
        if is_function_like(parent.kind(), lang) {
            if lang == Language::Ruby {
                return Some(parent);
            }
            if let Some(body) = parent.child_by_field_name("body") {
                if body.id() == current.id() {
                    return Some(parent);
                }
            }
            if lang == Language::Python {
                if let Some(params) = parent.child_by_field_name("parameters") {
                    if current.start_byte() > params.end_byte() {
                        return Some(parent);
                    }
                }
            }
            return None;
        }
        current = parent;
    }
}

fn find_child_by_kind<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// Named, non-comment children of a body node (i.e. its statements).
fn stmt_children<'a>(body: Node<'a>, lang: Language) -> Vec<Node<'a>> {
    let mut cursor = body.walk();
    let children: Vec<Node<'a>> = body
        .named_children(&mut cursor)
        .filter(|c| !is_comment_kind(c.kind(), lang))
        .collect();
    children
}

/// Whether a body contains (only) a comment matching the elision wording
/// (strong or weak -- this feeds `empty_body`'s own detection, which already
/// requires the body to be otherwise empty, so ambiguous markers are safe
/// here regardless of strength).
fn body_has_elision_comment(body: Node, source: &[u8], lang: Language) -> bool {
    let mut cursor = body.walk();
    let matched = body
        .named_children(&mut cursor)
        .filter(|c| is_comment_kind(c.kind(), lang))
        .any(|c| {
            c.utf8_text(source).is_ok_and(|t| {
                !is_doc_comment_text(t, lang)
                    && (ELISION_STRONG_RE.is_match(t) || ELISION_WEAK_RE.is_match(t))
            })
        });
    matched
}

/// Kinds that represent a plain string literal, per language. Only exact
/// literal nodes are considered -- identifiers, calls, and interpolated
/// sub-expressions are deliberately excluded, so a variable named
/// `stub_count` or a runtime-built message can never trigger a match.
fn string_literal_kinds(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["string_literal"],
        Language::Go => &["interpreted_string_literal", "raw_string_literal"],
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => &["string"],
        Language::Java | Language::C | Language::Cpp => &["string_literal"],
        Language::CSharp => &[
            "string_literal",
            "verbatim_string_literal",
            "raw_string_literal",
        ],
        Language::Php => &["string", "encapsed_string"],
        Language::Python | Language::Ruby | Language::Bash => &[],
    }
}

/// Find the first direct string-literal child of `container` (unwrapping a
/// single level of C#/PHP's `argument` wrapper node) and return its raw text
/// (including quotes). Never recurses into nested expressions/sub-calls, so
/// e.g. `panic!("{}", stub_count)` only ever sees the literal `"{}"`, and
/// `panic(fmt.Sprintf(...))` sees no literal at all (correctly not matched).
fn first_string_literal_text(container: Node, source: &[u8], lang: Language) -> Option<String> {
    let kinds = string_literal_kinds(lang);
    if kinds.is_empty() {
        return None;
    }
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        let literal = if kinds.contains(&child.kind()) {
            Some(child)
        } else if child.kind() == "argument" {
            child
                .named_child(0)
                .filter(|inner| kinds.contains(&inner.kind()))
        } else {
            None
        };
        if let Some(literal) = literal {
            if let Ok(text) = literal.utf8_text(source) {
                return Some(text.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Python decorator/declaration-context suppression
// ---------------------------------------------------------------------------

/// `.pyi` stub files, `@overload`/`@abstractmethod`-decorated functions, and
/// methods on a `Protocol`/`ABC` subclass are legitimate bodyless/minimal
/// declarations, not unfinished work.
///
/// Matching is against the *whole* qualified identifier (see
/// `dotted_last_component`), never a substring: `@not_an_overload` and
/// `class Worker(ABCWidget)` must NOT be exempted just because their names
/// happen to contain "overload"/"ABC".
fn python_suppressed(node: Node, source: &[u8], path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) == Some("pyi") {
        return true;
    }
    let Some(func) = nearest_python_function(node) else {
        return false;
    };
    if let Some(parent) = func.parent() {
        if parent.kind() == "decorated_definition" {
            let mut cursor = parent.walk();
            let flagged_decorator = parent.named_children(&mut cursor).any(|child| {
                // `decorator`'s text includes the leading "@"; use its inner
                // expression child (identifier or dotted attribute) instead.
                child.kind() == "decorator"
                    && child
                        .named_child(0)
                        .and_then(|expr| expr.utf8_text(source).ok())
                        .is_some_and(|t| {
                            matches!(dotted_last_component(t), "overload" | "abstractmethod")
                        })
            });
            if flagged_decorator {
                return true;
            }
        }
    }
    let mut current = func;
    while let Some(parent) = current.parent() {
        if parent.kind() == "class_definition" {
            let Some(superclasses) = parent.child_by_field_name("superclasses") else {
                return false;
            };
            let mut cursor = superclasses.walk();
            return superclasses.named_children(&mut cursor).any(|base| {
                base.utf8_text(source)
                    .is_ok_and(|t| matches!(dotted_last_component(t), "Protocol" | "ABC"))
            });
        }
        current = parent;
    }
    false
}

/// Extract the trailing dotted-name component of a decorator/base-class
/// expression's text (e.g. `"typing.overload"` -> `"overload"`, `"ABC"` ->
/// `"ABC"`), stripping a trailing call if present (`"some.overload()"` ->
/// `"overload"`). Used so exemption checks compare a whole identifier rather
/// than a substring.
fn dotted_last_component(text: &str) -> &str {
    let text = text.trim();
    let text = text.split('(').next().unwrap_or(text).trim();
    text.rsplit('.').next().unwrap_or(text).trim()
}

fn nearest_python_function(node: Node) -> Option<Node> {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "function_definition" {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// not_implemented matchers
// ---------------------------------------------------------------------------

fn match_not_implemented(node: Node, source: &[u8], lang: Language, path: &Path) -> bool {
    match lang {
        Language::Rust => match_rust_not_implemented(node, source),
        Language::Python => match_python_not_implemented(node, source, path),
        Language::Go => match_go_not_implemented(node, source),
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => {
            match_js_not_implemented(node, source, lang)
        }
        Language::Java => match_java_not_implemented(node, source),
        Language::CSharp => match_csharp_not_implemented(node, source),
        Language::Ruby => match_ruby_not_implemented(node, source),
        Language::Php => match_php_not_implemented(node, source),
        Language::C | Language::Cpp => match_c_not_implemented(node, source),
        // Bash has no reliable not-implemented AST idiom; relies on the
        // elision-comment category instead (see module docs / final report).
        Language::Bash => false,
    }
}

/// Rust: `todo!()`, `unimplemented!()` (any args), and `panic!`/`unreachable!`
/// whose string-literal message matches the not-implemented wording.
fn match_rust_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "macro_invocation" {
        return false;
    }
    let Some(mac) = node.child_by_field_name("macro") else {
        return false;
    };
    let Ok(name) = mac.utf8_text(source) else {
        return false;
    };
    match name {
        "todo" | "unimplemented" => true,
        "panic" | "unreachable" => find_child_by_kind(&node, "token_tree")
            .and_then(|args| first_string_literal_text(args, source, Language::Rust))
            .is_some_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(&text)),
        _ => false,
    }
}

/// Python: `raise NotImplementedError` (with or without call), and a function
/// body whose only statement is a bare `...` (Ellipsis). Suppressed for
/// `.pyi` files and `@overload`/`@abstractmethod`/`Protocol`/`ABC` contexts.
fn match_python_not_implemented(node: Node, source: &[u8], path: &Path) -> bool {
    if node.kind() == "raise_statement" {
        if python_suppressed(node, source, path) {
            return false;
        }
        let mut cursor = node.walk();
        if let Some(first) = node.children(&mut cursor).find(|c| c.is_named()) {
            return match first.kind() {
                "identifier" => first.utf8_text(source).ok() == Some("NotImplementedError"),
                "call" => {
                    first
                        .child_by_field_name("function")
                        .and_then(|f| f.utf8_text(source).ok())
                        == Some("NotImplementedError")
                }
                _ => false,
            };
        }
        return false;
    }

    if node.kind() == "function_definition" {
        if python_suppressed(node, source, path) {
            return false;
        }
        if let Some(body) = node.child_by_field_name("body") {
            let stmts = stmt_children(body, Language::Python);
            if let [stmt] = stmts.as_slice() {
                if stmt.kind() == "expression_statement" {
                    if let Some(inner) = stmt.named_child(0) {
                        return inner.kind() == "ellipsis";
                    }
                }
            }
        }
    }
    false
}

/// Go: `panic(<msg>)` where `<msg>` is a string literal matching the
/// not-implemented wording.
fn match_go_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(func) = node.child_by_field_name("function") else {
        return false;
    };
    if func.kind() != "identifier" || func.utf8_text(source).ok() != Some("panic") {
        return false;
    }
    node.child_by_field_name("arguments")
        .and_then(|a| first_string_literal_text(a, source, Language::Go))
        .is_some_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(&text))
}

/// JS/TS/TSX/JSX: `throw new Error(<msg>)` where `<msg>` is a string literal
/// matching the not-implemented wording.
fn match_js_not_implemented(node: Node, source: &[u8], lang: Language) -> bool {
    if node.kind() != "throw_statement" {
        return false;
    }
    let Some(inner) = node.named_child(0) else {
        return false;
    };
    if inner.kind() != "new_expression" {
        return false;
    }
    let Some(ctor) = inner.child_by_field_name("constructor") else {
        return false;
    };
    if ctor.utf8_text(source).ok() != Some("Error") {
        return false;
    }
    inner
        .child_by_field_name("arguments")
        .and_then(|a| first_string_literal_text(a, source, lang))
        .is_some_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(&text))
}

/// Java: `throw new NotImplementedException(...)` (any args -- no legitimate
/// use), or `throw new UnsupportedOperationException(<msg>)` where `<msg>`
/// matches the not-implemented wording. A message-less
/// `UnsupportedOperationException` is the standard "optional operation" idiom
/// (e.g. an immutable collection's `remove()`) and is not flagged.
fn match_java_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "throw_statement" {
        return false;
    }
    let Some(inner) = node.named_child(0) else {
        return false;
    };
    if inner.kind() != "object_creation_expression" {
        return false;
    }
    let Some(ty) = inner.child_by_field_name("type") else {
        return false;
    };
    let Ok(text) = ty.utf8_text(source) else {
        return false;
    };
    let name = text.rsplit('.').next().unwrap_or(text);
    match name {
        "NotImplementedException" => true,
        "UnsupportedOperationException" => inner
            .child_by_field_name("arguments")
            .and_then(|a| first_string_literal_text(a, source, Language::Java))
            .is_some_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(&text)),
        _ => false,
    }
}

/// C#: `throw new NotImplementedException(...)`.
fn match_csharp_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "throw_statement" && node.kind() != "throw_expression" {
        return false;
    }
    let Some(inner) = node.named_child(0) else {
        return false;
    };
    if inner.kind() != "object_creation_expression" {
        return false;
    }
    let Some(ty) = inner.child_by_field_name("type") else {
        return false;
    };
    let Ok(text) = ty.utf8_text(source) else {
        return false;
    };
    let name = text.rsplit('.').next().unwrap_or(text);
    name == "NotImplementedException"
}

/// Ruby: `raise NotImplementedError` (bare, with message, or `.new`).
fn match_ruby_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let Some(method) = node.child_by_field_name("method") else {
        return false;
    };
    if method.utf8_text(source).ok() != Some("raise") {
        return false;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    let Some(first) = args.named_children(&mut cursor).next() else {
        return false;
    };
    match first.kind() {
        "constant" => first.utf8_text(source).ok() == Some("NotImplementedError"),
        "call" => first.child_by_field_name("receiver").is_some_and(|r| {
            r.kind() == "constant" && r.utf8_text(source).ok() == Some("NotImplementedError")
        }),
        _ => false,
    }
}

/// PHP: `throw new <...Exception>(<msg>)` where `<msg>` is a string literal
/// matching the not-implemented wording.
fn match_php_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "throw_expression" {
        return false;
    }
    let Some(inner) = node.named_child(0) else {
        return false;
    };
    if inner.kind() != "object_creation_expression" {
        return false;
    }

    let mut class_name = None;
    let mut args_node = None;
    let mut cursor = inner.walk();
    for child in inner.named_children(&mut cursor) {
        match child.kind() {
            "name" | "qualified_name" => class_name = Some(child),
            "arguments" => args_node = Some(child),
            _ => {}
        }
    }
    let Some(class_name) = class_name else {
        return false;
    };
    let Ok(name_text) = class_name.utf8_text(source) else {
        return false;
    };
    let short_name = name_text.rsplit('\\').next().unwrap_or(name_text);
    if !short_name.ends_with("Exception") {
        return false;
    }
    args_node
        .and_then(|a| first_string_literal_text(a, source, Language::Php))
        .is_some_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(&text))
}

/// C/C++: `assert(0 && "...")` / `assert(false && "...")` where the string
/// literal matches the not-implemented wording.
///
/// The `abort()`-adjacent-comment variant mentioned as optional in the spec is
/// intentionally not implemented -- see the final report for rationale.
fn match_c_not_implemented(node: Node, source: &[u8]) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(func) = node.child_by_field_name("function") else {
        return false;
    };
    if func.kind() != "identifier" || func.utf8_text(source).ok() != Some("assert") {
        return false;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return false;
    };
    let Some(first_arg) = args.named_child(0) else {
        return false;
    };
    if first_arg.kind() != "binary_expression" {
        return false;
    }
    let Some(left) = first_arg.child_by_field_name("left") else {
        return false;
    };
    let Some(right) = first_arg.child_by_field_name("right") else {
        return false;
    };
    let left_text = left.utf8_text(source).unwrap_or("").trim();
    if left_text != "0" && left_text != "false" {
        return false;
    }
    if right.kind() != "string_literal" {
        return false;
    }
    right
        .utf8_text(source)
        .is_ok_and(|text| NOT_IMPLEMENTED_MSG_RE.is_match(text))
}

// ---------------------------------------------------------------------------
// empty_body matchers
// ---------------------------------------------------------------------------

/// Empty-but-implementable body detection.
///
/// Only flags when the body has zero statements AND either the declared
/// return type is non-void/non-unit, or the (otherwise empty) body contains
/// an elision comment. Languages without a static return-type signal
/// (JavaScript, Jsx, Ruby, Bash) only support the comment-branch; Python is
/// gated on `pass`-only bodies with an explicit return type annotation,
/// since Python lacks static typing generally. Python detection additionally
/// suppresses `.pyi` files and `@overload`/`@abstractmethod`/`Protocol`/`ABC`
/// contexts.
fn match_empty_body(node: Node, source: &[u8], lang: Language, path: &Path) -> bool {
    match lang {
        Language::Rust => empty_body_rust(node, source),
        Language::Go => empty_body_go(node, source),
        Language::Python => empty_body_python(node, source, path),
        Language::TypeScript | Language::Tsx => empty_body_ts(node, source, lang),
        Language::JavaScript | Language::Jsx => empty_body_comment_only(
            node,
            source,
            lang,
            &["function_declaration", "method_definition"],
        ),
        Language::Java => empty_body_java(node, source),
        Language::CSharp => empty_body_csharp(node, source),
        Language::C | Language::Cpp => empty_body_c(node, source, lang),
        Language::Php => empty_body_php(node, source),
        Language::Ruby => empty_body_ruby(node, source),
        Language::Bash => empty_body_comment_only(node, source, lang, &["function_definition"]),
    }
}

fn empty_body_rust(node: Node, source: &[u8]) -> bool {
    if node.kind() != "function_item" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if !stmt_children(body, Language::Rust).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, Language::Rust) {
        return true;
    }
    node.child_by_field_name("return_type")
        .and_then(|rt| rt.utf8_text(source).ok())
        .is_some_and(|text| text.trim() != "()")
}

fn empty_body_go(node: Node, source: &[u8]) -> bool {
    if node.kind() != "function_declaration" && node.kind() != "method_declaration" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if !stmt_children(body, Language::Go).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, Language::Go) {
        return true;
    }
    node.child_by_field_name("result").is_some()
}

fn empty_body_python(node: Node, source: &[u8], path: &Path) -> bool {
    if node.kind() != "function_definition" {
        return false;
    }
    if python_suppressed(node, source, path) {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let stmts = stmt_children(body, Language::Python);
    let [stmt] = stmts.as_slice() else {
        return false;
    };
    if stmt.kind() != "pass_statement" {
        return false;
    }
    node.child_by_field_name("return_type")
        .and_then(|rt| rt.utf8_text(source).ok())
        .is_some_and(|text| !matches!(text.trim(), "None" | "NoReturn" | ""))
}

fn empty_body_ts(node: Node, source: &[u8], lang: Language) -> bool {
    if node.kind() != "function_declaration" && node.kind() != "method_definition" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if body.kind() != "statement_block" || !stmt_children(body, lang).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, lang) {
        return true;
    }
    let Some(rt) = node.child_by_field_name("return_type") else {
        return false;
    };
    let Ok(text) = rt.utf8_text(source) else {
        return false;
    };
    let type_text = text.trim_start_matches(':').trim();
    !matches!(type_text, "void" | "any" | "unknown" | "never")
}

/// Shared comment-only empty-body rule for languages without a static
/// return-type signal: flag only when the body is empty except for an
/// elision comment.
fn empty_body_comment_only(node: Node, source: &[u8], lang: Language, node_kinds: &[&str]) -> bool {
    if !node_kinds.contains(&node.kind()) {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if !stmt_children(body, lang).is_empty() {
        return false;
    }
    body_has_elision_comment(body, source, lang)
}

fn empty_body_java(node: Node, source: &[u8]) -> bool {
    if node.kind() != "method_declaration" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if !stmt_children(body, Language::Java).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, Language::Java) {
        return true;
    }
    node.child_by_field_name("type")
        .is_some_and(|t| t.kind() != "void_type")
}

fn empty_body_csharp(node: Node, source: &[u8]) -> bool {
    if node.kind() != "method_declaration" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if body.kind() != "block" || !stmt_children(body, Language::CSharp).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, Language::CSharp) {
        return true;
    }
    node.child_by_field_name("returns")
        .and_then(|rt| rt.utf8_text(source).ok())
        .is_some_and(|text| text.trim() != "void")
}

fn empty_body_c(node: Node, source: &[u8], lang: Language) -> bool {
    if node.kind() != "function_definition" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if body.kind() != "compound_statement" || !stmt_children(body, lang).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, lang) {
        return true;
    }
    // Constructors/destructors in C++ have no "type" field; without a return
    // type signal we only flag via the comment branch above.
    node.child_by_field_name("type")
        .and_then(|t| t.utf8_text(source).ok())
        .is_some_and(|text| text.trim() != "void")
}

fn empty_body_php(node: Node, source: &[u8]) -> bool {
    if node.kind() != "function_definition" && node.kind() != "method_declaration" {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    if !stmt_children(body, Language::Php).is_empty() {
        return false;
    }
    if body_has_elision_comment(body, source, Language::Php) {
        return true;
    }
    node.child_by_field_name("return_type")
        .and_then(|rt| rt.utf8_text(source).ok())
        .is_some_and(|text| !matches!(text.trim(), "void" | "mixed"))
}

/// Ruby has no `body` field at all when a method has zero statements (see
/// `enclosing_function_body` for the shared rationale on why only the
/// comment-branch is supported here).
fn empty_body_ruby(node: Node, source: &[u8]) -> bool {
    if node.kind() != "method" {
        return false;
    }
    if node.child_by_field_name("body").is_some() {
        return false;
    }
    let mut cursor = node.walk();
    let matched = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "comment")
        .any(|c| {
            c.utf8_text(source).is_ok_and(|t| {
                !is_doc_comment_text(t, Language::Ruby)
                    && (ELISION_STRONG_RE.is_match(t) || ELISION_WEAK_RE.is_match(t))
            })
        });
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language as CoreLang;

    fn stubs_for(path: &str, lang: CoreLang, src: &str) -> Vec<Stub> {
        let file = SourceFile::from_content(path, lang, src.as_bytes().to_vec());
        Analyzer::new().analyze_file(&file)
    }

    fn categories(stubs: &[Stub]) -> Vec<Category> {
        stubs.iter().map(|s| s.category).collect()
    }

    // -----------------------------------------------------------------
    // not_implemented: true positives
    // -----------------------------------------------------------------

    #[test]
    fn test_rust_todo_macro_detected() {
        let stubs = stubs_for("a.rs", CoreLang::Rust, "fn f() -> i32 {\n    todo!()\n}\n");
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
        assert_eq!(stubs[0].severity, Severity::High);
    }

    #[test]
    fn test_rust_unimplemented_macro_detected() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    unimplemented!()\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_rust_panic_with_not_implemented_message_detected() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    panic!(\"not implemented yet\")\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_rust_panic_without_matching_message_is_not_a_stub() {
        // A real panic that isn't a stub idiom.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    panic!(\"division by zero\")\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_unreachable_used_correctly_is_not_a_stub() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f(x: u8) {\n    match x {\n        0 => {}\n        _ => unreachable!(\"invariant violated\"),\n    }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_panic_matching_identifier_named_stubs_is_not_a_stub() {
        // Regression: "Stubs" (identifier text) must not match the bounded
        // not-implemented message regex, and identifiers are never inspected
        // at all -- only string-literal argument nodes are.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    panic!(\"Expected Stubs command\")\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_panic_with_format_args_ignores_identifier_arguments() {
        // The literal fragment "{}" must be checked, not the identifier
        // `stub_count` passed as a format argument.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f(stub_count: u32) {\n    panic!(\"{}\", stub_count)\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_panic_bare_stub_message_detected() {
        // A bare "stub" message is a real not-implemented idiom and must not
        // be dropped just because "todo"/"stub" as raw substrings would be
        // too broad elsewhere (e.g. matching inside "Stubs").
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    panic!(\"stub\")\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_rust_panic_matching_identifier_named_stubs_is_still_not_a_stub() {
        // Re-confirm after re-adding a bounded `stub` alternative: "Stubs"
        // still doesn't match, since \bstub\b requires a trailing boundary
        // that "Stubs" (continuing into "s") doesn't have.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    panic!(\"Expected Stubs command\")\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_raise_not_implemented_error_detected() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    raise NotImplementedError\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_python_raise_not_implemented_error_call_detected() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    raise NotImplementedError(\"nope\")\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_python_ellipsis_body_detected() {
        let stubs = stubs_for("a.py", CoreLang::Python, "def f():\n    ...\n");
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_python_other_raise_is_not_a_stub() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    raise ValueError(\"bad input\")\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_string_containing_not_implemented_is_not_flagged() {
        // Regression: string literals must never trigger detection.
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    return \"not implemented\"\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_pyi_ellipsis_declaration_not_flagged() {
        let stubs = stubs_for("a.pyi", CoreLang::Python, "def parse(v: str) -> int: ...\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_pyi_raise_not_implemented_error_not_flagged() {
        let stubs = stubs_for(
            "a.pyi",
            CoreLang::Python,
            "def parse(v: str) -> int:\n    raise NotImplementedError\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_overload_ellipsis_declaration_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "from typing import overload\n\n@overload\ndef parse(v: str) -> int: ...\n\n\ndef parse(v):\n    return int(v)\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_abstractmethod_raise_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "from abc import ABC, abstractmethod\n\nclass Base(ABC):\n    @abstractmethod\n    def run(self):\n        raise NotImplementedError\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_dotted_typing_overload_exempt() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "import typing\n\n@typing.overload\ndef parse(v: str) -> int: ...\n\n\ndef parse(v):\n    return int(v)\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_dotted_abc_abstractmethod_exempt() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "import abc\n\nclass Base(abc.ABC):\n    @abc.abstractmethod\n    def run(self):\n        raise NotImplementedError\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_decorator_substring_match_is_not_exempt() {
        // `@not_an_overload` merely CONTAINS "overload" as a substring; the
        // decorator's own (qualified) name must equal "overload", not just
        // contain it, so this must still be flagged as a real stub.
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "@not_an_overload\ndef f():\n    raise NotImplementedError\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_python_superclass_substring_match_is_not_exempt() {
        // `ABCWidget` merely CONTAINS "ABC" as a substring; the base class
        // name must equal "ABC" (or "abc.ABC"), not just contain it.
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "class Worker(ABCWidget):\n    def run(self):\n        raise NotImplementedError\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_python_abstractmethod_pass_body_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "from abc import ABC, abstractmethod\n\nclass Base(ABC):\n    @abstractmethod\n    def run(self) -> int:\n        pass\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_protocol_method_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "from typing import Protocol\n\nclass Sized(Protocol):\n    def size(self) -> int:\n        ...\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_dotted_typing_protocol_exempt() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "import typing\n\nclass Sized(typing.Protocol):\n    def size(self) -> int:\n        ...\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_non_abstract_ellipsis_body_still_flagged() {
        // Suppression must not become blanket: an ordinary (non-decorated,
        // non-Protocol) function with an ellipsis body is still a stub.
        let stubs = stubs_for("a.py", CoreLang::Python, "def f():\n    ...\n");
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_go_panic_not_implemented_detected() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\nfunc f() {\n\tpanic(\"not implemented\")\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_go_panic_real_error_is_not_a_stub() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\nfunc f() {\n\tpanic(\"index out of range\")\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_go_panic_bare_stub_message_detected() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\nfunc f() {\n\tpanic(\"stub\")\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_go_panic_with_formatted_dynamic_message_not_flagged() {
        // fmt.Sprintf(...) is a call, not a string literal -- must not be
        // inspected even if some nested literal inside it would match.
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\nimport \"fmt\"\nfunc f(x int) {\n\tpanic(fmt.Sprintf(\"unexpected value: %d\", x))\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_typescript_throw_not_implemented_detected() {
        let stubs = stubs_for(
            "a.ts",
            CoreLang::TypeScript,
            "function f() {\n  throw new Error('not implemented');\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_javascript_throw_todo_stub_detected() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f() {\n  throw new Error('TODO: implement this');\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_tsx_throw_unimplemented_detected() {
        let stubs = stubs_for(
            "a.tsx",
            CoreLang::Tsx,
            "function f() {\n  throw new Error('unimplemented');\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_jsx_throw_unimplemented_detected() {
        let stubs = stubs_for(
            "a.jsx",
            CoreLang::Jsx,
            "function f() {\n  throw new Error('unimplemented');\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_typescript_throw_bare_stub_message_detected() {
        let stubs = stubs_for(
            "a.ts",
            CoreLang::TypeScript,
            "function f() {\n  throw new Error('stub');\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_js_throw_real_error_is_not_a_stub() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f() {\n  throw new Error('invalid input');\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_js_throw_other_type_is_not_a_stub() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f() {\n  throw new TypeError('not implemented');\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_js_throw_with_identifier_message_not_flagged() {
        // `todoMessage` is an identifier, not a string literal -- even
        // though its name contains "todo", it must never be inspected.
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f(todoMessage) {\n  throw new Error(todoMessage);\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_java_unsupported_operation_exception_detected() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class A {\n  void f() {\n    throw new UnsupportedOperationException(\"nope, not implemented\");\n  }\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_java_not_implemented_exception_detected() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class A {\n  void f() {\n    throw new NotImplementedException();\n  }\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_java_unsupported_operation_exception_without_message_not_flagged() {
        // Standard "optional operation" idiom, e.g. an immutable collection's
        // overridden `remove()`. Not necessarily unfinished work.
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class ImmutableList {\n  void remove(int index) {\n    throw new UnsupportedOperationException();\n  }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_java_unsupported_operation_exception_with_unrelated_message_not_flagged() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class ImmutableList {\n  void remove(int index) {\n    throw new UnsupportedOperationException(\"list is immutable\");\n  }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_java_other_exception_is_not_a_stub() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class A {\n  void f() {\n    throw new IllegalArgumentException(\"bad\");\n  }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_csharp_not_implemented_exception_detected() {
        let stubs = stubs_for(
            "A.cs",
            CoreLang::CSharp,
            "class A {\n  void F() {\n    throw new NotImplementedException();\n  }\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_csharp_other_exception_is_not_a_stub() {
        let stubs = stubs_for(
            "A.cs",
            CoreLang::CSharp,
            "class A {\n  void F() {\n    throw new InvalidOperationException();\n  }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_ruby_raise_not_implemented_error_bare_detected() {
        let stubs = stubs_for(
            "a.rb",
            CoreLang::Ruby,
            "def f\n  raise NotImplementedError\nend\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_ruby_raise_not_implemented_error_with_message_detected() {
        let stubs = stubs_for(
            "a.rb",
            CoreLang::Ruby,
            "def f\n  raise NotImplementedError, \"nope\"\nend\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_ruby_raise_not_implemented_error_new_detected() {
        let stubs = stubs_for(
            "a.rb",
            CoreLang::Ruby,
            "def f\n  raise NotImplementedError.new\nend\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_ruby_raise_other_error_is_not_a_stub() {
        let stubs = stubs_for(
            "a.rb",
            CoreLang::Ruby,
            "def f\n  raise ArgumentError, \"bad\"\nend\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_php_throw_exception_not_implemented_detected() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nfunction f() {\n  throw new BadMethodCallException(\"not implemented\");\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_php_throw_real_exception_is_not_a_stub() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nfunction f() {\n  throw new InvalidArgumentException(\"bad value\");\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_php_throw_non_exception_class_is_not_a_stub() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nfunction f() {\n  throw new Foo(\"not implemented\");\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_c_assert_zero_not_implemented_detected() {
        let stubs = stubs_for(
            "a.c",
            CoreLang::C,
            "void f() {\n  assert(0 && \"not implemented\");\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_cpp_assert_false_not_implemented_detected() {
        let stubs = stubs_for(
            "a.cpp",
            CoreLang::Cpp,
            "void f() {\n  assert(false && \"unimplemented\");\n}\n",
        );
        assert_eq!(categories(&stubs), vec![Category::NotImplemented]);
    }

    #[test]
    fn test_c_assert_real_invariant_is_not_a_stub() {
        let stubs = stubs_for(
            "a.c",
            CoreLang::C,
            "void f(int x) {\n  assert(x > 0 && \"x must be positive\");\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_bash_has_no_not_implemented_idiom() {
        // Bash relies solely on the elision-comment category; a bare panic-like
        // string in a bash command must not be treated as not_implemented.
        let stubs = stubs_for(
            "a.sh",
            CoreLang::Bash,
            "foo() {\n  echo \"not implemented\"\n}\n",
        );
        assert!(
            !stubs.iter().any(|s| s.category == Category::NotImplemented),
            "found: {stubs:?}"
        );
    }

    // -----------------------------------------------------------------
    // elision: comments, all languages
    // -----------------------------------------------------------------

    #[test]
    fn test_rust_elision_comment_detected() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    // ... rest of the implementation\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.category == Category::Elision || s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_rust_bare_todo_comment_is_satd_not_stub() {
        // A bare TODO (no "implement" wording) is SATD's job, not stubs'.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    // TODO: rename var\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_todo_implement_comment_detected() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() {\n    // TODO: implement retry logic\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_rust_string_literal_with_elision_wording_not_flagged() {
        // Elision must come from a comment node, never a string literal.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn f() -> &'static str {\n    \"for brevity, this is incomplete\"\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_module_doc_comment_not_flagged() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "//! This module intentionally leaves the rest of the implementation for a follow-up crate.\n\nfn f() -> i32 { 1 }\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_item_doc_comment_not_flagged() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "/// Placeholder documentation describing a fully-implemented helper.\nfn helper() -> i32 { 1 }\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_ordinary_placeholder_comment_in_real_function_not_flagged() {
        // "placeholder" is ambiguous; without independent evidence of
        // unfinished work in the same function, it must not be flagged.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn build() -> std::path::PathBuf {\n    // Use a placeholder root since files are relative paths from tree\n    std::path::PathBuf::from(\".\")\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_rust_placeholder_comment_in_empty_function_is_flagged() {
        // Same ambiguous wording, but paired with an empty body -- this is
        // independent evidence of unfinished work.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn build() -> i32 {\n    // placeholder\n}\n",
        );
        assert_eq!(stubs.len(), 1, "found: {stubs:?}");
        assert_eq!(stubs[0].category, Category::EmptyBody);
        assert!(stubs[0].categories.contains(&Category::Elision));
    }

    #[test]
    fn test_go_elision_comment_detected() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\nfunc f() {\n\t// fill this in\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_python_elision_comment_detected() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    # for brevity, this skips validation\n    return None\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_python_explanatory_comment_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "def f():\n    # returns the cached value if present\n    return None\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_typescript_elision_comment_detected() {
        let stubs = stubs_for(
            "a.ts",
            CoreLang::TypeScript,
            "function f() {\n  // your code here\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_javascript_elision_comment_detected() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f() {\n  // implementation omitted\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_javascript_doc_comment_not_flagged() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "/**\n * Placeholder-free, fully implemented helper.\n */\nfunction helper() {\n  return 1;\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_tsx_elision_comment_detected() {
        let stubs = stubs_for(
            "a.tsx",
            CoreLang::Tsx,
            "function F() {\n  // keep existing behavior below\n  return null;\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_jsx_elision_comment_detected() {
        let stubs = stubs_for(
            "a.jsx",
            CoreLang::Jsx,
            "function F() {\n  // for brevity\n  return null;\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_java_elision_comment_paired_with_empty_body_detected() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "class A {\n  int f() {\n    // stub out\n  }\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_java_javadoc_not_flagged() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "/**\n * Placeholder-free javadoc for a complete method.\n */\nclass A {\n  int f() { return 1; }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_csharp_elision_comment_detected() {
        let stubs = stubs_for(
            "A.cs",
            CoreLang::CSharp,
            "class A {\n  void F() {\n    // implementation omitted\n  }\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_csharp_xmldoc_not_flagged() {
        let stubs = stubs_for(
            "A.cs",
            CoreLang::CSharp,
            "/// <summary>A placeholder-free, complete summary.</summary>\nclass A {\n  void F() { }\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_c_elision_comment_detected() {
        let stubs = stubs_for(
            "a.c",
            CoreLang::C,
            "void f() {\n  // rest of the code omitted for brevity\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_c_ordinary_placeholder_comment_in_real_function_not_flagged() {
        let stubs = stubs_for(
            "a.c",
            CoreLang::C,
            "const char *describe(int x) {\n  // for returns, use a generic non-nil placeholder\n  return \"n/a\";\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_cpp_elision_comment_detected() {
        let stubs = stubs_for("a.cpp", CoreLang::Cpp, "void f() {\n  // placeholder\n}\n");
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_ruby_elision_comment_detected() {
        let stubs = stubs_for("a.rb", CoreLang::Ruby, "def f\n  # placeholder\nend\n");
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_php_elision_comment_detected() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nfunction f() {\n  // fill this in\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_bash_elision_comment_detected() {
        let stubs = stubs_for(
            "a.sh",
            CoreLang::Bash,
            "foo() {\n  # rest of the implementation\n  echo hi\n}\n",
        );
        assert!(stubs
            .iter()
            .any(|s| s.categories.contains(&Category::Elision)));
    }

    #[test]
    fn test_bash_normal_comment_not_flagged() {
        let stubs = stubs_for(
            "a.sh",
            CoreLang::Bash,
            "foo() {\n  # prints a greeting\n  echo hi\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    // -----------------------------------------------------------------
    // empty_body: true positives
    // -----------------------------------------------------------------

    #[test]
    fn test_rust_empty_body_non_unit_return_detected() {
        let stubs = stubs_for("a.rs", CoreLang::Rust, "fn f() -> i32 {}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_rust_trait_default_method_unit_return_not_flagged() {
        // Explicit false-positive case from the spec: a trait default `{}`
        // returning `()` must not flag.
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "trait T {\n    fn foo(&self) {}\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_go_empty_body_with_result_detected() {
        let stubs = stubs_for("a.go", CoreLang::Go, "package p\nfunc f() error {}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_go_method_returning_nothing_not_flagged() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\ntype T struct{}\nfunc (t T) F() {}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_python_pass_with_return_type_detected() {
        let stubs = stubs_for("a.py", CoreLang::Python, "def f() -> int:\n    pass\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_python_empty_init_with_pass_not_flagged() {
        let stubs = stubs_for(
            "a.py",
            CoreLang::Python,
            "class A:\n    def __init__(self):\n        pass\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_typescript_empty_body_non_void_return_detected() {
        let stubs = stubs_for("a.ts", CoreLang::TypeScript, "function f(): number {}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_typescript_interface_method_signature_not_flagged() {
        // No body at all (a signature), so this must never reach the
        // empty-body path in the first place.
        let stubs = stubs_for(
            "a.ts",
            CoreLang::TypeScript,
            "interface Foo {\n  bar(): number;\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_typescript_void_return_empty_body_not_flagged() {
        let stubs = stubs_for("a.ts", CoreLang::TypeScript, "function f(): void {}\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_javascript_empty_constructor_not_flagged() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "class Foo {\n  constructor() {}\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_javascript_empty_body_with_elision_comment_detected() {
        let stubs = stubs_for(
            "a.js",
            CoreLang::JavaScript,
            "function f() {\n  // TODO: implement\n}\n",
        );
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_java_empty_body_non_void_detected() {
        let stubs = stubs_for("A.java", CoreLang::Java, "class A {\n  int f() {}\n}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_java_abstract_method_not_flagged() {
        let stubs = stubs_for(
            "A.java",
            CoreLang::Java,
            "abstract class A {\n  abstract int f();\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_java_void_empty_body_not_flagged() {
        let stubs = stubs_for("A.java", CoreLang::Java, "class A {\n  void f() {}\n}\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_csharp_empty_body_non_void_detected() {
        let stubs = stubs_for("A.cs", CoreLang::CSharp, "class A {\n  int F() {}\n}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_csharp_interface_method_not_flagged() {
        let stubs = stubs_for("A.cs", CoreLang::CSharp, "interface A {\n  int F();\n}\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_c_empty_body_non_void_detected() {
        let stubs = stubs_for("a.c", CoreLang::C, "int f() {}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_c_empty_void_body_not_flagged() {
        let stubs = stubs_for("a.c", CoreLang::C, "void f() {}\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_cpp_empty_constructor_not_flagged() {
        // Constructors have no return type field at all; without a comment
        // signal this must not flag.
        let stubs = stubs_for(
            "a.cpp",
            CoreLang::Cpp,
            "class Foo {\npublic:\n  Foo() {}\n};\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_cpp_pure_virtual_not_flagged() {
        let stubs = stubs_for(
            "a.cpp",
            CoreLang::Cpp,
            "class Foo {\npublic:\n  virtual int bar() = 0;\n};\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_php_empty_body_non_void_detected() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nclass A {\n  public function foo(): int {}\n}\n",
        );
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_php_abstract_method_not_flagged() {
        let stubs = stubs_for(
            "a.php",
            CoreLang::Php,
            "<?php\nabstract class A {\n  abstract public function foo(): int;\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_ruby_empty_method_with_elision_comment_detected() {
        let stubs = stubs_for("a.rb", CoreLang::Ruby, "def foo\n  # placeholder\nend\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_ruby_plain_empty_method_not_flagged() {
        // Common, legitimate no-op/hook idiom in Ruby.
        let stubs = stubs_for("a.rb", CoreLang::Ruby, "def foo\nend\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_bash_empty_function_with_elision_comment_detected() {
        let stubs = stubs_for("a.sh", CoreLang::Bash, "foo() {\n  # your code here\n}\n");
        assert!(stubs.iter().any(|s| s.category == Category::EmptyBody));
    }

    #[test]
    fn test_bash_noop_function_not_flagged() {
        let stubs = stubs_for("a.sh", CoreLang::Bash, "foo() {\n  :\n}\n");
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    // -----------------------------------------------------------------
    // Merging: one finding per site
    // -----------------------------------------------------------------

    #[test]
    fn test_empty_body_and_elision_at_same_function_merge_into_one_stub() {
        let stubs = stubs_for(
            "a.go",
            CoreLang::Go,
            "package p\n\nfunc f() {\n\t// ... rest of the implementation\n}\n",
        );
        assert_eq!(stubs.len(), 1, "found: {stubs:?}");
        assert_eq!(stubs[0].category, Category::EmptyBody);
        assert_eq!(
            stubs[0].categories,
            vec![Category::Elision, Category::EmptyBody]
        );
        assert!(stubs[0].lines.len() >= 2, "found: {:?}", stubs[0].lines);
    }

    #[test]
    fn test_two_unrelated_elision_comments_in_non_empty_function_stay_separate() {
        let stubs = stubs_for(
            "a.ts",
            CoreLang::TypeScript,
            "function f() {\n  // for brevity, step one is skipped\n  doWork();\n  // for brevity, step two is skipped\n  doMoreWork();\n}\n",
        );
        assert_eq!(stubs.len(), 2, "found: {stubs:?}");
        assert!(stubs.iter().all(|s| s.category == Category::Elision));
    }

    // -----------------------------------------------------------------
    // General
    // -----------------------------------------------------------------

    #[test]
    fn test_clean_file_produces_no_stubs() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        );
        assert!(stubs.is_empty(), "found: {stubs:?}");
    }

    #[test]
    fn test_stubs_sorted_by_line() {
        let stubs = stubs_for(
            "a.rs",
            CoreLang::Rust,
            "fn a() {\n    todo!()\n}\nfn b() {\n    unimplemented!()\n}\n",
        );
        assert_eq!(stubs.len(), 2);
        assert!(stubs[0].line < stubs[1].line);
    }

    #[test]
    fn test_severity_ordering_for_gate_threshold() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
    }

    #[test]
    fn test_severity_from_str() {
        assert_eq!("low".parse::<Severity>(), Ok(Severity::Low));
        assert_eq!("Medium".parse::<Severity>(), Ok(Severity::Medium));
        assert_eq!("HIGH".parse::<Severity>(), Ok(Severity::High));
        assert!("bogus".parse::<Severity>().is_err());
    }

    #[test]
    fn test_analyzer_uses_content_source_for_historical_commits() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet, FilesystemSource};
        use std::path::PathBuf;
        use std::sync::Arc;

        let current = tempfile::tempdir().unwrap();
        let historical = tempfile::tempdir().unwrap();
        std::fs::write(current.path().join("s.rs"), "fn f() {}\n").unwrap();
        std::fs::write(historical.path().join("s.rs"), "fn f() {\n    todo!()\n}\n").unwrap();
        let files = FileSet::from_files(current.path().to_path_buf(), vec![PathBuf::from("s.rs")]);
        let config = Config::default();
        let source = Arc::new(FilesystemSource::new(historical.path()));
        let ctx =
            AnalysisContext::new(&files, &config, Some(current.path())).with_content_source(source);

        let result = Analyzer::new().analyze(&ctx).unwrap();

        assert_eq!(result.stubs.len(), 1);
        assert_eq!(result.stubs[0].file, "s.rs");
        assert_eq!(result.summary.total_stubs, 1);
        assert_eq!(result.by_category.get("not_implemented"), Some(&1));
    }

    #[test]
    fn test_analyze_aggregates_across_files() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, Analyzer as _, FileSet};

        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "fn f() {\n    todo!()\n}\n").unwrap();
        std::fs::write(
            temp.path().join("b.py"),
            "def f():\n    raise NotImplementedError\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("c.rs"), "fn ok() -> i32 { 1 }\n").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        let result = Analyzer::new().analyze(&ctx).unwrap();
        assert_eq!(result.summary.total_stubs, 2);
        assert_eq!(result.summary.high_severity, 2);
        assert_eq!(result.by_category.get("not_implemented"), Some(&2));
    }
}
