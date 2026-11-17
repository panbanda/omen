//! Rails routes: the entry points that `routes.rb` registers.
//!
//! The table "Rails routes" in `docs/SECURITY.md` lists each form that omen reads and each form
//! that it skips. Omen checked each form against Rails 7.2.2: `mapper.rb`, `journey/router/utils.rb`,
//! `abstract_controller`, and the inflector. A form that omen did not verify gives no route, no
//! symbol, or no guard. Omen does not guess.
//!
//! - Routes. Omen reads `get`, `post`, `put`, `patch`, `delete`, `match`, `root`, `resources`, and
//!   `resource`, in `namespace`, `scope`, `controller`, `member`, `collection`, `constraints`, and
//!   `defaults` blocks. A route needs a literal path, a `draw` call on the routes of the
//!   application or of an engine that the repository defines, and no unknown option. A route in a
//!   dead branch, a method, a lambda, a loop, a class body, or an unknown block is skipped.
//! - Paths. Omen gives the path as the normalization of Journey and of the mapper gives it.
//!   A nested route needs the singular name of the parent resource, and a `resource` call needs
//!   the plural of its name. Omen uses the inflection rules of Rails 7.2.2. If an initializer
//!   changes the inflections, omen skips the nested routes, and it gives no controller for a
//!   `resource` call. A registered acronym changes the class names. A call that omen cannot read
//!   gives no class name.
//! - Symbols. A route matches an instance method that is public. If the method is not public, or
//!   its visibility is not readable, the route has no symbol and no guard.
//! - Guards. The guards are the `before_action` callbacks of the action in the order of Rails. A
//!   callback replaces an earlier callback with the same name. A callback with a condition, a
//!   list that is not a literal, or a place in a block adds no guard. A `skip_before_action` call
//!   with such a place removes the guard.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use parking_lot::Mutex;
use tree_sitter::Node;

use super::persistent::PersistentMap;
use super::{route, Context, Draft, EntryBasis, EntryKind};
use crate::core::Language;
use crate::index::extract::FileSymbols;
use crate::index::model::{Span, Symbol};

/// The routes of a `resources` call, in the order of Rails.
const COLLECTION_ACTIONS: [(&str, &str, &str); 8] = [
    ("index", "GET", ""),
    ("create", "POST", ""),
    ("new", "GET", "/new"),
    ("edit", "GET", "/:id/edit"),
    ("show", "GET", "/:id"),
    ("update", "PATCH", "/:id"),
    ("update", "PUT", "/:id"),
    ("destroy", "DELETE", "/:id"),
];

/// The routes of a `resource` call.
const SINGULAR_ACTIONS: [(&str, &str, &str); 7] = [
    ("new", "GET", "/new"),
    ("create", "POST", ""),
    ("edit", "GET", "/edit"),
    ("show", "GET", ""),
    ("update", "PATCH", ""),
    ("update", "PUT", ""),
    ("destroy", "DELETE", ""),
];

/// Options of `resources` that do not change the paths or the controller.
const HARMLESS_OPTIONS: [&str; 3] = ["constraints", "defaults", "concerns"];

/// The scope that the blocks around a route give.
#[derive(Clone, Default)]
pub(super) struct Scope {
    /// The path of the blocks, as `@scope[:path]` of Rails. `None` means that the path is not
    /// known.
    prefix: Option<Prefix>,
    modules: Modules,
    controller: Option<ScopedController>,
    /// The resource, when the route is directly in a `resources` or `resource` block.
    resource: Option<ResourceContext>,
    /// The route is in a `member` or `collection` block, or `on:` puts it there. A canonical action
    /// name then adds no path segment.
    method_scope: bool,
    /// The route is in a `draw` block of the routes of the application or of an engine.
    draw: bool,
    /// The value of `config.api_only` for the route set of the `draw` call.
    api_only: ApiOnly,
}

impl Scope {
    /// The path of a route in a block of `resources`: the path of the blocks, and the nested path of
    /// the resource.
    fn nested_prefix(&self) -> Option<Prefix> {
        match &self.resource {
            Some(context) => join(&self.prefix, context.nested.as_ref()?),
            None => self.prefix.clone(),
        }
    }
}

/// The paths of a resource after the path of the blocks around it, without a leading `/`.
#[derive(Clone)]
pub(super) struct ResourceContext {
    singular: bool,
    collection: String,
    member: String,
    /// The path for the routes of a nested resource. It is `None` when omen does not know the
    /// singular name that the path needs.
    nested: Option<String>,
}

struct Opt<'t> {
    key: String,
    string_key: bool,
    value: Node<'t>,
}

struct Options<'t>(Vec<Opt<'t>>);

impl<'t> Options<'t> {
    fn get(&self, key: &str) -> Option<Node<'t>> {
        self.0
            .iter()
            .rfind(|option| !option.string_key && option.key == key)
            .map(|option| option.value)
    }
}

struct Target {
    controller: Option<ControllerRef>,
    action: Option<String>,
}

fn next_id() -> usize {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The controller of a `controller` block or a resource block. The text is shared by each route in
/// the block, and the id stands for it in a key, so a route does not hash or copy a long name.
#[derive(Clone)]
pub(super) struct ScopedController {
    text: Rc<str>,
    id: usize,
}

impl ScopedController {
    fn new(text: String) -> Self {
        Self {
            text: Rc::from(text),
            id: next_id(),
        }
    }
}

/// The controller of one route.
enum ControllerRef {
    Scoped(ScopedController),
    Text(String),
}

impl ControllerRef {
    fn text(&self) -> &str {
        match self {
            Self::Scoped(controller) => &controller.text,
            Self::Text(text) => text,
        }
    }

    fn key(&self) -> ControllerKey {
        match self {
            Self::Scoped(controller) => ControllerKey::Scoped(controller.id),
            Self::Text(text) => ControllerKey::Text(text.clone()),
        }
    }
}

/// A chain of modules, a controller, and an action.
pub(super) type HandlerKey = (usize, ControllerKey, String);

#[derive(Hash, PartialEq, Eq)]
pub(super) enum ControllerKey {
    Scoped(usize),
    Text(String),
}

/// Appends the bytes of an escape sequence of a Ruby string in double quotes. The escapes that omen
/// models are `\n \t \r \a \b \e \f \v \s`, `\0` and the octal form, `\xHH`, `\uXXXX`, `\u{...}`,
/// and a backslash before another character. The forms `\c`, `\C-`, `\M-`, and a backslash before
/// a line break give `None`.
fn decode_escape(escape: &str, out: &mut Vec<u8>) -> Option<()> {
    let characters: Vec<char> = escape.chars().collect();
    if characters.first() != Some(&'\\') || characters.len() < 2 {
        return None;
    }
    let push = |character: char, out: &mut Vec<u8>| {
        let mut buffer = [0u8; 4];
        out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    };
    match characters[1] {
        'n' => out.push(b'\n'),
        't' => out.push(b'\t'),
        'r' => out.push(b'\r'),
        'a' => out.push(7),
        'b' => out.push(8),
        'e' => out.push(27),
        'f' => out.push(12),
        'v' => out.push(11),
        's' => out.push(b' '),
        'c' | 'C' | 'M' | '\n' | '\r' => return None,
        'x' => {
            let digits: String = characters[2..].iter().collect();
            if digits.is_empty() || digits.len() > 2 {
                return None;
            }
            out.push(u8::from_str_radix(&digits, 16).ok()?);
        }
        'u' => {
            let rest: String = characters[2..].iter().collect();
            let codes: Vec<&str> = match rest.strip_prefix('{') {
                Some(braced) => braced.strip_suffix('}')?.split_whitespace().collect(),
                None if rest.len() == 4 => vec![rest.as_str()],
                None => return None,
            };
            if codes.is_empty() {
                return None;
            }
            for code in codes {
                if code.len() > 6 {
                    return None;
                }
                push(char::from_u32(u32::from_str_radix(code, 16).ok()?)?, out);
            }
        }
        '0'..='7' => {
            let digits: String = characters[1..].iter().collect();
            if digits.len() > 3 || !digits.chars().all(|digit| ('0'..='7').contains(&digit)) {
                return None;
            }
            out.push((u32::from_str_radix(&digits, 8).ok()? & 0xFF) as u8);
        }
        other => {
            if characters.len() != 2 {
                return None;
            }
            push(other, out);
        }
    }
    Some(())
}

/// The value of a Ruby string literal in double quotes or single quotes. An interpolation, a form
/// such as `%q(...)`, and an escape that omen does not model give `None`.
fn evaluate_string(node: Node<'_>, source: &[u8]) -> Option<String> {
    let text = super::syntax::text(node, source);
    let mut bytes: Vec<u8> = Vec::new();
    match text.as_bytes().first()? {
        b'\'' => {
            let body = text.strip_prefix('\'')?.strip_suffix('\'')?;
            let mut characters = body.chars().peekable();
            while let Some(character) = characters.next() {
                if character == '\\' && matches!(characters.peek(), Some('\\' | '\'')) {
                    let escaped = characters.next()?;
                    bytes.extend_from_slice(escaped.to_string().as_bytes());
                } else {
                    bytes.extend_from_slice(character.to_string().as_bytes());
                }
            }
        }
        b'"' => {
            let mut cursor = node.walk();
            for part in node.named_children(&mut cursor) {
                match part.kind() {
                    "string_content" => {
                        let content = super::syntax::text(part, source);
                        if content.contains("#$") || content.contains("#@") {
                            return None;
                        }
                        bytes.extend_from_slice(content.as_bytes());
                    }
                    "escape_sequence" => {
                        decode_escape(super::syntax::text(part, source), &mut bytes)?
                    }
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    String::from_utf8(bytes).ok()
}

/// The kind of a literal `true`, `false`, or `nil`, also in the pattern of a `when`.
fn literal_kind(node: Node<'_>) -> Option<&'static str> {
    match node.kind() {
        "true" => Some("true"),
        "false" => Some("false"),
        "nil" => Some("nil"),
        "pattern" => node.named_child(0).and_then(literal_kind),
        _ => None,
    }
}

/// The children of a `case` that can run, when the subject and each pattern of each `when` are the
/// literals `true`, `false`, or `nil`: the subject, the first `when` with a pattern equal to the
/// subject, and the `else` when no `when` matches. For any other `case`, the result is `None`, as
/// omen cannot tell which branch runs.
fn literal_case_children(case: Node<'_>) -> Option<Vec<Node<'_>>> {
    if case.kind() != "case" {
        return None;
    }
    let subject = literal_kind(case.child_by_field_name("value")?)?;
    let mut cursor = case.walk();
    let children: Vec<Node<'_>> = case.named_children(&mut cursor).collect();
    let mut winner: Option<Node<'_>> = None;
    for child in children.iter().filter(|child| child.kind() == "when") {
        let mut pattern_cursor = child.walk();
        let patterns: Vec<Node<'_>> = child
            .children_by_field_name("pattern", &mut pattern_cursor)
            .collect();
        let kinds: Option<Vec<&str>> = patterns.into_iter().map(literal_kind).collect();
        if kinds?.contains(&subject) && winner.is_none() {
            winner = Some(*child);
        }
    }
    Some(
        children
            .into_iter()
            .filter(|child| match child.kind() {
                "when" => Some(*child) == winner,
                "else" => winner.is_none(),
                _ => true,
            })
            .collect(),
    )
}

/// The class name of a name with `_`, as `camelize` of ActiveSupport gives it. A registered acronym
/// keeps its form.
fn camelize(part: &str, acronyms: &HashMap<String, String>) -> String {
    part.split('_')
        .map(|word| {
            if let Some(acronym) = acronyms.get(word) {
                return acronym.clone();
            }
            let mut characters = word.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// The rules of `ActiveSupport::Inflector#singularize` in Rails 7.2.2, from
/// `activesupport/lib/active_support/inflections.rb`. The pairs are in the order that the file
/// defines them. Rails applies the last rule first, and the first rule that matches wins. The
/// last rules are the singular rules of `inflect.irregular`.
const SINGULAR_RULES: [(&str, &str); 39] = [
    ("s$", ""),
    ("(ss)$", "${1}"),
    ("(n)ews$", "${1}ews"),
    ("([ti])a$", "${1}um"),
    (
        "((a)naly|(b)a|(d)iagno|(p)arenthe|(p)rogno|(s)ynop|(t)he)(sis|ses)$",
        "${1}sis",
    ),
    ("(^analy)(sis|ses)$", "${1}sis"),
    ("([^f])ves$", "${1}fe"),
    ("(hive)s$", "${1}"),
    ("(tive)s$", "${1}"),
    ("([lr])ves$", "${1}f"),
    ("([^aeiouy]|qu)ies$", "${1}y"),
    ("(s)eries$", "${1}eries"),
    ("(m)ovies$", "${1}ovie"),
    ("(x|ch|ss|sh)es$", "${1}"),
    ("^(m|l)ice$", "${1}ouse"),
    ("(bus)(es)?$", "${1}"),
    ("(o)es$", "${1}"),
    ("(shoe)s$", "${1}"),
    ("(cris|test)(is|es)$", "${1}is"),
    ("^(a)x[ie]s$", "${1}xis"),
    ("(octop|vir)(us|i)$", "${1}us"),
    ("(alias|status)(es)?$", "${1}"),
    ("^(ox)en", "${1}"),
    ("(vert|ind)ices$", "${1}ex"),
    ("(matr)ices$", "${1}ix"),
    ("(quiz)zes$", "${1}"),
    ("(database)s$", "${1}"),
    ("(p)erson$", "${1}erson"),
    ("(p)eople$", "${1}erson"),
    ("(m)an$", "${1}an"),
    ("(m)en$", "${1}an"),
    ("(c)hild$", "${1}hild"),
    ("(c)hildren$", "${1}hild"),
    ("(s)ex$", "${1}ex"),
    ("(s)exes$", "${1}ex"),
    ("(m)ove$", "${1}ove"),
    ("(m)oves$", "${1}ove"),
    ("(z)ombie$", "${1}ombie"),
    ("(z)ombies$", "${1}ombie"),
];

const UNCOUNTABLE_WORDS: [&str; 10] = [
    "equipment",
    "information",
    "rice",
    "money",
    "species",
    "series",
    "fish",
    "sheep",
    "jeans",
    "police",
];

/// The plural rules of Rails 7.2.2, in the order that `inflections.rb` defines them. Rails applies
/// the last rule first. The last rules are the plural rules of `inflect.irregular`.
const PLURAL_RULES: [(&str, &str); 33] = [
    ("$", "s"),
    ("s$", "s"),
    ("^(ax|test)is$", "${1}es"),
    ("(octop|vir)us$", "${1}i"),
    ("(octop|vir)i$", "${1}i"),
    ("(alias|status)$", "${1}es"),
    ("(bu)s$", "${1}ses"),
    ("(buffal|tomat)o$", "${1}oes"),
    ("([ti])um$", "${1}a"),
    ("([ti])a$", "${1}a"),
    ("sis$", "ses"),
    ("(?:([^f])fe|([lr])f)$", "${1}${2}ves"),
    ("(hive)$", "${1}s"),
    ("([^aeiouy]|qu)y$", "${1}ies"),
    ("(x|ch|ss|sh)$", "${1}es"),
    ("(matr|vert|ind)(?:ix|ex)$", "${1}ices"),
    ("^(m|l)ouse$", "${1}ice"),
    ("^(m|l)ice$", "${1}ice"),
    ("^(ox)$", "${1}en"),
    ("^(oxen)$", "${1}"),
    ("(quiz)$", "${1}zes"),
    ("(p)erson$", "${1}eople"),
    ("(p)eople$", "${1}eople"),
    ("(m)an$", "${1}en"),
    ("(m)en$", "${1}en"),
    ("(c)hild$", "${1}hildren"),
    ("(c)hildren$", "${1}hildren"),
    ("(s)ex$", "${1}exes"),
    ("(s)exes$", "${1}exes"),
    ("(m)ove$", "${1}oves"),
    ("(m)oves$", "${1}oves"),
    ("(z)ombie$", "${1}ombies"),
    ("(z)ombies$", "${1}ombies"),
];

type CompiledRules = Vec<(regex::Regex, &'static str)>;

fn compile_rules(rules: &[(&'static str, &'static str)]) -> CompiledRules {
    rules
        .iter()
        .rev()
        .map(|(pattern, replacement)| {
            (
                regex::Regex::new(&format!("(?i){pattern}")).expect("a fixed pattern"),
                *replacement,
            )
        })
        .collect()
}

/// Applies the rules of an inflection as `apply_inflections` of ActiveSupport does: an uncountable
/// word stays as it is, and the first rule that matches changes the word one time.
fn inflect(word: &str, rules: &CompiledRules) -> String {
    let lower = word.to_lowercase();
    let uncountable = UNCOUNTABLE_WORDS.iter().any(|candidate| {
        lower.strip_suffix(candidate).is_some_and(|before| {
            before
                .chars()
                .next_back()
                .is_none_or(|last| !(last.is_alphanumeric() || last == '_'))
        })
    });
    if word.is_empty() || uncountable {
        return word.to_string();
    }
    for (rule, replacement) in rules {
        if rule.is_match(word) {
            return rule.replace(word, *replacement).into_owned();
        }
    }
    word.to_string()
}

/// The singular of a word as Rails 7.2.2 gives it with its default inflections. The rules of an
/// application can change the result. See `Context::rails_singular`.
fn singularize(word: &str) -> String {
    static RULES: std::sync::LazyLock<CompiledRules> =
        std::sync::LazyLock::new(|| compile_rules(&SINGULAR_RULES));
    inflect(word, &RULES)
}

/// The plural of a word as Rails 7.2.2 gives it with its default inflections. The rules of an
/// application can change the result. See `Context::rails_plural`.
fn pluralize(word: &str) -> String {
    static RULES: std::sync::LazyLock<CompiledRules> =
        std::sync::LazyLock::new(|| compile_rules(&PLURAL_RULES));
    inflect(word, &RULES)
}

/// The path that `Journey::Router::Utils.normalize_path` and `Mapper.normalize_path` of Rails 7.2.2
/// give: one leading `/`, no repeated `/`, no trailing `/` except for the root path, the
/// percent codes in upper case, and the optional segments in front of the `/`.
fn normalize_path(path: &str) -> String {
    static ENCODED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new("(%[a-f0-9]{2})").expect("a fixed pattern"));
    static OPEN: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"/(\(+)/?").expect("a fixed pattern"));
    static ALL_OPTIONAL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(\(+[^)]+\))(\(+/:[^)]+\))*$").expect("a fixed pattern")
    });
    static LEADING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^(\(+)/").expect("a fixed pattern"));
    let mut text = String::from("/");
    for character in path.chars() {
        if character == '/' && text.ends_with('/') {
            continue;
        }
        text.push(character);
    }
    if text != "/" {
        if text.ends_with('/') {
            text.pop();
        }
        text = ENCODED
            .replace_all(&text, |captures: &regex::Captures<'_>| {
                captures[1].to_uppercase()
            })
            .into_owned();
    }
    text = OPEN.replace_all(&text, "${1}/").into_owned();
    if ALL_OPTIONAL.is_match(&text) {
        text = LEADING.replace(&text, "/${1}").into_owned();
    }
    text
}

/// The escape of `URI::RFC2396_Parser#escape` that the mapper applies to the path of a route: each
/// byte that is not a letter, a digit, or one of `-_.!~*'();/?:@&=+$,[]` becomes `%` and two
/// upper case hexadecimal digits. Thus `%` becomes `%25`.
fn escape_route_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'();/?:@&=+$,[]".contains(&byte) {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

/// The path of a route, as `add_route` of the mapper gives it: the path of the blocks, a `/`, and
/// the path of the route, escaped and then normalized.
fn route_path(base: &Prefix, part: &str) -> String {
    normalize_path(&escape_route_path(&format!("{}/{part}", base.text())))
}

/// The normalized path of the blocks around a route, as a chain of segments. A new segment does
/// not copy the earlier segments, so each block takes a constant time. The text of a prefix takes a
/// time in proportion to its depth, and each route asks for it one time. `None` is the root path
/// `/`.
#[derive(Clone)]
pub(super) struct Prefix(Option<Rc<PrefixNode>>);

struct PrefixNode {
    parent: Option<Rc<PrefixNode>>,
    /// The segment without a `/` at its start or its end, and with upper case percent codes.
    segment: String,
}

impl Drop for PrefixNode {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(mut owned) => next = owned.parent.take(),
                Err(_) => break,
            }
        }
    }
}

impl Prefix {
    fn root() -> Self {
        Self(None)
    }

    /// The text of the path: a `/`, and the segments with a `/` between them.
    fn text(&self) -> String {
        let mut segments: Vec<&str> = Vec::new();
        let mut node = self.0.as_deref();
        while let Some(current) = node {
            segments.push(current.segment.as_str());
            node = current.parent.as_deref();
        }
        segments.reverse();
        format!("/{}", segments.join("/"))
    }

    /// The prefix after this prefix and a part of a path, as `merge_path_scope` of Rails gives it.
    /// A part with a parenthesis gives `None`: Rails rewrites the optional segments over the whole
    /// path, and omen does not model it.
    fn push(&self, part: &str) -> Option<Self> {
        if part.contains(['(', ')']) {
            return None;
        }
        let segment = normalize_path(part);
        let segment = segment.trim_matches('/');
        if segment.is_empty() {
            return Some(self.clone());
        }
        Some(Self(Some(Rc::new(PrefixNode {
            parent: self.0.clone(),
            segment: segment.to_string(),
        }))))
    }
}

/// The prefix after a prefix and a part of a path. A prefix that is not known gives no prefix.
fn join(prefix: &Option<Prefix>, part: &str) -> Option<Prefix> {
    prefix.as_ref().and_then(|prefix| prefix.push(part))
}

impl<'a> Context<'a> {
    pub(super) fn ruby(&self, node: Node<'_>, scope: Option<&Rc<Scope>>, out: &mut Vec<Draft>) {
        if node.kind() != "call" || !self.file.file.ends_with("routes.rb") {
            return;
        }
        let Some(method) = node.child_by_field_name("method") else {
            return;
        };
        if node.child_by_field_name("receiver").is_some() {
            return;
        }
        let name = self.text(method);
        if !matches!(
            name,
            "get"
                | "post"
                | "put"
                | "patch"
                | "delete"
                | "match"
                | "root"
                | "resources"
                | "resource"
        ) {
            return;
        }
        let Some(scope) = scope.filter(|scope| scope.draw) else {
            return;
        };
        let scope: &Scope = scope;
        match name {
            "resources" | "resource" => self.rails_resources(node, name == "resource", scope, out),
            _ => self.rails_plain_route(node, name, scope, out),
        }
    }

    /// The text of a string without interpolation, or of a symbol.
    fn rails_literal(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "string" => self.ruby_string_value(node),
            "simple_symbol" => Some(self.text(node).trim_start_matches(':').to_string()),
            "bare_symbol" | "hash_key_symbol" => Some(self.text(node).to_string()),
            _ => None,
        }
    }

    fn ruby_string_value(&self, node: Node<'_>) -> Option<String> {
        evaluate_string(node, self.source)
    }

    /// One literal, or a list of literals.
    fn rails_names(&self, node: Node<'_>) -> Option<Vec<String>> {
        if !matches!(node.kind(), "array" | "symbol_array" | "string_array") {
            return self.rails_literal(node).map(|name| vec![name]);
        }
        let mut cursor = node.walk();
        let items: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        items
            .into_iter()
            .map(|item| self.rails_literal(item))
            .collect()
    }

    /// The pairs of the arguments, from `key: value` and from `{ }` hashes. A splat gives `None`.
    fn rails_options<'t>(&self, arguments: &[Node<'t>]) -> Option<Options<'t>> {
        let mut found = Vec::new();
        for argument in arguments {
            match argument.kind() {
                "pair" => found.push(self.rails_pair(*argument)?),
                "hash" => {
                    let mut cursor = argument.walk();
                    let pairs: Vec<Node<'t>> = argument.named_children(&mut cursor).collect();
                    for pair in pairs {
                        if pair.kind() != "pair" {
                            return None;
                        }
                        found.push(self.rails_pair(pair)?);
                    }
                }
                "hash_splat_argument" => return None,
                _ => {}
            }
        }
        Some(Options(found))
    }

    fn rails_pair<'t>(&self, pair: Node<'t>) -> Option<Opt<'t>> {
        let key = pair.child_by_field_name("key")?;
        let value = pair.child_by_field_name("value")?;
        let mut cursor = pair.walk();
        let rocket = pair
            .children(&mut cursor)
            .any(|child| !child.is_named() && child.kind() == "=>");
        Some(Opt {
            key: self.rails_literal(key)?,
            string_key: rocket && key.kind() == "string",
            value,
        })
    }

    /// The request methods of a `via:` value. Only symbols have a meaning that omen knows: `:all` is
    /// any method, and another symbol is the dashed upper case form, as `:m_search` is `M-SEARCH`. A
    /// string, or `:all` with another value, gives `None`.
    fn rails_via(&self, value: Node<'_>) -> Option<Vec<String>> {
        let mut cursor = value.walk();
        let items: Vec<Node<'_>> = if matches!(value.kind(), "array" | "symbol_array") {
            value.named_children(&mut cursor).collect()
        } else {
            vec![value]
        };
        let mut verbs = Vec::new();
        for item in items {
            if !matches!(item.kind(), "simple_symbol" | "bare_symbol") {
                return None;
            }
            let name = self.text(item).trim_start_matches(':');
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
            {
                return None;
            }
            verbs.push(name);
        }
        if verbs.is_empty() {
            return None;
        }
        if verbs.contains(&"all") {
            return (verbs.len() == 1).then(|| vec![String::from("ANY")]);
        }
        Some(
            verbs
                .into_iter()
                .map(|verb| verb.replace('_', "-").to_uppercase())
                .collect(),
        )
    }

    fn rails_positional<'t>(arguments: &[Node<'t>]) -> Vec<Node<'t>> {
        arguments
            .iter()
            .copied()
            .filter(|argument| !matches!(argument.kind(), "pair" | "hash" | "hash_splat_argument"))
            .collect()
    }

    /// The scope for the code inside the node, from the scope outside it. Only these nodes can hold
    /// a route: a list of statements, a block, a call with a block, a condition that omen can
    /// evaluate, and a `case` that omen can evaluate. Any other node, such as an expression, an
    /// assignment, an argument, a method, or a class, makes the scope not readable. The block of a
    /// call enters the scope of the call.
    pub(super) fn scope_inside(
        &self,
        node: Node<'_>,
        parent: Option<Node<'_>>,
        outer: Rc<Scope>,
    ) -> Option<Rc<Scope>> {
        if !matches!(
            node.kind(),
            "program"
                | "body_statement"
                | "block_body"
                | "then"
                | "else"
                | "elsif"
                | "if"
                | "unless"
                | "if_modifier"
                | "unless_modifier"
                | "conditional"
                | "case"
                | "when"
                | "do_block"
                | "block"
                | "call"
        ) {
            return None;
        }
        let Some(parent) = parent else {
            return Some(outer);
        };
        if self.follows_jump(parent, node) {
            return None;
        }
        match parent.kind() {
            "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
                if parent.child_by_field_name("condition") == Some(node)
                    || Self::is_dead_branch(parent, node)
                {
                    return None;
                }
            }
            "case" => match self.case_alive(parent) {
                Some(alive) if alive.contains(&node.id()) => {}
                _ => return None,
            },
            "call" => {
                if parent.child_by_field_name("block") != Some(node) {
                    return None;
                }
                if matches!(
                    parent
                        .child_by_field_name("method")
                        .map(|method| self.text(method)),
                    Some("constraints" | "defaults")
                ) && parent.child_by_field_name("receiver").is_none()
                {
                    return Some(outer);
                }
                return self.rails_enter(parent, (*outer).clone()).map(Rc::new);
            }
            _ => {}
        }
        Some(outer)
    }

    /// Tells if the node comes after a statement of the same list that jumps out of the block:
    /// `next`, `break`, `return`, `redo`, or `retry`. Ruby does not run such a node. The search
    /// keeps the place of the first jump of each list.
    fn follows_jump(&self, list: Node<'_>, node: Node<'_>) -> bool {
        if !matches!(
            list.kind(),
            "program" | "body_statement" | "block_body" | "then" | "else"
        ) {
            return false;
        }
        let first = {
            let known = self.rails_jumps.borrow().get(&list.id()).copied();
            match known {
                Some(first) => first,
                None => {
                    let mut cursor = list.walk();
                    let first = list
                        .named_children(&mut cursor)
                        .find(|statement| {
                            matches!(
                                statement.kind(),
                                "next" | "break" | "return" | "redo" | "retry"
                            )
                        })
                        .map(|statement| statement.start_byte());
                    self.rails_jumps.borrow_mut().insert(list.id(), first);
                    first
                }
            }
        };
        first.is_some_and(|start| node.start_byte() > start)
    }

    /// The ids of the children of a `case` that can run, when omen can evaluate the `case`. The
    /// search keeps the result for each `case`, so each branch does not read all the patterns.
    fn case_alive(&self, case: Node<'_>) -> Option<Rc<Vec<usize>>> {
        if let Some(known) = self.rails_cases.borrow().get(&case.id()) {
            return known.clone();
        }
        let alive = literal_case_children(case)
            .map(|children| Rc::new(children.iter().map(Node::id).collect::<Vec<_>>()));
        self.rails_cases
            .borrow_mut()
            .insert(case.id(), alive.clone());
        alive
    }

    /// Tells if a literal condition makes the branch of `child` dead: `if false`, `if nil`,
    /// `unless true`, and the `else` of `if true`. A condition that is not a literal is live.
    fn is_dead_branch(parent: Node<'_>, child: Node<'_>) -> bool {
        let Some(condition) = parent.child_by_field_name("condition") else {
            return false;
        };
        if condition == child {
            return false;
        }
        let truth = match condition.kind() {
            "true" => true,
            "false" | "nil" => false,
            _ => return false,
        };
        let inverted = matches!(parent.kind(), "unless" | "unless_modifier");
        let runs_when = truth != inverted;
        let main_branch = parent
            .child_by_field_name("consequence")
            .or_else(|| parent.child_by_field_name("body"));
        let in_main = main_branch == Some(child);
        in_main != runs_when
    }

    /// Tells if the call is `draw` of the routes of the application or of an engine:
    /// `Rails.application.routes`, `Name::Application.routes`, or `Name::Engine.routes`.
    fn is_draw_block(&self, call: Node<'_>) -> bool {
        call.child_by_field_name("method")
            .is_some_and(|method| self.text(method) == "draw")
            && call
                .child_by_field_name("receiver")
                .is_some_and(|receiver| self.routes_object_is_defined(self.text(receiver)))
    }

    fn rails_enter(&self, call: Node<'_>, mut scope: Scope) -> Option<Scope> {
        let method = self.text(call.child_by_field_name("method")?);
        let receiver = call.child_by_field_name("receiver").is_some();
        if self.is_draw_block(call) {
            let engine = call
                .child_by_field_name("receiver")
                .is_some_and(|receiver| {
                    self.text(receiver)
                        .trim()
                        .trim_start_matches("::")
                        .ends_with("::Engine.routes")
                });
            if engine {
                return None;
            }
            scope.draw = true;
            scope.api_only = call
                .child_by_field_name("receiver")
                .map_or(ApiOnly::Unknown, |receiver| {
                    self.rails_api_only(self.text(receiver))
                });
            return Some(scope);
        }
        if receiver {
            return None;
        }
        if matches!(method, "constraints" | "defaults") {
            return Some(scope);
        }
        let arguments = Self::arguments_of(call);
        let options = self.rails_options(&arguments)?;
        let positional = Self::rails_positional(&arguments);
        match method {
            "namespace" => {
                if options
                    .0
                    .iter()
                    .any(|option| !matches!(option.key.as_str(), "path" | "module" | "as"))
                    || positional.len() != 1
                {
                    return None;
                }
                let name = self.rails_literal(positional[0])?;
                let path = options
                    .get("path")
                    .map_or(Some(name.clone()), |value| self.rails_literal(value))?;
                let module = options
                    .get("module")
                    .map_or(Some(name), |value| self.rails_literal(value))?;
                scope.prefix = join(&scope.nested_prefix(), &path);
                scope.modules = scope.modules.extend(split_module(&module));
                if scope.resource.take().is_some() {
                    scope.method_scope = false;
                }
                Some(scope)
            }
            "scope" => {
                const SCOPE_OPTIONS: [&str; 6] = [
                    "path",
                    "module",
                    "controller",
                    "as",
                    "constraints",
                    "defaults",
                ];
                if options
                    .0
                    .iter()
                    .any(|option| !SCOPE_OPTIONS.contains(&option.key.as_str()))
                    || positional.len() > 1
                {
                    return None;
                }
                let path = match positional.first() {
                    Some(argument) => Some(self.rails_literal(*argument)?),
                    None => match options.get("path") {
                        Some(value) => Some(self.rails_literal(value)?),
                        None => None,
                    },
                };
                if let Some(path) = path {
                    scope.prefix = join(&scope.prefix, &path);
                }
                if let Some(value) = options.get("module") {
                    scope.modules = scope
                        .modules
                        .extend(split_module(&self.rails_literal(value)?));
                }
                if let Some(value) = options.get("controller") {
                    scope.controller = Some(ScopedController::new(self.rails_literal(value)?));
                }
                Some(scope)
            }
            "controller" => {
                if positional.len() != 1 || !options.0.is_empty() {
                    return None;
                }
                scope.controller = Some(ScopedController::new(self.rails_literal(positional[0])?));
                Some(scope)
            }
            "member" | "collection" => {
                let resource = scope.resource.take()?;
                if !positional.is_empty() || !options.0.is_empty() {
                    return None;
                }
                let part = if method == "member" {
                    &resource.member
                } else {
                    &resource.collection
                };
                scope.prefix = join(&scope.prefix, part);
                scope.method_scope = true;
                Some(scope)
            }
            "resources" | "resource" => {
                let singular = method == "resource";
                let definition = self.rails_resource_definition(&positional, &options, &scope)?;
                if definition.names.len() != 1
                    || scope.method_scope
                    || matches!(scope.api_only, ApiOnly::Unknown)
                {
                    return None;
                }
                let name = &definition.names[0];
                let naming = definition.as_name.as_deref().unwrap_or(name);
                let context = definition.context(singular, name, self.rails_singular(naming));
                scope.prefix = scope.nested_prefix();
                scope.controller = Some(ScopedController::new(definition.controller_for(
                    singular,
                    name,
                    self.rails_plural(name),
                )?));
                scope.modules = definition.modules.clone();
                scope.resource = Some(context);
                scope.method_scope = false;
                Some(scope)
            }
            _ => None,
        }
    }

    fn rails_resource_definition(
        &self,
        positional: &[Node<'_>],
        options: &Options<'_>,
        scope: &Scope,
    ) -> Option<ResourceDefinition> {
        let names: Option<Vec<String>> = positional
            .iter()
            .map(|argument| self.rails_literal(*argument))
            .collect();
        let names = names?;
        if names.is_empty() {
            return None;
        }
        let mut only = None;
        let mut except = Vec::new();
        let mut path = None;
        let mut controller = None;
        let mut as_name = None;
        let mut module: Option<String> = None;
        let mut modules = scope.modules.clone();
        for option in &options.0 {
            if option.string_key {
                return None;
            }
            match option.key.as_str() {
                "only" => only = Some(self.rails_names(option.value)?),
                "except" => except = self.rails_names(option.value)?,
                "path" => path = Some(self.rails_literal(option.value)?),
                "controller" => controller = Some(self.rails_literal(option.value)?),
                "as" => as_name = Some(self.rails_literal(option.value)?),
                "format" if option.value.kind() == "false" => {}
                "module" => module = Some(self.rails_literal(option.value)?),
                key if HARMLESS_OPTIONS.contains(&key) => {}
                _ => return None,
            }
        }
        if let Some(module) = module {
            modules = modules.extend(split_module(&module));
        }
        Some(ResourceDefinition {
            names,
            only,
            except,
            path,
            controller,
            as_name,
            modules,
        })
    }

    fn rails_resources(&self, node: Node<'_>, singular: bool, scope: &Scope, out: &mut Vec<Draft>) {
        let arguments = Self::arguments_of(node);
        let Some(options) = self.rails_options(&arguments) else {
            return;
        };
        let positional = Self::rails_positional(&arguments);
        let Some(definition) = self.rails_resource_definition(&positional, &options, scope) else {
            return;
        };
        if scope.method_scope {
            return;
        }
        let api_only = match scope.api_only {
            ApiOnly::On => true,
            ApiOnly::Off => false,
            ApiOnly::Unknown => return,
        };
        let Some(base) = scope.nested_prefix() else {
            return;
        };
        let table: &[(&str, &str, &str)] = if singular {
            &SINGULAR_ACTIONS
        } else {
            &COLLECTION_ACTIONS
        };
        for name in &definition.names {
            let context = definition.context(singular, name, None);
            let controller = definition.controller_for(singular, name, self.rails_plural(name));
            for (action, verb, suffix) in table {
                if !definition.allows(action, api_only) {
                    continue;
                }
                let path = route_path(&base, &format!("{}{suffix}", context.collection));
                let variant_path = path.clone();
                let (symbol, guards) = match &controller {
                    Some(controller) => self.rails_handler_cached(
                        &definition.modules,
                        &ControllerRef::Text(controller.clone()),
                        action,
                    ),
                    None => (None, Vec::new()),
                };
                out.push(Draft {
                    range: Span::of(&node),
                    kind: EntryKind::HttpHandler,
                    symbol,
                    framework: "rails",
                    route: Some(route(verb, path)),
                    guards,
                    basis: EntryBasis::Registered,
                    variant: Some(format!("{action}@{name}@{variant_path}")),
                });
            }
        }
    }

    /// `get`, `post`, `put`, `patch`, `delete`, `match`, and `root`. Omen reads the options `to`,
    /// `controller`, `action`, `on`, `via` of `match`, and `format: false`. The options `as`,
    /// `constraints`, `defaults`, `anchor`, and a constraint with a regular expression do not
    /// change the path or the controller. Any other option skips the route.
    fn rails_plain_route(&self, node: Node<'_>, name: &str, scope: &Scope, out: &mut Vec<Draft>) {
        let arguments = Self::arguments_of(node);
        let Some(options) = self.rails_options(&arguments) else {
            return;
        };
        let positional = Self::rails_positional(&arguments);
        let rocket = options
            .0
            .iter()
            .find(|option| option.string_key)
            .and_then(|first| {
                options
                    .0
                    .iter()
                    .rfind(|option| option.string_key && option.key == first.key)
            });
        let root = name == "root";
        for option in options.0.iter().filter(|option| !option.string_key) {
            let known = match option.key.as_str() {
                "to" | "controller" | "action" | "on" | "as" | "constraints" | "defaults"
                | "anchor" => true,
                "via" => name == "match",
                "format" => option.value.kind() == "false",
                _ => option.value.kind() == "regex",
            };
            if !known {
                return;
            }
        }

        let mut target_text = None;
        let mut path_node = None;
        if root {
            match positional.as_slice() {
                [] => {}
                [one] => target_text = Some(*one),
                _ => return,
            }
        } else {
            match (positional.as_slice(), rocket) {
                ([one], None) => path_node = Some((*one, self.rails_literal(*one))),
                ([], Some(option)) => {
                    path_node = Some((option.value, Some(option.key.clone())));
                    target_text = Some(option.value);
                }
                _ => return,
            }
        }
        if let Some(to) = options.get("to") {
            target_text = Some(to);
        }
        let symbol_path = path_node
            .as_ref()
            .is_some_and(|(node, _)| node.kind() != "string")
            && rocket.is_none();
        let path_text = if root {
            String::new()
        } else {
            let Some((_, Some(text))) = &path_node else {
                return;
            };
            text.clone()
        };

        let on = match options.get("on") {
            Some(value) => match self.rails_literal(value).as_deref() {
                Some(on @ ("member" | "collection")) => Some(on.to_string()),
                _ => return,
            },
            None => None,
        };
        let (base, method_scope) = match (&on, &scope.resource) {
            (Some(on), Some(context)) => {
                let part = if on == "member" {
                    &context.member
                } else {
                    &context.collection
                };
                (join(&scope.prefix, part), true)
            }
            (Some(_), None) => return,
            (None, Some(context)) if root => {
                if context.singular {
                    return;
                }
                (join(&scope.prefix, &context.collection), false)
            }
            (None, Some(context)) if context.singular => {
                (join(&scope.prefix, &context.member), true)
            }
            (None, Some(_)) => (scope.nested_prefix(), false),
            (None, None) => (scope.prefix.clone(), scope.method_scope),
        };
        let Some(base) = base else {
            return;
        };
        let canonical = method_scope
            && symbol_path
            && ["index", "create", "new", "show", "update", "destroy"]
                .contains(&path_text.as_str());
        let full_path = if root || canonical {
            route_path(&base, "")
        } else {
            route_path(&base, &path_text)
        };

        let no_target = Target {
            controller: None,
            action: None,
        };
        let rocket_with_target = rocket.is_some()
            && ["to", "controller", "action"]
                .iter()
                .any(|key| options.get(key).is_some());
        let target = match target_text {
            _ if rocket_with_target => no_target,
            Some(node) if node.kind() == "string" => {
                let Some(text) = self.rails_literal(node) else {
                    return;
                };
                match text.split_once('#') {
                    Some((controller, action)) if !action.contains('#') => Target {
                        controller: Some(ControllerRef::Text(controller.to_string())),
                        action: Some(action.to_string()),
                    },
                    _ => no_target,
                }
            }
            Some(_) => no_target,
            None => {
                let read = |key: &str| match options.get(key) {
                    None => Some(None),
                    Some(value) => self.rails_literal(value).map(Some),
                };
                match (read("controller"), read("action")) {
                    (Some(controller), Some(action)) => {
                        let controller = controller
                            .map(ControllerRef::Text)
                            .or_else(|| scope.controller.clone().map(ControllerRef::Scoped));
                        let from_path = (!root
                            && !path_text.is_empty()
                            && path_text
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                        .then(|| path_text.replace('-', "_"));
                        let action = action.or(from_path.filter(|_| controller.is_some()));
                        Target { controller, action }
                    }
                    _ => no_target,
                }
            }
        };

        let methods: Vec<String> = match name {
            "match" => {
                let Some(via) = options.get("via").and_then(|value| self.rails_via(value)) else {
                    return;
                };
                via
            }
            "root" => vec![String::from("GET")],
            verb => vec![verb.to_uppercase()],
        };
        let (symbol, guards) = match (&target.controller, &target.action) {
            (Some(controller), Some(action)) => {
                self.rails_handler_cached(&scope.modules, controller, action)
            }
            _ => (None, Vec::new()),
        };
        for method in methods {
            out.push(Draft {
                range: Span::of(&node),
                kind: EntryKind::HttpHandler,
                symbol: symbol.clone(),
                framework: "rails",
                route: Some(route(&method, full_path.clone())),
                guards: guards.clone(),
                basis: EntryBasis::Registered,
                variant: None,
            });
        }
    }

    /// The shared index of the repository. The first routes file that asks builds it, and the
    /// other files of the analysis use it.
    fn rails_shared(&self) -> &RailsShared {
        self.all_parsed
            .shared()
            .rails
            .get_or_init(|| RailsShared::build(self.all))
    }

    fn rails_inflections(&self) -> &Inflections {
        self.rails_shared()
            .inflections
            .get_or_init(|| self.read_inflections())
    }

    /// Reads the files in `config/initializers`, and the files with `inflect` in the name. The
    /// comments of the generated `inflections.rb` change nothing.
    fn read_inflections(&self) -> Inflections {
        let mut found = Inflections::default();
        for (position, file) in self.all.iter().enumerate() {
            let name = file.file.rsplit('/').next().unwrap_or("");
            if file.language != Language::Ruby
                || !(file.file.contains("config/initializers/") || name.contains("inflect"))
            {
                continue;
            }
            if let Some(parsed) = self.all_parsed.tree(position) {
                read_inflection_calls(
                    parsed.root_node(),
                    &parsed.source,
                    0,
                    false,
                    &[],
                    &mut found,
                );
            }
        }
        if self
            .rails_shared()
            .classes
            .contains_key("ActiveSupport.Inflector")
        {
            found.unreadable_acronym = true;
            found.changes_singulars = true;
        }
        found
    }

    /// The singular name that a nested route needs, or `None` when the inflections of the
    /// application can change the name.
    fn rails_singular(&self, name: &str) -> Option<String> {
        (!self.rails_inflections().changes_singulars).then(|| singularize(name))
    }

    /// The plural name of a singular resource, or `None` when the inflections of the application
    /// can change the name.
    fn rails_plural(&self, name: &str) -> Option<String> {
        (!self.rails_inflections().changes_singulars).then(|| pluralize(name))
    }

    /// The value of `config.api_only` for the route set of a `draw` call: `Rails.application`, a
    /// class that extends `Rails::Application`, or an engine. Omen reads each Ruby file. An
    /// assignment in the body of the application class, or in a file in `config`, belongs to the
    /// application. An assignment in the body of an engine class belongs to that engine. Any
    /// other assignment, a value that is not `true` or `false`, and an assignment in a branch that
    /// omen cannot evaluate make the value unknown.
    fn rails_api_only(&self, receiver: &str) -> ApiOnly {
        let sets = self.rails_shared().api_only.get_or_init(|| {
            let mut sets = ApiOnlySets::default();
            for (position, file) in self.all.iter().enumerate() {
                if file.language != Language::Ruby {
                    continue;
                }
                let in_config = file.file.starts_with("config/") || file.file.contains("/config/");
                if let Some(parsed) = self.all_parsed.tree(position) {
                    self.collect_api_only(
                        parsed.root_node(),
                        &ApiScan {
                            source: &parsed.source,
                            in_config,
                        },
                        0,
                        false,
                        &mut Vec::new(),
                        &mut sets,
                    );
                }
            }
            sets
        });
        let Some(base) = receiver.trim().strip_suffix(".routes") else {
            return ApiOnly::Unknown;
        };
        let constant = base.trim_start_matches("::");
        if base == "Rails.application" || constant.ends_with("::Application") {
            sets.for_application()
        } else {
            sets.for_engine(&constant.replace("::", "."))
        }
    }

    /// The kind of the class with the identity: an application, an engine, or another class.
    fn rails_class_kind(&self, identity: &str) -> ClassKind {
        let superclass = |symbols: &[(usize, usize)]| {
            symbols.iter().find_map(|(position, index)| {
                self.all[*position].symbols[*index]
                    .supertypes
                    .first()
                    .map(|parent| parent.trim_start_matches("::").to_string())
            })
        };
        match self
            .rails_shared()
            .classes
            .get(identity)
            .and_then(|symbols| superclass(symbols))
            .as_deref()
        {
            Some("Rails::Application") => ClassKind::Application,
            Some("Rails::Engine") => ClassKind::Engine,
            _ => ClassKind::Other,
        }
    }

    /// Reads the assignments `x.api_only = value`. `names` are the modules and the classes around
    /// the node, and a flag that tells which of them is a class. The search goes through the
    /// statement lists, the literal conditions that it can evaluate, the class bodies, and the
    /// blocks. An assignment under any other kind of node, such as an expression, an argument, or
    /// a `begin` block, is not attributed to a route set.
    fn collect_api_only(
        &self,
        node: Node<'_>,
        scan: &ApiScan<'_>,
        depth: usize,
        conditional: bool,
        names: &mut Vec<(bool, String)>,
        out: &mut ApiOnlySets,
    ) {
        if depth > MAX_READ_DEPTH {
            out.unattributed = true;
            return;
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        match node.kind() {
            "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
                let condition = node.child_by_field_name("condition");
                let literal = condition
                    .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
                for part in children {
                    if Some(part) == condition {
                        self.check_api_only_subtree(part, scan, out);
                        continue;
                    }
                    if Self::is_dead_branch(node, part) {
                        continue;
                    }
                    self.collect_api_only(
                        part,
                        scan,
                        depth + 1,
                        conditional || !literal,
                        names,
                        out,
                    );
                }
            }
            "case" if literal_case_children(node).is_some() => {
                for child in literal_case_children(node).unwrap_or_default() {
                    self.collect_api_only(child, scan, depth + 1, conditional, names, out);
                }
            }
            "case" | "case_match" | "lambda" | "method" | "singleton_method" | "for" | "while"
            | "until" | "while_modifier" | "until_modifier" => {
                for child in children {
                    self.collect_api_only(child, scan, depth + 1, true, names, out);
                }
            }
            "class" | "module" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| super::syntax::text(name, scan.source).replace("::", "."))
                    .unwrap_or_default();
                names.push((node.kind() == "class", name));
                for child in children {
                    self.collect_api_only(child, scan, depth + 1, conditional, names, out);
                }
                names.pop();
            }
            "assignment" => {
                let assigns_api_only = node.child_by_field_name("left").is_some_and(|left| {
                    left.kind() == "call"
                        && left.child_by_field_name("method").is_some_and(|method| {
                            super::syntax::text(method, scan.source) == "api_only"
                        })
                });
                if assigns_api_only {
                    self.attribute_api_only(node, scan, conditional, names, out);
                }
                if let Some(right) = node.child_by_field_name("right") {
                    self.check_api_only_subtree(right, scan, out);
                }
            }
            "call" => {
                let configure = node
                    .child_by_field_name("method")
                    .is_some_and(|method| super::syntax::text(method, scan.source) == "configure");
                for child in children {
                    if matches!(child.kind(), "do_block" | "block") {
                        self.collect_api_only(
                            child,
                            scan,
                            depth + 1,
                            conditional || !configure,
                            names,
                            out,
                        );
                    } else {
                        self.check_api_only_subtree(child, scan, out);
                    }
                }
            }
            "program" | "body_statement" | "then" | "else" | "block_body" | "do_block"
            | "block" => {
                for child in children {
                    self.collect_api_only(child, scan, depth + 1, conditional, names, out);
                }
            }
            _ => self.check_api_only_subtree(node, scan, out),
        }
    }

    /// Marks an assignment of `api_only` under the node as not attributed to a route set.
    fn check_api_only_subtree(&self, node: Node<'_>, scan: &ApiScan<'_>, out: &mut ApiOnlySets) {
        if subtree_assigns_api_only(node, scan.source, 0) {
            out.unattributed = true;
        }
    }

    /// Gives the assignment `x.api_only = value` to a route set, by its receiver.
    fn attribute_api_only(
        &self,
        node: Node<'_>,
        scan: &ApiScan<'_>,
        conditional: bool,
        names: &[(bool, String)],
        out: &mut ApiOnlySets,
    ) {
        let value = match node.child_by_field_name("right").map(|right| right.kind()) {
            Some("true") if !conditional => Some(true),
            Some("false") if !conditional => Some(false),
            _ => None,
        };
        let receiver = node
            .child_by_field_name("left")
            .and_then(|left| left.child_by_field_name("receiver"));
        let receiver_text = receiver.map(|receiver| super::syntax::text(receiver, scan.source));
        let constant = receiver
            .filter(|receiver| {
                receiver.kind() == "call"
                    && receiver
                        .child_by_field_name("method")
                        .is_some_and(|method| super::syntax::text(method, scan.source) == "config")
            })
            .and_then(|receiver| receiver.child_by_field_name("receiver"))
            .map(|constant| {
                super::syntax::text(constant, scan.source)
                    .trim_start_matches("::")
                    .to_string()
            })
            .filter(|path| is_constant_path(path));
        if receiver_text == Some("Rails.application.config") {
            out.application.push(value);
        } else if let Some(path) = constant {
            let identity = path.replace("::", ".");
            match self.rails_class_kind(&identity) {
                ClassKind::Application => out.application.push(value),
                ClassKind::Engine => out.engines.entry(identity).or_default().push(value),
                ClassKind::Other => out.unattributed = true,
            }
        } else if receiver_text == Some("config") {
            match names.iter().rposition(|(is_class, _)| *is_class) {
                Some(class_depth) => {
                    let identity = names[..=class_depth]
                        .iter()
                        .map(|(_, name)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    match self.rails_class_kind(&identity) {
                        ClassKind::Application => out.application.push(value),
                        ClassKind::Engine => out.engines.entry(identity).or_default().push(value),
                        ClassKind::Other => out.unattributed = true,
                    }
                }
                None if scan.in_config => out.application.push(value),
                None => out.unattributed = true,
            }
        } else {
            out.unattributed = true;
        }
    }

    /// Tells if the text names the routes of the application or of an engine, and the repository
    /// defines the class: `Rails.application.routes`, `Name::Application.routes`, or
    /// `Name::Engine.routes`. The class must have the superclass `Rails::Application` or
    /// `Rails::Engine`.
    fn routes_object_is_defined(&self, text: &str) -> bool {
        let Some(base) = text.trim().strip_suffix(".routes") else {
            return false;
        };
        if base == "Rails.application" {
            return true;
        }
        let constant = base.trim_start_matches("::");
        let superclass = if constant.ends_with("::Application") {
            "Rails::Application"
        } else if constant.ends_with("::Engine") {
            "Rails::Engine"
        } else {
            return false;
        };
        let constant_path = constant.split("::").all(|part| {
            part.chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if !constant_path {
            return false;
        }
        let shared = self.rails_shared();
        shared
            .classes
            .get(&constant.replace("::", "."))
            .into_iter()
            .flatten()
            .any(|(position, index)| {
                self.all[*position].symbols[*index]
                    .supertypes
                    .first()
                    .is_some_and(|parent| parent.trim_start_matches("::") == superclass)
            })
    }

    /// The result of `rails_handler` for a route. The file keeps the result for each chain of
    /// modules, controller, and action, so a route in a block with a long name does not work out the
    /// name again.
    fn rails_handler_cached(
        &self,
        modules: &Modules,
        controller: &ControllerRef,
        action: &str,
    ) -> Handler {
        let known_key = (modules.id(), controller.key());
        let known = self.rails_known.borrow().get(&known_key).copied();
        let known = known.unwrap_or_else(|| {
            let known = self
                .rails_identity(&modules.to_vec(), controller.text())
                .is_some_and(|identity| self.rails_shared().classes.contains_key(&identity));
            self.rails_known.borrow_mut().insert(known_key, known);
            known
        });
        if !known {
            return (None, Vec::new());
        }
        let key = (modules.id(), controller.key(), action.to_string());
        if let Some(found) = self.rails_handled.borrow().get(&key) {
            return found.clone();
        }
        let result = self.rails_handler(&modules.to_vec(), controller.text(), action);
        self.rails_handled.borrow_mut().insert(key, result.clone());
        result
    }

    /// The action symbol and the guards of a controller. `controller` is `admin/users`, and
    /// `modules` are the modules that `namespace` and `scope module:` add. A controller path
    /// with a leading `/` ignores the modules. The class must have the same chain of modules as its
    /// container path. A route without a module matches only a class outside each module. The
    /// index keeps the result for each class and action. If the inflections of the application
    /// have an acronym that omen cannot read, the controller is not known.
    fn rails_handler(&self, modules: &[String], controller: &str, action: &str) -> Handler {
        let Some(identity) = self.rails_identity(modules, controller) else {
            return (None, Vec::new());
        };
        let shared = self.rails_shared();
        let key = (identity.clone(), action.to_string());
        if let Some(found) = shared.handled.lock().get(&key) {
            return found.clone();
        }
        let result = self.rails_resolve(shared, &identity, action);
        shared.handled.lock().insert(key, result.clone());
        result
    }

    /// The identity of the controller class, as the index keys it: the camelized modules and name
    /// with a `.` between them. A name that Rails cannot build from a valid path, and a controller
    /// that an acronym that omen cannot read may change, give `None`.
    fn rails_identity(&self, modules: &[String], controller: &str) -> Option<String> {
        let inflections = self.rails_inflections();
        if inflections.unreadable_acronym {
            return None;
        }
        let relative = controller.strip_prefix('/');
        let own: Vec<&str> = relative.unwrap_or(controller).split('/').collect();
        let valid = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
        };
        if !own.iter().all(|part| valid(part))
            || (relative.is_none() && !modules.iter().all(|module| valid(module)))
        {
            return None;
        }
        let mut segments: Vec<String> = if relative.is_some() {
            Vec::new()
        } else {
            modules.to_vec()
        };
        segments.extend(own.iter().map(|part| part.to_string()));
        let last = segments.pop()?;
        let mut qualified: Vec<String> = segments
            .iter()
            .map(|module| camelize(module, &inflections.acronyms))
            .collect();
        qualified.push(camelize(
            &format!("{last}_controller"),
            &inflections.acronyms,
        ));
        Some(qualified.join("."))
    }

    fn rails_resolve(&self, shared: &RailsShared, identity: &str, action: &str) -> Handler {
        if !shared.classes.contains_key(identity) {
            return (None, Vec::new());
        }
        let symbol = match self.rails_action(shared, identity, action) {
            Action::Hidden => return (None, Vec::new()),
            Action::Public(symbol) => symbol,
        };
        let guards = self.rails_program(shared, identity).guards_for(action);
        (symbol, guards)
    }

    /// Finds the nearest class in the chain of superclasses that decides if the action runs. A class
    /// decides when it defines an instance method with the name of the action, or when it calls
    /// `private`, `protected`, or `public` for that name. Rails runs an action only when the method
    /// is public. If the method is not public, or omen cannot read the visibility, the action is
    /// hidden: it has no symbol and no guard. If no class decides, a view can give the action.
    /// The index keeps the decision for each class that the search passes.
    fn rails_action(&self, shared: &RailsShared, identity: &str, action: &str) -> Action {
        let state = self.rails_action_state(shared, identity);
        let reach = state
            .map
            .get(&action.to_string())
            .copied()
            .unwrap_or(state.floor);
        if matches!(reach, Reach::Hidden) {
            return Action::Hidden;
        }
        let visibility = self.rails_visibility(shared, identity);
        let symbol = (|| {
            if !visibility.defines.contains(action) {
                return None;
            }
            let candidates = shared.methods.get(identity)?.get(action)?;
            let mut public = candidates.iter().filter(|(position, index)| {
                let symbol = &self.all[*position].symbols[*index];
                visibility
                    .public
                    .contains(&(*position, symbol.name_range.start_byte))
            });
            let found = public.next()?;
            public
                .next()
                .is_none()
                .then(|| self.all[found.0].symbols[found.1].key.to_string())
        })();
        Action::Public(symbol)
    }

    /// The reach of each action name for a class, as the nearest class in the chain that decides it
    /// gives it. A class adds its definitions and its visibility calls to the state of its
    /// superclass, and a class with visibility that omen cannot read hides each action that no class
    /// below it decides. The state shares the structure of the state above it, and the index keeps
    /// the state of each class, so the cost of an action does not depend on the length of the chain.
    fn rails_action_state(&self, shared: &RailsShared, identity: &str) -> Arc<ActionState> {
        if let Some(found) = shared.states.lock().get(identity) {
            return Arc::clone(found);
        }
        let mut chain = vec![identity.to_string()];
        let mut current = identity.to_string();
        let mut state = Arc::new(ActionState {
            floor: Reach::Public,
            map: PersistentMap::default(),
        });
        while let Some(Some(parent)) = shared.parents.get(&current) {
            if let Some(found) = shared.states.lock().get(parent) {
                state = Arc::clone(found);
                break;
            }
            chain.push(parent.clone());
            current = parent.clone();
        }
        for class in chain.iter().rev() {
            let visibility = self.rails_visibility(shared, class);
            let next = if visibility.unreadable {
                ActionState {
                    floor: Reach::Hidden,
                    map: PersistentMap::default(),
                }
            } else {
                let reach = |public: bool| if public { Reach::Public } else { Reach::Hidden };
                let mut map = state.map.clone();
                for (name, public) in &visibility.overrides {
                    map.insert(name.clone(), reach(*public));
                }
                for name in &visibility.defines {
                    map.insert(name.clone(), reach(visibility.public_names.contains(name)));
                }
                ActionState {
                    floor: state.floor,
                    map,
                }
            };
            state = Arc::new(next);
            shared
                .states
                .lock()
                .insert(class.clone(), Arc::clone(&state));
        }
        state
    }

    /// The callbacks of the chain of a class, in the order of Rails. A class with no callback of
    /// its own shares the program of its superclass. A class with callbacks adds a layer to the
    /// program of its superclass, and it does not copy the callbacks of the superclass. The index
    /// keeps the program of each class that a route reaches, and of each superclass with two or
    /// more subclasses.
    fn rails_program(&self, shared: &RailsShared, identity: &str) -> Arc<Program> {
        if let Some(found) = shared.programs.lock().get(identity) {
            return Arc::clone(found);
        }
        let mut chain = vec![identity.to_string()];
        let mut current = identity.to_string();
        let mut program: Option<Arc<Program>> = None;
        while let Some(Some(parent)) = shared.parents.get(&current) {
            if let Some(found) = shared.programs.lock().get(parent) {
                program = Some(Arc::clone(found));
                break;
            }
            chain.push(parent.clone());
            current = parent.clone();
        }
        let mut program =
            program.unwrap_or_else(|| Arc::new(Program::layer(None, Vec::new(), false)));
        for class in chain.iter().rev() {
            let mut own: Vec<Callback> = Vec::new();
            let mut opaque = false;
            for (position, index) in shared.classes.get(class).into_iter().flatten() {
                let class_callbacks = self.rails_callbacks(shared, *position, *index);
                own.extend(class_callbacks.list.iter().cloned());
                opaque |= class_callbacks.opaque;
            }
            opaque |= self.is_named_outside_the_class(class);
            if !own.is_empty() || opaque {
                program = Arc::new(Program::layer(Some(program), own, opaque));
            }
            shared
                .programs
                .lock()
                .insert(class.clone(), Arc::clone(&program));
        }
        program
    }

    /// Tells if a call outside the class body names the class and can change its callbacks: a
    /// callback call, `include`, `extend`, `prepend`, or `class_eval` with the constant of the class
    /// as its receiver. The constant may have fewer modules than the identity of the class, so omen
    /// compares each end of the identity.
    fn is_named_outside_the_class(&self, identity: &str) -> bool {
        let named = self.rails_shared().external.get_or_init(|| {
            let mut named = HashSet::new();
            for (position, file) in self.all.iter().enumerate() {
                if file.language != Language::Ruby {
                    continue;
                }
                let Some(parsed) = self.all_parsed.tree(position) else {
                    continue;
                };
                let mut stack = vec![parsed.root_node()];
                while let Some(node) = stack.pop() {
                    if node.kind() == "call" {
                        let method = node
                            .child_by_field_name("method")
                            .map_or("", |method| super::syntax::text(method, &parsed.source));
                        let receiver = node.child_by_field_name("receiver");
                        if let Some(receiver) = receiver.filter(|receiver| {
                            matches!(receiver.kind(), "constant" | "scope_resolution")
                        }) {
                            if is_callback_family(method) || CLASS_CHANGING_CALLS.contains(&method)
                            {
                                let text = super::syntax::text(receiver, &parsed.source);
                                named.insert(
                                    text.trim().trim_start_matches("::").replace("::", "."),
                                );
                            }
                        }
                    }
                    let mut cursor = node.walk();
                    stack.extend(node.named_children(&mut cursor));
                }
            }
            named
        });
        if named.is_empty() {
            return false;
        }
        let mut rest = identity;
        loop {
            if named.contains(rest) {
                return true;
            }
            match rest.split_once('.') {
                Some((_, tail)) => rest = tail,
                None => return false,
            }
        }
    }

    /// The callback calls in the body of the class, in source order. A call in a method, or in a
    /// nested class or module, is not a callback of this class. The index keeps the result.
    fn rails_callbacks(
        &self,
        shared: &RailsShared,
        position: usize,
        index: usize,
    ) -> Arc<ClassCallbacks> {
        if let Some(found) = shared.callbacks.lock().get(&(position, index)) {
            return Arc::clone(found);
        }
        let mut callbacks = ClassCallbacks::default();
        self.with_class_body(position, index, |reader, node| {
            reader.collect_callbacks(node, false, 0, &mut callbacks);
        });
        let callbacks = Arc::new(callbacks);
        shared
            .callbacks
            .lock()
            .insert((position, index), Arc::clone(&callbacks));
        callbacks
    }

    /// Calls `read` with a reader of the file and the syntax node of the class.
    fn with_class_body(
        &self,
        position: usize,
        index: usize,
        read: impl FnOnce(&Context<'_>, Node<'_>),
    ) {
        let class = &self.all[position].symbols[index];
        let Some(parsed) = self.all_parsed.tree(position) else {
            return;
        };
        let Some(mut node) = parsed
            .root_node()
            .descendant_for_byte_range(class.range.start_byte, class.range.end_byte)
        else {
            return;
        };
        while !matches!(node.kind(), "class" | "module") {
            let Some(parent) = node.parent() else {
                return;
            };
            node = parent;
        }
        let reader = Context::new(
            &parsed,
            &self.all[position],
            self.all,
            self.all_parsed,
            self.rules,
        );
        read(&reader, node);
    }

    /// The instance methods of the openings of a class, and which of them are public. The index
    /// keeps the result.
    fn rails_visibility(&self, shared: &RailsShared, identity: &str) -> Arc<Visibility> {
        if let Some(found) = shared.visibility.lock().get(identity) {
            return Arc::clone(found);
        }
        let mut openings: Vec<Opening> = Vec::new();
        for (position, index) in shared.classes.get(identity).into_iter().flatten() {
            self.with_class_body(*position, *index, |reader, node| {
                let mut opening = Opening {
                    position: *position,
                    ..Opening::default()
                };
                reader.read_visibility(node, &mut opening);
                openings.push(opening);
            });
        }
        let visibility = Arc::new(Visibility::resolve(openings));
        shared
            .visibility
            .lock()
            .insert(identity.to_string(), Arc::clone(&visibility));
        visibility
    }

    /// Reads the `def` statements of a class body, with `private`, `protected`, and `public`.
    /// A singleton method, and a method in `class << self`, is not an instance method. A
    /// definition in a block or a conditional, and a visibility call with an argument that is not
    /// a name or a `def`, make the visibility unreadable.
    fn read_visibility(&self, class: Node<'_>, out: &mut Opening) {
        let mut cursor = class.walk();
        let statements: Vec<Node<'_>> = match class.child_by_field_name("body") {
            Some(body) => {
                let mut body_cursor = body.walk();
                body.named_children(&mut body_cursor).collect()
            }
            None => class.named_children(&mut cursor).collect(),
        };
        let mut public = true;
        let define = |node: Node<'_>, public: bool, out: &mut Opening| {
            if let Some(name) = node.child_by_field_name("name") {
                out.defs.push(DefEvent {
                    name: self.text(name).to_string(),
                    start: name.start_byte(),
                    public,
                });
            }
        };
        for statement in statements {
            match statement.kind() {
                "method" => define(statement, public, out),
                "singleton_method" | "singleton_class" | "class" | "module" => {}
                "identifier" => match self.text(statement) {
                    "private" | "protected" => public = false,
                    "public" => public = true,
                    _ => {}
                },
                "call"
                    if statement.child_by_field_name("receiver").is_none()
                        && matches!(
                            statement
                                .child_by_field_name("method")
                                .map(|method| self.text(method)),
                            Some("private" | "protected" | "public")
                        ) =>
                {
                    let makes_public = statement
                        .child_by_field_name("method")
                        .is_some_and(|method| self.text(method) == "public");
                    let arguments = Self::arguments_of(statement);
                    if arguments.is_empty() {
                        public = makes_public;
                    }
                    for argument in arguments {
                        match argument.kind() {
                            "method" => define(argument, makes_public, out),
                            "simple_symbol" | "string" => match self.rails_literal(argument) {
                                Some(name) => out.ops.push(OpEvent {
                                    name,
                                    public: makes_public,
                                    at: statement.start_byte(),
                                }),
                                None => out.unreadable = true,
                            },
                            "array" => match self.rails_names(argument) {
                                Some(names) => {
                                    for name in names {
                                        out.ops.push(OpEvent {
                                            name,
                                            public: makes_public,
                                            at: statement.start_byte(),
                                        });
                                    }
                                }
                                None => out.unreadable = true,
                            },
                            _ => out.unreadable = true,
                        }
                    }
                }
                _ => {
                    if self.defines_methods(statement, 0) {
                        out.unreadable = true;
                    }
                }
            }
        }
    }

    /// Tells if the code has a `def`, or a call of `private`, `protected`, or `public`, outside a
    /// nested class or module.
    fn defines_methods(&self, node: Node<'_>, depth: usize) -> bool {
        if depth > MAX_READ_DEPTH {
            return true;
        }
        match node.kind() {
            "method" => return true,
            "class" | "module" | "singleton_class" => return false,
            "call" => {
                let method = node
                    .child_by_field_name("method")
                    .map_or("", |method| self.text(method));
                if node.child_by_field_name("receiver").is_none()
                    && matches!(method, "private" | "protected" | "public")
                {
                    return true;
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        children
            .into_iter()
            .any(|child| self.defines_methods(child, depth + 1))
    }

    /// Reads the callbacks in the children of the node. `conditional` tells that a branch that
    /// omen cannot evaluate holds the node. A callback in such a branch is conditional. A literal
    /// condition makes a branch dead or live, as it does for a route. A callback under any other
    /// kind of node makes the callbacks of the class opaque: omen reports no guard for it.
    fn collect_callbacks(
        &self,
        node: Node<'_>,
        conditional: bool,
        depth: usize,
        out: &mut ClassCallbacks,
    ) {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        for child in children {
            self.collect_callback_node(child, conditional, depth + 1, out);
        }
    }

    fn collect_callback_node(
        &self,
        node: Node<'_>,
        conditional: bool,
        depth: usize,
        out: &mut ClassCallbacks,
    ) {
        if depth > MAX_READ_DEPTH {
            out.opaque = true;
            return;
        }
        match node.kind() {
            "method" | "singleton_method" | "class" | "module" | "singleton_class" => {}
            "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
                let condition = node.child_by_field_name("condition");
                let literal = condition
                    .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
                let mut cursor = node.walk();
                let parts: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
                for part in parts {
                    if Some(part) == condition {
                        if subtree_has_callback(part, self.source, 0) {
                            out.opaque = true;
                        }
                        continue;
                    }
                    if Self::is_dead_branch(node, part) {
                        continue;
                    }
                    self.collect_callback_node(part, conditional || !literal, depth + 1, out);
                }
            }
            "case" if literal_case_children(node).is_some() => {
                for child in literal_case_children(node).unwrap_or_default() {
                    self.collect_callback_node(child, conditional, depth + 1, out);
                }
            }
            "case" | "case_match" | "do_block" | "block" | "lambda" => {
                self.collect_callbacks(node, true, depth, out)
            }
            "call" => match self.rails_callback(node) {
                Some(mut callback) if node.child_by_field_name("receiver").is_none() => {
                    callback.conditional |= conditional;
                    out.list.push(callback);
                }
                _ => {
                    if node.child_by_field_name("method").is_some_and(|method| {
                        is_callback_family(self.text(method))
                            || CLASS_CHANGING_CALLS.contains(&self.text(method))
                    }) {
                        out.opaque = true;
                    }
                    let mut cursor = node.walk();
                    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
                    for child in children {
                        if matches!(child.kind(), "do_block" | "block") {
                            self.collect_callback_node(child, conditional, depth + 1, out);
                        } else if subtree_has_callback(child, self.source, 0) {
                            out.opaque = true;
                        }
                    }
                }
            },
            "body_statement" | "then" | "else" | "when" | "block_body" => {
                self.collect_callbacks(node, conditional, depth, out)
            }
            _ => {
                if subtree_has_callback(node, self.source, 0) {
                    out.opaque = true;
                }
            }
        }
    }

    /// The names, the `only:` and `except:` lists, and the conditions of a callback call.
    /// A list that is not a literal makes the callback unreadable. See `Callback::applies_to`.
    fn rails_callback(&self, call: Node<'_>) -> Option<Callback> {
        let (skip, mut prepend) = match self.text(call.child_by_field_name("method")?) {
            "before_action" | "before_filter" => (false, false),
            "prepend_before_action" | "prepend_before_filter" => (false, true),
            "skip_before_action" | "skip_before_filter" => (true, false),
            _ => return None,
        };
        let arguments = Self::arguments_of(call);
        let options = self.rails_options(&arguments);
        let names = Self::rails_positional(&arguments)
            .into_iter()
            .map(|argument| self.rails_literal(argument))
            .collect::<Option<Vec<String>>>()?;
        let list = |key: &str| {
            options
                .as_ref()
                .and_then(|options| options.get(key))
                .map(|value| self.rails_names(value))
        };
        let (only, except) = (list("only"), list("except"));
        let mut prepend_unreadable = false;
        if !skip {
            if let Some(value) = options.as_ref().and_then(|options| options.get("prepend")) {
                match value.kind() {
                    "true" => prepend = true,
                    "false" => {}
                    _ => prepend_unreadable = true,
                }
            }
        }
        let unreadable = options.is_none()
            || prepend_unreadable
            || matches!(only, Some(None))
            || matches!(except, Some(None));
        let conditional = options
            .as_ref()
            .is_some_and(|options| options.get("if").is_some() || options.get("unless").is_some());
        let places = names
            .iter()
            .enumerate()
            .map(|(place, name)| (name.clone(), place))
            .collect();
        Some(Callback {
            skip,
            names,
            places,
            only: only.flatten().map(|list| list.into_iter().collect()),
            except: except.flatten().map(|list| list.into_iter().collect()),
            unreadable,
            conditional,
            prepend,
        })
    }
}

#[derive(Clone)]
struct Callback {
    skip: bool,
    names: Vec<String>,
    /// The place of the last time that each name is in `names`.
    places: HashMap<String, usize>,
    only: Option<HashSet<String>>,
    except: Option<HashSet<String>>,
    /// An `only:` or `except:` value that is not a literal list.
    unreadable: bool,
    /// An `if:` or `unless:` option, or a branch that omen cannot evaluate. A condition does not
    /// decide the action.
    conditional: bool,
    /// A `prepend: true` option, or a `prepend_before_action` call. The callback goes to the front.
    prepend: bool,
}

impl Callback {
    /// A callback never claims a guard that may not apply. A `before_action` call with a list
    /// that omen cannot read, or with `if:` or `unless:`, adds no guard. A `skip_before_action`
    /// call with a list that omen cannot read removes the guard for each action. A
    /// `skip_before_action` call with a condition removes the guard for the actions that it names.
    fn applies_to(&self, action: &str) -> bool {
        if self.unreadable {
            return self.skip;
        }
        (self.skip || !self.conditional)
            && self.only.as_ref().is_none_or(|only| only.contains(action))
            && self
                .except
                .as_ref()
                .is_none_or(|except| !except.contains(action))
    }
}

/// The place of a guard in the chain: the index of its callback, the place of its name in the
/// callback, and `prepend`.
type Slot = (usize, usize, bool);

/// What one layer of callbacks says about a name.
enum Decision {
    /// The layer decides: the guard has this slot, or the action has no guard with the name.
    Decided(Option<Slot>),
    /// A `skip_before_action` call that does not apply, or no callback with the name.
    Undecided,
}

/// The guards that an action gets when no `only:` or `except:` list names it.
struct DefaultGuards {
    slots: PersistentMap<String, Slot>,
}

impl DefaultGuards {
    fn names(&self) -> Vec<String> {
        ordered_names(&self.slots)
    }
}

/// The names of the guards in the order of Rails.
fn ordered_names(slots: &PersistentMap<String, Slot>) -> Vec<String> {
    let entries = slots.entries();
    let mut ordered: Vec<(&str, Slot)> = entries
        .iter()
        .map(|(name, slot)| (name.as_str(), *slot))
        .collect();
    order_slots(&mut ordered);
    ordered
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect()
}

/// The callbacks of one class: the calls that omen reads, and a flag that tells that a callback
/// call is in a place that omen cannot read. Then the class has no guard that omen can claim.
#[derive(Default)]
struct ClassCallbacks {
    list: Vec<Callback>,
    opaque: bool,
}

/// A program works out the guards of an action from its default guards when the callbacks that list
/// the action have fewer names than this, and callbacks. A program with more takes the guards from
/// the layer above it, so a layer costs its own callbacks.
const RESOLVE_FROM_DEFAULT_LIMIT: usize = if cfg!(test) { 6 } else { 512 };

/// A callback of the chain. A skip with an `except:` list, or with an `only:` list, is also in a
/// chain of the skips of the same kind, newest first.
struct Link {
    callback: Arc<Callback>,
    index: usize,
    older: Option<Arc<Link>>,
    /// For an action: the first call at this link or before it that applies to the action. A search
    /// that passes a call that does not apply keeps the result here, so the next search jumps.
    jump: Mutex<HashMap<String, Option<Arc<Link>>>>,
}

impl Link {
    fn new(callback: &Arc<Callback>, index: usize, older: Option<Arc<Link>>) -> Arc<Self> {
        Arc::new(Self {
            callback: Arc::clone(callback),
            index,
            older,
            jump: Mutex::new(HashMap::new()),
        })
    }

    /// The first call from this link back that applies to the action.
    fn applying(link: &Arc<Link>, action: &str) -> Option<Arc<Link>> {
        let mut passed: Vec<Arc<Link>> = Vec::new();
        let mut current = Some(Arc::clone(link));
        let found = loop {
            let Some(candidate) = current else {
                break None;
            };
            if let Some(known) = candidate.jump.lock().get(action) {
                break known.clone();
            }
            if candidate.callback.applies_to(action) {
                break Some(candidate);
            }
            current = candidate.older.clone();
            passed.push(candidate);
        };
        for link in passed {
            link.jump.lock().insert(action.to_string(), found.clone());
        }
        found
    }
}

/// The callbacks of a chain with a list that names an action.
#[derive(Clone, Default)]
struct Listing {
    /// The callbacks by their index.
    callbacks: PersistentMap<usize, Arc<Callback>>,
    /// The number of names in the callbacks, and of callbacks. A search for the guards of the action
    /// costs this much when it starts from the default guards.
    weight: usize,
}

/// What the callbacks with one name decide for each action, except the skips with an `only:` list.
#[derive(Clone, Default)]
struct NameState {
    /// The newest call that decides for each action: a call that is not a skip, and a skip with no
    /// list.
    always: Option<Arc<Link>>,
    /// The skips with an `except:` list and no `only:` list, newest first, after `always`.
    others: Option<Arc<Link>>,
}

impl NameState {
    /// The call that decides the guard for the action.
    fn deciding(&self, action: &str) -> Option<Arc<Link>> {
        let others = self
            .others
            .as_ref()
            .and_then(|start| Link::applying(start, action));
        match (self.always.as_ref(), others) {
            (Some(always), Some(other)) if other.index > always.index => Some(other),
            (Some(always), _) => Some(Arc::clone(always)),
            (None, other) => other,
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let mut next = self.older.take();
        while let Some(node) = next {
            match Arc::try_unwrap(node) {
                Ok(mut owned) => {
                    owned.jump.get_mut().clear();
                    next = owned.older.take();
                }
                Err(_) => break,
            }
        }
    }
}

/// The callbacks of a chain of classes in the order of Rails, ready to answer for an action.
///
/// A callback replaces the callbacks with its names, and it adds its names when it applies. Thus
/// the last callback of a name decides the guard of that name. Only the lists `only:` and
/// `except:` make the result depend on the action. The program keeps the guards for an action that
/// no list names. For another action, it works out again only the names of the callbacks that list
/// the action, and it starts from the guards for the other actions.
///
/// A program is a layer: the callbacks of one class, and the program of its superclass. A layer
/// shares the indexes of the superclass, and it does not copy them. The index of a callback has one
/// entry for each action of its lists, and not one for each name: a name is worked out only when a
/// route of the action asks for the guards.
struct Program {
    parent: Option<Arc<Program>>,
    /// The number of callbacks in the layers of the superclasses.
    offset: usize,
    callbacks: Vec<Arc<Callback>>,
    /// The callbacks of this layer by name, as indexes.
    by_name: HashMap<String, Vec<usize>>,
    /// What the callbacks of this layer and of the superclasses decide, by name.
    names: PersistentMap<String, NameState>,
    /// The callbacks of this layer and of the superclasses with a list that names the action, by
    /// the index of the callback.
    listed: PersistentMap<String, Listing>,
    /// The skips with an `only:` list that apply to the action, newest first, in this layer and in
    /// the superclasses.
    only_skips: PersistentMap<String, Arc<Link>>,
    /// A callback of this layer or of a superclass is in a place that omen cannot read. Then the
    /// layer builds no index.
    opaque: bool,
    default: std::sync::OnceLock<Arc<DefaultGuards>>,
    /// The guards of an action that a list names, as this layer has them, by action.
    resolved: Mutex<HashMap<String, PersistentMap<String, Slot>>>,
}

impl Drop for Program {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(layer) = next {
            match Arc::try_unwrap(layer) {
                Ok(mut owned) => next = owned.parent.take(),
                Err(_) => break,
            }
        }
    }
}

impl Program {
    fn layer(parent: Option<Arc<Program>>, callbacks: Vec<Callback>, opaque: bool) -> Self {
        let offset = parent
            .as_ref()
            .map_or(0, |parent| parent.offset + parent.callbacks.len());
        let callbacks: Vec<Arc<Callback>> = callbacks.into_iter().map(Arc::new).collect();
        let opaque = opaque || parent.as_ref().is_some_and(|parent| parent.opaque);
        let mut names = PersistentMap::default();
        let mut listed = PersistentMap::default();
        let mut only_skips = PersistentMap::default();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        if !opaque {
            if let Some(parent) = &parent {
                names = parent.names.clone();
                listed = parent.listed.clone();
                only_skips = parent.only_skips.clone();
            }
            for (index, callback) in callbacks.iter().enumerate() {
                let global = offset + index;
                let decides_always = !callback.skip
                    || callback.unreadable
                    || (callback.only.is_none() && callback.except.is_none());
                let is_only_skip = callback.skip && !callback.unreadable && callback.only.is_some();
                for name in &callback.names {
                    by_name.entry(name.clone()).or_default().push(index);
                    if is_only_skip {
                        continue;
                    }
                    let mut state: NameState = names.get(name).cloned().unwrap_or_default();
                    if decides_always {
                        state = NameState {
                            always: Some(Link::new(callback, global, None)),
                            others: None,
                        };
                    } else {
                        state.others = Some(Link::new(callback, global, state.others.take()));
                    }
                    names.insert(name.clone(), state);
                }
                if callback.unreadable {
                    continue;
                }
                let eligible = callback.only.iter().chain(&callback.except).flatten();
                let mut seen: HashSet<&String> = HashSet::new();
                for action in eligible {
                    if !seen.insert(action) {
                        continue;
                    }
                    let mut set: Listing = listed.get(action).cloned().unwrap_or_default();
                    set.callbacks.insert(global, Arc::clone(callback));
                    set.weight += callback.names.len() + 1;
                    listed.insert(action.clone(), set);
                }
                if is_only_skip {
                    for action in callback.only.iter().flatten() {
                        let excepted = callback
                            .except
                            .as_ref()
                            .is_some_and(|except| except.contains(action));
                        if !excepted {
                            let older = only_skips.get(action).cloned();
                            only_skips.insert(action.clone(), Link::new(callback, global, older));
                        }
                    }
                }
            }
        }
        Self {
            parent,
            offset,
            callbacks,
            by_name,
            names,
            listed,
            only_skips,
            opaque,
            default: std::sync::OnceLock::new(),
            resolved: Mutex::new(HashMap::new()),
        }
    }

    fn decide(&self, name: &str, action: &str) -> Decision {
        let Some(indexes) = self.by_name.get(name) else {
            return Decision::Undecided;
        };
        for index in indexes.iter().rev() {
            let callback = &self.callbacks[*index];
            let applies = callback.applies_to(action);
            if callback.skip {
                if applies {
                    return Decision::Decided(None);
                }
            } else {
                return Decision::Decided(applies.then(|| {
                    let place = callback.places.get(name).copied().unwrap_or(0);
                    (self.offset + *index, place, callback.prepend)
                }));
            }
        }
        Decision::Undecided
    }

    /// The slot of the guard with the name for the action, from the newest callback that decides.
    /// `skip` is the newest skip with an `only:` list that applies to the action and has the name.
    fn slot_for(&self, name: &str, action: &str, skip: Option<&Arc<Link>>) -> Option<Slot> {
        let state = self.names.get(&name.to_string());
        let deciding = state.and_then(|state| state.deciding(action));
        let link = match (deciding, skip) {
            (Some(link), Some(skip)) if skip.index > link.index => Arc::clone(skip),
            (Some(link), _) => link,
            (None, Some(skip)) => Arc::clone(skip),
            (None, None) => return None,
        };
        let callback = &link.callback;
        if callback.skip {
            return None;
        }
        callback.applies_to(action).then(|| {
            let place = callback.places.get(name).copied().unwrap_or(0);
            (link.index, place, callback.prepend)
        })
    }

    /// The default guards. The search goes up to the nearest layer that has them, and it applies
    /// the layers below it one time each. Each layer shares the structure of the layer above it.
    fn default_guards(&self) -> Arc<DefaultGuards> {
        if let Some(found) = self.default.get() {
            return Arc::clone(found);
        }
        let mut pending = vec![self];
        let mut slots: PersistentMap<String, Slot> = PersistentMap::default();
        let mut layer = self.parent.as_deref();
        while let Some(above) = layer {
            if let Some(found) = above.default.get() {
                slots = found.slots.clone();
                break;
            }
            pending.push(above);
            layer = above.parent.as_deref();
        }
        for layer in pending.iter().rev() {
            for name in layer.by_name.keys() {
                match layer.decide(name, "") {
                    Decision::Decided(Some(slot)) => slots.insert(name.clone(), slot),
                    Decision::Decided(None) => slots.remove(name),
                    Decision::Undecided => {}
                }
            }
            let _ = layer.default.set(Arc::new(DefaultGuards {
                slots: slots.clone(),
            }));
        }
        Arc::clone(self.default.get().expect("the last layer sets the default"))
    }

    /// The guards of an action. An action that no list names has the default guards. For another
    /// action, the layer has the guards of the layer above it, with the changes of its own
    /// callbacks. The program keeps the guards of each layer that asks, and the layers share the
    /// structure of the layers above them, so a chain of classes that repeat one listed action
    /// costs one change for each class. A layer whose chain has few callbacks for the action works
    /// the guards out from the default guards instead, so a long chain with one listed callback
    /// costs only that callback.
    fn guards_for(&self, action: &str) -> Vec<String> {
        if self.opaque {
            return Vec::new();
        }
        if self.listed.get(&action.to_string()).is_none() {
            return self.default_guards().names();
        }
        ordered_names(&self.resolved(action))
    }

    fn resolved(&self, action: &str) -> PersistentMap<String, Slot> {
        if let Some(found) = self.resolved.lock().get(action) {
            return found.clone();
        }
        let weight = self
            .listed
            .get(&action.to_string())
            .map_or(0, |listing| listing.weight);
        if weight < RESOLVE_FROM_DEFAULT_LIMIT {
            let slots = self.resolve_from_default(action);
            self.resolved
                .lock()
                .insert(action.to_string(), slots.clone());
            return slots;
        }
        let mut pending = vec![self];
        let mut layer = self.parent.as_deref();
        let mut slots = None;
        while let Some(above) = layer {
            if let Some(found) = above.resolved.lock().get(action) {
                slots = Some(found.clone());
                break;
            }
            pending.push(above);
            layer = above.parent.as_deref();
        }
        let mut slots = match (slots, pending.pop()) {
            (Some(slots), top) => {
                pending.extend(top);
                slots
            }
            (None, Some(top)) => {
                let slots = top.resolve_from_default(action);
                top.resolved
                    .lock()
                    .insert(action.to_string(), slots.clone());
                slots
            }
            (None, None) => PersistentMap::default(),
        };
        for layer in pending.iter().rev() {
            for name in layer.by_name.keys() {
                match layer.decide(name, action) {
                    Decision::Decided(Some(slot)) => slots.insert(name.clone(), slot),
                    Decision::Decided(None) => slots.remove(name),
                    Decision::Undecided => {}
                }
            }
            layer
                .resolved
                .lock()
                .insert(action.to_string(), slots.clone());
        }
        slots
    }

    /// The guards of an action that a list names: the default guards, and the names of the callbacks
    /// that name the action, worked out again.
    fn resolve_from_default(&self, action: &str) -> PersistentMap<String, Slot> {
        let mut slots = self.default_guards().slots.clone();
        let Some(listing) = self.listed.get(&action.to_string()) else {
            return slots;
        };
        let mut skips: HashMap<&str, Arc<Link>> = HashMap::new();
        let mut link = self.only_skips.get(&action.to_string());
        while let Some(current) = link {
            for name in &current.callback.names {
                skips
                    .entry(name.as_str())
                    .or_insert_with(|| Arc::clone(current));
            }
            link = current.older.as_ref();
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let entries = listing.callbacks.entries();
        for (_, callback) in &entries {
            for name in &callback.names {
                if !seen.insert(name.as_str()) {
                    continue;
                }
                match self.slot_for(name, action, skips.get(name.as_str())) {
                    Some(slot) => slots.insert(name.clone(), slot),
                    None => slots.remove(name),
                }
            }
        }
        slots
    }
}

/// Puts the guards in the order of Rails: the prepended guards first, the last one first, and
/// then the other guards in the order of their callbacks.
fn order_slots(slots: &mut [(&str, Slot)]) {
    slots.sort_by(|(_, left), (_, right)| match (left.2, right.2) {
        (true, true) => (right.0, right.1).cmp(&(left.0, left.1)),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => (left.0, left.1).cmp(&(right.0, right.1)),
    });
}

/// A class identity and an action name.
type ActionKey = (String, String);

/// The symbol and the guards of an action.
pub(super) type Handler = (Option<String>, Vec<String>);

/// A symbol of the index: the position of the file, and the position in the symbols of the file.
type Placed = (usize, usize);

/// The class that a short name gives. A name that belongs to two classes is ambiguous.
enum UniqueClass {
    One(String),
    Many,
}

/// What the chain of a class says about the action names: the reach of a name that a class decides,
/// and the reach of any other name.
struct ActionState {
    floor: Reach,
    map: PersistentMap<String, Reach>,
}

/// Whether the nearest class that decides lets Rails run the action.
#[derive(Clone, Copy)]
enum Reach {
    Public,
    Hidden,
}

/// Whether Rails runs an action.
enum Action {
    /// The action runs. The symbol is the public method of the class, when there is one.
    Public(Option<String>),
    /// The method is not public, or omen cannot read its visibility.
    Hidden,
}

/// A `def` in a class body: the name, the start of the name, and the visibility when the `def`
/// runs.
struct DefEvent {
    name: String,
    start: usize,
    public: bool,
}

/// A call `private :name`, `protected :name`, or `public :name`.
struct OpEvent {
    name: String,
    public: bool,
    at: usize,
}

/// The visibility events of one opening of a class.
#[derive(Default)]
struct Opening {
    position: usize,
    defs: Vec<DefEvent>,
    ops: Vec<OpEvent>,
    unreadable: bool,
}

/// The instance methods of the openings of a class: the names that the class defines, the names
/// that are public, and the public definitions by the position of the file and the start of the
/// name. `unreadable` tells that omen cannot read the visibility.
#[derive(Default)]
struct Visibility {
    defines: HashSet<String>,
    public_names: HashSet<String>,
    public: HashSet<(usize, usize)>,
    /// A `private`, `protected`, or `public` call for a name that no opening defines: it changes the
    /// visibility of a method that the class inherits. The value tells if the method is public.
    overrides: HashMap<String, bool>,
    unreadable: bool,
}

impl Visibility {
    /// Applies the events of the openings. In one file, the events run in the order of the source.
    /// The order of the files is not known. Thus a name that two files define, and a call that
    /// names a method of another file, make the visibility unreadable.
    fn resolve(openings: Vec<Opening>) -> Self {
        let mut found = Self {
            unreadable: openings.iter().any(|opening| opening.unreadable),
            ..Self::default()
        };
        let mut files: std::collections::BTreeMap<usize, Vec<&Opening>> =
            std::collections::BTreeMap::new();
        for opening in &openings {
            files.entry(opening.position).or_default().push(opening);
        }
        let mut definers: HashMap<&str, HashSet<usize>> = HashMap::new();
        let mut foreign: Vec<(usize, &str, bool)> = Vec::new();
        for (file, group) in &files {
            enum Event<'e> {
                Def(&'e DefEvent),
                Op(&'e OpEvent),
            }
            let mut events: Vec<(usize, Event<'_>)> = Vec::new();
            for opening in group {
                events.extend(opening.defs.iter().map(|def| (def.start, Event::Def(def))));
                events.extend(opening.ops.iter().map(|op| (op.at, Event::Op(op))));
            }
            events.sort_by_key(|(at, _)| *at);
            let mut state: HashMap<&str, bool> = HashMap::new();
            let mut defs: Vec<&DefEvent> = Vec::new();
            for (_, event) in events {
                match event {
                    Event::Def(def) => {
                        state.insert(def.name.as_str(), def.public);
                        defs.push(def);
                        definers.entry(def.name.as_str()).or_default().insert(*file);
                    }
                    Event::Op(op) => {
                        if state.contains_key(op.name.as_str()) {
                            state.insert(op.name.as_str(), op.public);
                        } else {
                            foreign.push((*file, op.name.as_str(), op.public));
                        }
                    }
                }
            }
            for def in defs {
                found.defines.insert(def.name.clone());
                if state.get(def.name.as_str()).copied().unwrap_or(false) {
                    found.public.insert((*file, def.start));
                    found.public_names.insert(def.name.clone());
                }
            }
        }
        if definers.values().any(|files| files.len() > 1)
            || foreign.iter().any(|(file, name, _)| {
                definers
                    .get(name)
                    .is_some_and(|files| files.iter().any(|other| other != file))
            })
        {
            found.unreadable = true;
        }
        let mut override_files: HashMap<&str, usize> = HashMap::new();
        for (file, name, public) in foreign {
            if override_files
                .insert(name, file)
                .is_some_and(|other| other != file)
            {
                found.unreadable = true;
            }
            found.overrides.insert(name.to_string(), public);
        }
        found
    }
}

/// What the files of the inflections say.
#[derive(Default)]
pub(super) struct Inflections {
    /// The acronyms, by the lower case word.
    acronyms: HashMap<String, String>,
    /// An `acronym` call that omen cannot read, or a `clear` call, or an inflection call in a
    /// branch that omen cannot evaluate. The name of a controller is not known.
    unreadable_acronym: bool,
    /// A call that can change the singular or the plural of a word.
    changes_singulars: bool,
}

/// The scope outside every block: the root path.
pub(super) fn root_scope() -> Rc<Scope> {
    Rc::new(Scope {
        prefix: Some(Prefix::root()),
        ..Scope::default()
    })
}

/// The deepest syntax node that omen reads with a recursive search of the whole tree. A deeper
/// tree makes the result unknown, so the search does not overflow the stack of the thread.
const MAX_READ_DEPTH: usize = 1_000;

/// The value of `config.api_only` for a route set.
#[derive(Clone, Copy, Default, PartialEq)]
enum ApiOnly {
    #[default]
    Off,
    On,
    Unknown,
}

/// The text of a file, and whether the file is in `config`, for the search for `api_only`.
struct ApiScan<'s> {
    source: &'s [u8],
    in_config: bool,
}

/// The kind of a class of the repository, for `config`.
enum ClassKind {
    Application,
    Engine,
    Other,
}

/// The assignments `x.api_only = value` of the application and of each engine. A value is `None`
/// when it is not `true` or `false`, or when omen cannot evaluate the branch that holds it.
#[derive(Default)]
struct ApiOnlySets {
    application: Vec<Option<bool>>,
    engines: HashMap<String, Vec<Option<bool>>>,
    /// An assignment that omen cannot attribute to one route set.
    unattributed: bool,
}

impl ApiOnlySets {
    fn value(&self, values: &[Option<bool>]) -> ApiOnly {
        if self.unattributed
            || self.engines.values().any(|assigned| !assigned.is_empty())
            || values.iter().any(Option::is_none)
            || (values.contains(&Some(true)) && values.contains(&Some(false)))
        {
            ApiOnly::Unknown
        } else if values.contains(&Some(true)) {
            ApiOnly::On
        } else {
            ApiOnly::Off
        }
    }

    fn for_application(&self) -> ApiOnly {
        self.value(&self.application)
    }

    fn for_engine(&self, identity: &str) -> ApiOnly {
        self.value(self.engines.get(identity).map_or(&[], Vec::as_slice))
    }
}

/// Tells if the text is a constant path, such as `Blog::Engine`.
fn is_constant_path(text: &str) -> bool {
    text.split("::").all(|part| {
        part.chars()
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Tells if the text is the constant `ActiveSupport::Inflector`, with or without a leading `::`.
fn is_inflector_constant(text: &str) -> bool {
    text.trim().trim_start_matches("::") == "ActiveSupport::Inflector"
}

/// Tells if the node is a call of `ActiveSupport::Inflector.inflections`.
fn is_inflections_call(node: Node<'_>, source: &[u8]) -> bool {
    node.kind() == "call"
        && node
            .child_by_field_name("method")
            .is_some_and(|method| super::syntax::text(method, source) == "inflections")
        && node
            .child_by_field_name("receiver")
            .is_some_and(|receiver| is_inflector_constant(super::syntax::text(receiver, source)))
}

/// The locale of a call of `inflections`.
enum Locale {
    /// No argument, `:en`, or `"en"`: the locale that the names of the classes use.
    English,
    /// Another symbol or string.
    Other,
    /// An argument that omen cannot read.
    Unknown,
}

fn inflections_locale(call: Node<'_>, source: &[u8]) -> Locale {
    let mut cursor = call.walk();
    let arguments: Vec<Node<'_>> = call
        .child_by_field_name("arguments")
        .map(|list| list.named_children(&mut cursor).collect())
        .unwrap_or_default();
    match arguments.as_slice() {
        [] => Locale::English,
        [one] => {
            let name = match one.kind() {
                "simple_symbol" => Some(
                    super::syntax::text(*one, source)
                        .trim_start_matches(':')
                        .to_string(),
                ),
                "string" => evaluate_string(*one, source),
                _ => None,
            };
            match name.as_deref() {
                Some("en") => Locale::English,
                Some(_) => Locale::Other,
                None => Locale::Unknown,
            }
        }
        _ => Locale::Unknown,
    }
}

/// Tells if the node is the object of the inflections: a block parameter of
/// `ActiveSupport::Inflector.inflections`, or the call `ActiveSupport::Inflector.inflections`.
fn is_inflection_object(node: Node<'_>, source: &[u8], params: &[String]) -> bool {
    if node.kind() == "identifier" {
        let name = super::syntax::text(node, source);
        return params.iter().any(|param| param == name);
    }
    node.kind() == "call"
        && node
            .child_by_field_name("method")
            .is_some_and(|method| super::syntax::text(method, source) == "inflections")
        && node
            .child_by_field_name("receiver")
            .is_some_and(|receiver| is_inflector_constant(super::syntax::text(receiver, source)))
}

/// The names of the parameters of a block of the call.
fn block_parameter_names(call: Node<'_>, source: &[u8]) -> Vec<String> {
    let Some(block) = call.child_by_field_name("block") else {
        return Vec::new();
    };
    let Some(parameters) = block.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter(|parameter| parameter.kind() == "identifier")
        .map(|parameter| super::syntax::text(parameter, source).to_string())
        .collect()
}

/// The calls that change the callbacks of a class without a callback name: they add the callbacks
/// of a module, or run code in the class.
const CLASS_CHANGING_CALLS: [&str; 7] = [
    "include",
    "extend",
    "prepend",
    "class_eval",
    "class_exec",
    "module_eval",
    "instance_eval",
];

/// Tells if a method name belongs to the callback family of Rails: a call of it changes the guards
/// of a class, and omen reads only a part of the forms.
fn is_callback_family(method: &str) -> bool {
    method.contains("before_action")
        || method.contains("before_filter")
        || matches!(
            method,
            "skip_callback"
                | "set_callback"
                | "reset_callbacks"
                | "skip_filter"
                | "skip_action_callback"
        )
}

/// Tells if the code has a call of the callback family, with any receiver.
fn subtree_has_callback(node: Node<'_>, source: &[u8], depth: usize) -> bool {
    if depth > MAX_READ_DEPTH {
        return true;
    }
    if node.kind() == "call"
        && node
            .child_by_field_name("method")
            .is_some_and(|method| is_callback_family(super::syntax::text(method, source)))
    {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| subtree_has_callback(child, source, depth + 1))
}

/// Tells if the code assigns `api_only` of any receiver.
fn subtree_assigns_api_only(node: Node<'_>, source: &[u8], depth: usize) -> bool {
    if depth > MAX_READ_DEPTH {
        return true;
    }
    if matches!(node.kind(), "assignment" | "operator_assignment")
        && node.child_by_field_name("left").is_some_and(|left| {
            left.kind() == "call"
                && left
                    .child_by_field_name("method")
                    .is_some_and(|method| super::syntax::text(method, source) == "api_only")
        })
    {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| subtree_assigns_api_only(child, source, depth + 1))
}

const INFLECTION_METHODS: [&str; 7] = [
    "acronym",
    "irregular",
    "uncountable",
    "human",
    "clear",
    "singular",
    "plural",
];

/// Tells if the code has a call of a method that changes the inflections, with any receiver.
fn subtree_has_inflection_call(node: Node<'_>, source: &[u8], depth: usize) -> bool {
    if depth > MAX_READ_DEPTH {
        return true;
    }
    if node.kind() == "constant" && super::syntax::text(node, source) == "Inflections" {
        return true;
    }
    if node.kind() == "call"
        && node
            .child_by_field_name("method")
            .is_some_and(|method| INFLECTION_METHODS.contains(&super::syntax::text(method, source)))
    {
        return true;
    }
    if is_inflection_object(node, source, &[]) {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| subtree_has_inflection_call(child, source, depth + 1))
}

/// Tells if the call is `<name>.<method>(<literals>)` for a name of `params`, a method that changes
/// the inflections, no block, and arguments that are strings, symbols, regular expressions, or
/// lists of them, without interpolation.
fn modelled_inflection_call(node: Node<'_>, source: &[u8], params: &[String]) -> bool {
    fn literal(node: Node<'_>, source: &[u8], depth: usize) -> bool {
        if depth > MAX_READ_DEPTH {
            return false;
        }
        match node.kind() {
            "string" => evaluate_string(node, source).is_some(),
            "simple_symbol" => true,
            "regex" => {
                let mut cursor = node.walk();
                let plain = node
                    .named_children(&mut cursor)
                    .all(|child| child.kind() != "interpolation");
                plain
            }
            "array" => {
                let mut cursor = node.walk();
                let all = node
                    .named_children(&mut cursor)
                    .all(|child| literal(child, source, depth + 1));
                all
            }
            _ => false,
        }
    }
    let receiver_ok = node
        .child_by_field_name("receiver")
        .is_some_and(|receiver| {
            receiver.kind() == "identifier"
                && params
                    .iter()
                    .any(|name| name == super::syntax::text(receiver, source))
        });
    let method_ok = node
        .child_by_field_name("method")
        .is_some_and(|method| INFLECTION_METHODS.contains(&super::syntax::text(method, source)));
    let arguments_ok = node
        .child_by_field_name("arguments")
        .is_none_or(|arguments| {
            let mut cursor = arguments.walk();
            let all = arguments
                .named_children(&mut cursor)
                .all(|argument| literal(argument, source, 0));
            all
        });
    receiver_ok && method_ok && arguments_ok && node.child_by_field_name("block").is_none()
}

/// Tells if each statement of the block of `inflections` is a modelled call, a comment, or a
/// condition or a `case` that omen can evaluate, with branches that follow the same rule. A dead
/// branch is not read. Any other statement, such as a call of `binding`, `eval`, or `send`, a
/// definition, or a condition that omen cannot evaluate, makes the block not readable. The `own`
/// node is the parameter list of the block.
fn inflection_statements_ok(
    node: Node<'_>,
    source: &[u8],
    params: &[String],
    own: Option<Node<'_>>,
    depth: usize,
) -> bool {
    if depth > MAX_READ_DEPTH {
        return false;
    }
    if Some(node) == own {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    let all = |parts: Vec<Node<'_>>| {
        parts
            .into_iter()
            .all(|part| inflection_statements_ok(part, source, params, own, depth + 1))
    };
    match node.kind() {
        "comment" | "pattern" | "true" | "false" | "nil" => true,
        "do_block" | "block" | "body_statement" | "block_body" | "then" | "else" => all(children),
        "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" => {
            let condition = node.child_by_field_name("condition");
            if !condition
                .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"))
            {
                return false;
            }
            all(children
                .into_iter()
                .filter(|part| Some(*part) != condition && !Context::is_dead_branch(node, *part))
                .collect())
        }
        "case" => match literal_case_children(node) {
            Some(alive) => all(alive),
            None => false,
        },
        "when" => all(children),
        "call" => modelled_inflection_call(node, source, params),
        _ => false,
    }
}

/// Reads the inflection calls in the code: `acronym` with a string, and the calls that change
/// a singular or a plural form. Only a call on the object of the inflections counts: the
/// parameter of the block of `ActiveSupport::Inflector.inflections`, or the result of that call.
/// The search goes through the statement lists, the literal conditions that it can evaluate, and
/// the blocks. A call under any other kind of node, a call in a branch that omen cannot
/// evaluate, and a block parameter that the code binds again make the inflections unreadable.
fn read_inflection_calls(
    node: Node<'_>,
    source: &[u8],
    depth: usize,
    conditional: bool,
    params: &[String],
    out: &mut Inflections,
) {
    let unreadable = |out: &mut Inflections| {
        out.unreadable_acronym = true;
        out.changes_singulars = true;
    };
    if depth > MAX_READ_DEPTH {
        unreadable(out);
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    match node.kind() {
        "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
            let condition = node.child_by_field_name("condition");
            let literal = condition
                .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
            for part in children {
                if Some(part) == condition {
                    if subtree_has_inflection_call(part, source, 0) {
                        unreadable(out);
                    }
                    continue;
                }
                if Context::is_dead_branch(node, part) {
                    continue;
                }
                read_inflection_calls(
                    part,
                    source,
                    depth + 1,
                    conditional || !literal,
                    params,
                    out,
                );
            }
        }
        "case" if literal_case_children(node).is_some() => {
            for child in literal_case_children(node).unwrap_or_default() {
                read_inflection_calls(child, source, depth + 1, conditional, params, out);
            }
        }
        "case" | "case_match" | "lambda" | "method" | "singleton_method" | "for" | "while"
        | "until" | "while_modifier" | "until_modifier" => {
            for child in children {
                read_inflection_calls(child, source, depth + 1, true, params, out);
            }
        }
        "program" | "body_statement" | "then" | "else" | "block_body" | "do_block" | "block"
        | "class" | "module" => {
            for child in children {
                read_inflection_calls(child, source, depth + 1, conditional, params, out);
            }
        }
        "call" => {
            let method = node
                .child_by_field_name("method")
                .map_or("", |method| super::syntax::text(method, source));
            let receiver = node.child_by_field_name("receiver");
            let opens_inflections = method == "inflections"
                && receiver.is_some_and(|receiver| {
                    is_inflector_constant(super::syntax::text(receiver, source))
                });
            let governing = if opens_inflections {
                Some(node)
            } else {
                receiver.filter(|receiver| is_inflections_call(*receiver, source))
            };
            match governing.map(|call| inflections_locale(call, source)) {
                Some(Locale::Other) => return,
                Some(Locale::Unknown) => {
                    unreadable(out);
                    return;
                }
                _ => {}
            }
            let mut block_params: Vec<String> = params.to_vec();
            if opens_inflections {
                let names = block_parameter_names(node, source);
                let own = node
                    .child_by_field_name("block")
                    .and_then(|block| block.child_by_field_name("parameters"));
                let readable = match node.child_by_field_name("block") {
                    Some(block) => inflection_statements_ok(block, source, &names, own, 0),
                    None => node.parent().is_some_and(|parent| {
                        parent.kind() == "call"
                            && parent.child_by_field_name("receiver") == Some(node)
                            && parent.child_by_field_name("method").is_some_and(|method| {
                                INFLECTION_METHODS.contains(&super::syntax::text(method, source))
                            })
                    }),
                };
                if readable {
                    block_params.extend(names);
                } else {
                    unreadable(out);
                }
            }
            let on_object =
                receiver.is_some_and(|receiver| is_inflection_object(receiver, source, params));
            let affects = on_object && INFLECTION_METHODS.contains(&method);
            if affects && conditional {
                unreadable(out);
            } else if affects {
                match method {
                    "acronym" => {
                        let mut arguments_cursor = node.walk();
                        let arguments: Vec<Node<'_>> = node
                            .child_by_field_name("arguments")
                            .map(|list| list.named_children(&mut arguments_cursor).collect())
                            .unwrap_or_default();
                        let words: Vec<String> = arguments
                            .iter()
                            .filter_map(|argument| {
                                (argument.kind() == "string")
                                    .then(|| evaluate_string(*argument, source))
                                    .flatten()
                            })
                            .collect();
                        if words.is_empty() || words.len() != arguments.len() {
                            out.unreadable_acronym = true;
                        }
                        for word in words {
                            out.acronyms.insert(word.to_lowercase(), word);
                        }
                    }
                    "clear" => unreadable(out),
                    _ => out.changes_singulars = true,
                }
            }
            for child in children {
                if matches!(child.kind(), "do_block" | "block") {
                    read_inflection_calls(
                        child,
                        source,
                        depth + 1,
                        conditional || !opens_inflections,
                        &block_params,
                        out,
                    );
                } else if affects {
                    if !is_inflection_object(child, source, params)
                        && subtree_has_inflection_call(child, source, 0)
                    {
                        unreadable(out);
                    }
                } else if subtree_has_inflection_call(child, source, 0)
                    && !is_inflection_object(child, source, params)
                {
                    unreadable(out);
                }
            }
        }
        _ => {
            if subtree_has_inflection_call(node, source, 0) {
                unreadable(out);
            }
        }
    }
}

/// The controller classes by their chain of modules, and their methods by the class that holds
/// them. The key of a method has the chain of its class. The index also keeps the superclass of
/// each class, and each result that it computes. Omen builds the index one time for each
/// analysis, and each routes file uses it.
pub(super) struct RailsShared {
    classes: HashMap<String, Vec<Placed>>,
    methods: HashMap<String, HashMap<String, Vec<Placed>>>,
    /// The superclass of each class identity. A class in a loop of superclasses has none.
    parents: HashMap<String, Option<String>>,
    inflections: std::sync::OnceLock<Inflections>,
    api_only: std::sync::OnceLock<ApiOnlySets>,
    external: std::sync::OnceLock<HashSet<String>>,
    callbacks: Mutex<HashMap<Placed, Arc<ClassCallbacks>>>,
    visibility: Mutex<HashMap<String, Arc<Visibility>>>,
    programs: Mutex<HashMap<String, Arc<Program>>>,
    handled: Mutex<HashMap<ActionKey, Handler>>,
    states: Mutex<HashMap<String, Arc<ActionState>>>,
}

impl RailsShared {
    pub(super) fn build(all: &[FileSymbols]) -> Self {
        let mut classes: HashMap<String, Vec<Placed>> = HashMap::new();
        let mut superclasses: HashMap<&str, UniqueClass> = HashMap::new();
        let mut methods: HashMap<String, HashMap<String, Vec<Placed>>> = HashMap::new();
        for (position, file) in all.iter().enumerate() {
            if file.language != Language::Ruby {
                continue;
            }
            for (index, symbol) in file.symbols.iter().enumerate() {
                if symbol.kind.is_type() {
                    let identity = qualified_name(file, symbol);
                    let unique = match superclasses.get(symbol.name.as_str()) {
                        None => UniqueClass::One(identity.clone()),
                        Some(UniqueClass::One(existing)) if *existing == identity => {
                            UniqueClass::One(identity.clone())
                        }
                        Some(_) => UniqueClass::Many,
                    };
                    superclasses.insert(symbol.name.as_str(), unique);
                    classes.entry(identity).or_default().push((position, index));
                } else if symbol.is_callable() {
                    let qualified = qualified_name(file, symbol);
                    if let Some(owner) = qualified.strip_suffix(&format!(".{}", symbol.name)) {
                        methods
                            .entry(owner.to_string())
                            .or_default()
                            .entry(symbol.name.clone())
                            .or_default()
                            .push((position, index));
                    }
                }
            }
        }
        for group in classes.values_mut() {
            group.sort_by_key(|(position, index)| {
                (*position, all[*position].symbols[*index].range.start_byte)
            });
        }
        let parents = resolve_parents(all, &classes, &superclasses);
        Self {
            classes,
            methods,
            parents,
            inflections: std::sync::OnceLock::new(),
            api_only: std::sync::OnceLock::new(),
            external: std::sync::OnceLock::new(),
            callbacks: Mutex::new(HashMap::new()),
            visibility: Mutex::new(HashMap::new()),
            programs: Mutex::new(HashMap::new()),
            handled: Mutex::new(HashMap::new()),
            states: Mutex::new(HashMap::new()),
        }
    }
}

/// The superclass identity of each class. A
/// qualified superclass name, a name of two classes, and a class in a loop of superclasses give
/// no superclass. A class that leads into a loop keeps its superclass. Omen visits each class one
/// time.
fn resolve_parents(
    all: &[FileSymbols],
    classes: &HashMap<String, Vec<Placed>>,
    superclasses: &HashMap<&str, UniqueClass>,
) -> HashMap<String, Option<String>> {
    let raw: HashMap<&str, Option<&str>> = classes
        .iter()
        .map(|(identity, group)| {
            let parent = group
                .iter()
                .find_map(|(position, index)| all[*position].symbols[*index].supertypes.first())
                .filter(|parent| !parent.contains("::"))
                .and_then(|parent| match superclasses.get(parent.as_str()) {
                    Some(UniqueClass::One(found)) => Some(found.as_str()),
                    _ => None,
                });
            (identity.as_str(), parent)
        })
        .collect();
    let mut parents: HashMap<String, Option<String>> = HashMap::new();
    for start in classes.keys() {
        if parents.contains_key(start) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut position: HashMap<&str, usize> = HashMap::new();
        let mut cycle_start = None;
        let mut current = Some(start.as_str());
        while let Some(identity) = current {
            if parents.contains_key(identity) {
                break;
            }
            if let Some(found) = position.get(identity) {
                cycle_start = Some(*found);
                break;
            }
            position.insert(identity, path.len());
            path.push(identity);
            current = raw.get(identity).copied().flatten();
        }
        for (step, identity) in path.iter().enumerate() {
            let in_cycle = cycle_start.is_some_and(|first| step >= first);
            let parent = if in_cycle {
                None
            } else {
                raw.get(identity).copied().flatten().map(str::to_string)
            };
            parents.insert((*identity).to_string(), parent);
        }
    }
    parents
}

/// The chain of containers and the name of a class, joined with `.`. The key of the symbol has
/// the chain, so the key tells `Admin::V1::UsersController` from `Other::V1::UsersController`.
fn qualified_name(file: &FileSymbols, symbol: &Symbol) -> String {
    let key = symbol.key.to_string();
    let chain = key.strip_prefix(&format!("{}#", file.file)).unwrap_or(&key);
    let chain = chain.split('~').next().unwrap_or(chain);
    chain.replace("::", ".")
}

/// The modules of the blocks around a route, as a chain. A new module does not copy the earlier
/// modules, so each block takes a time in proportion to its own modules. The list is built at a
/// route.
#[derive(Clone, Default)]
pub(super) struct Modules(Option<Rc<ModuleNode>>);

struct ModuleNode {
    parent: Option<Rc<ModuleNode>>,
    name: String,
    id: usize,
}

impl Drop for ModuleNode {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(mut owned) => next = owned.parent.take(),
                Err(_) => break,
            }
        }
    }
}

impl Modules {
    fn extend(&self, names: Vec<String>) -> Self {
        let mut chain = self.0.clone();
        for name in names {
            chain = Some(Rc::new(ModuleNode {
                parent: chain,
                name,
                id: next_id(),
            }));
        }
        Self(chain)
    }

    /// A number that stands for the chain in a key. The empty chain has 0.
    fn id(&self) -> usize {
        self.0.as_ref().map_or(0, |node| node.id)
    }

    fn to_vec(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut node = self.0.as_deref();
        while let Some(current) = node {
            names.push(current.name.clone());
            node = current.parent.as_deref();
        }
        names.reverse();
        names
    }
}

/// The parts of a module path. An empty part stays: Rails builds the name of the controller class
/// from it, and the class does not exist.
fn split_module(path: &str) -> Vec<String> {
    path.split('/').map(str::to_string).collect()
}

struct ResourceDefinition {
    names: Vec<String>,
    only: Option<Vec<String>>,
    except: Vec<String>,
    path: Option<String>,
    controller: Option<String>,
    /// The `as:` option. It replaces the name of the resource where Rails builds the nested
    /// parameter.
    as_name: Option<String>,
    modules: Modules,
}

impl ResourceDefinition {
    /// Tells if `actions` of Rails has the action: the `only:` list, or the default actions, and
    /// then without the `except:` list. An API application has no `new` and no `edit` by default.
    fn allows(&self, action: &str, api_only: bool) -> bool {
        let available = match &self.only {
            Some(only) => only.iter().any(|name| name == action),
            None => !(api_only && matches!(action, "new" | "edit")),
        };
        available && !self.except.iter().any(|name| name == action)
    }

    /// The paths of the resource, as `Resource` and `SingletonResource` of Rails give them.
    fn context(
        &self,
        singular: bool,
        name: &str,
        singular_name: Option<String>,
    ) -> ResourceContext {
        let segment = self.path.clone().unwrap_or_else(|| name.to_string());
        if singular {
            return ResourceContext {
                singular,
                collection: segment.clone(),
                member: segment.clone(),
                nested: Some(segment),
            };
        }
        ResourceContext {
            singular,
            member: format!("{segment}/:id"),
            nested: singular_name.map(|singular_name| format!("{segment}/:{singular_name}_id")),
            collection: segment,
        }
    }

    /// The controller path of a resource. A singular resource uses the plural of its name. If the
    /// application can change the plural, `plural` is `None`, and the controller is not known.
    fn controller_for(&self, singular: bool, name: &str, plural: Option<String>) -> Option<String> {
        if let Some(controller) = &self.controller {
            return Some(controller.clone());
        }
        if singular {
            plural
        } else {
            Some(name.to_string())
        }
    }
}
