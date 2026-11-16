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
    prefix: Option<String>,
    modules: Vec<String>,
    controller: Option<String>,
    /// The resource, when the route is directly in a `resources` or `resource` block.
    resource: Option<ResourceContext>,
    /// The route is in a `member` or `collection` block, or `on:` puts it there. A canonical action
    /// name then adds no path segment.
    method_scope: bool,
    /// The route is in a `draw` block of the routes of the application or of an engine.
    draw: bool,
}

impl Scope {
    /// The path of a route in a block of `resources`: the path of the blocks, and the nested path of
    /// the resource.
    fn nested_prefix(&self) -> Option<String> {
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
            .find(|option| !option.string_key && option.key == key)
            .map(|option| option.value)
    }
}

struct Target {
    controller: Option<String>,
    action: Option<String>,
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
fn route_path(base: &str, part: &str) -> String {
    normalize_path(&escape_route_path(&format!("{base}/{part}")))
}

/// The path of a part of a path after a prefix, as `merge_path_scope` of Rails does. A prefix that
/// is not known gives no path.
fn join(prefix: &Option<String>, part: &str) -> Option<String> {
    join_str(prefix.as_ref()?, part)
}

fn join_str(prefix: &str, part: &str) -> Option<String> {
    Some(normalize_path(&format!("{prefix}/{part}")))
}

impl<'a> Context<'a> {
    pub(super) fn ruby(&self, node: Node<'_>, out: &mut Vec<Draft>) {
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
        let Some(scope) = self.rails_scope(node) else {
            return;
        };
        match name {
            "resources" | "resource" => self.rails_resources(node, name == "resource", &scope, out),
            _ => self.rails_plain_route(node, name, &scope, out),
        }
    }

    /// The text of a string without interpolation, or of a symbol.
    fn rails_literal(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "string" => {
                let mut cursor = node.walk();
                let interpolated = node
                    .named_children(&mut cursor)
                    .any(|child| child.kind() == "interpolation");
                (!interpolated).then(|| self.string_of(node))
            }
            "simple_symbol" => Some(self.text(node).trim_start_matches(':').to_string()),
            "bare_symbol" | "hash_key_symbol" => Some(self.text(node).to_string()),
            _ => None,
        }
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
        Some(Opt {
            key: self.rails_literal(key)?,
            string_key: key.kind() == "string",
            value,
        })
    }

    fn rails_positional<'t>(arguments: &[Node<'t>]) -> Vec<Node<'t>> {
        arguments
            .iter()
            .copied()
            .filter(|argument| !matches!(argument.kind(), "pair" | "hash" | "hash_splat_argument"))
            .collect()
    }

    /// The scope that the blocks around the node give. `None` means that a block is not readable.
    /// The scope inside each block is kept, so a route reads the scope of its nearest block, and
    /// the search does not walk to the `draw` call again.
    fn rails_scope(&self, node: Node<'_>) -> Option<Scope> {
        let mut pending: Vec<Node<'_>> = Vec::new();
        let mut current = node;
        let mut scope: Option<Rc<Scope>> = loop {
            match Self::next_frame(current) {
                Frame::Invalid => break None,
                Frame::Root => {
                    break Some(Rc::new(Scope {
                        prefix: Some(String::from("/")),
                        ..Scope::default()
                    }))
                }
                Frame::Call(frame) => {
                    if let Some(known) = self.rails_scopes.borrow().get(&frame.id()) {
                        break known.clone();
                    }
                    pending.push(frame);
                    current = frame;
                }
            }
        };
        for frame in pending.into_iter().rev() {
            scope = scope.and_then(|outer| {
                if matches!(
                    frame
                        .child_by_field_name("method")
                        .map(|method| self.text(method)),
                    Some("constraints" | "defaults")
                ) && frame.child_by_field_name("receiver").is_none()
                {
                    return Some(outer);
                }
                self.rails_enter(frame, (*outer).clone()).map(Rc::new)
            });
            self.rails_scopes
                .borrow_mut()
                .insert(frame.id(), scope.clone());
        }
        scope
            .filter(|scope| scope.draw)
            .map(|scope| (*scope).clone())
    }

    /// The nearest block call above the node. A method, a lambda, a loop, a class, and a dead
    /// branch between them make the scope not readable.
    fn next_frame(start: Node<'_>) -> Frame<'_> {
        let mut child = start;
        while let Some(parent) = child.parent() {
            match parent.kind() {
                "call" if parent.child_by_field_name("block") == Some(child) => {
                    return Frame::Call(parent)
                }
                "for" | "while" | "until" | "while_modifier" | "until_modifier" | "method"
                | "singleton_method" | "lambda" | "class" | "module" | "singleton_class" => {
                    return Frame::Invalid
                }
                "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional"
                    if Self::is_dead_branch(parent, child) =>
                {
                    return Frame::Invalid
                }
                _ => {}
            }
            child = parent;
        }
        Frame::Root
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
            scope.draw = true;
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
                scope.modules.extend(split_segments(&module));
                scope.resource = None;
                scope.method_scope = false;
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
                    scope
                        .modules
                        .extend(split_segments(&self.rails_literal(value)?));
                }
                if let Some(value) = options.get("controller") {
                    scope.controller = Some(self.rails_literal(value)?);
                }
                Some(scope)
            }
            "controller" => {
                if positional.len() != 1 || !options.0.is_empty() {
                    return None;
                }
                scope.controller = Some(self.rails_literal(positional[0])?);
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
                    || matches!(self.rails_api_only(), ApiOnly::Unknown)
                {
                    return None;
                }
                let name = &definition.names[0];
                let naming = definition.as_name.as_deref().unwrap_or(name);
                let context = definition.context(singular, name, self.rails_singular(naming));
                scope.prefix = scope.nested_prefix();
                scope.controller =
                    Some(definition.controller_for(singular, name, self.rails_plural(name))?);
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
                "module" => modules.extend(split_segments(&self.rails_literal(option.value)?)),
                key if HARMLESS_OPTIONS.contains(&key) => {}
                _ => return None,
            }
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
        let api_only = match self.rails_api_only() {
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
                    Some(controller) => self.rails_handler(&definition.modules, controller, action),
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
        let rocket = options.0.iter().find(|option| option.string_key);
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

        let target = match target_text {
            Some(node) if node.kind() == "string" => {
                let Some(text) = self.rails_literal(node) else {
                    return;
                };
                match text.split_once('#') {
                    Some((controller, action)) => Target {
                        controller: Some(controller.to_string()),
                        action: Some(action.to_string()),
                    },
                    None => Target {
                        controller: None,
                        action: None,
                    },
                }
            }
            _ => {
                let controller = options
                    .get("controller")
                    .and_then(|value| self.rails_literal(value))
                    .or_else(|| scope.controller.clone());
                let from_path = (target_text.is_none()
                    && !root
                    && !path_text.is_empty()
                    && path_text
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .then(|| path_text.replace('-', "_"));
                let action = options
                    .get("action")
                    .and_then(|value| self.rails_literal(value))
                    .or(from_path.filter(|_| controller.is_some()));
                Target { controller, action }
            }
        };

        let methods: Vec<String> = match name {
            "match" => {
                let Some(via) = options.get("via").and_then(|value| self.rails_names(value)) else {
                    return;
                };
                via.into_iter()
                    .map(|verb| {
                        if verb == "all" {
                            String::from("ANY")
                        } else {
                            verb.to_uppercase()
                        }
                    })
                    .collect()
            }
            "root" => vec![String::from("GET")],
            verb => vec![verb.to_uppercase()],
        };
        let (symbol, guards) = match (&target.controller, &target.action) {
            (Some(controller), Some(action)) => {
                self.rails_handler(&scope.modules, controller, action)
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
                read_inflection_calls(parsed.root_node(), &parsed.source, false, &[], &mut found);
            }
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

    /// Whether the application sets `config.api_only`. Omen reads the files in `config`. A value
    /// that is not `true` or `false`, and an assignment in a branch that omen cannot evaluate,
    /// make the value unknown.
    fn rails_api_only(&self) -> ApiOnly {
        *self.rails_shared().api_only.get_or_init(|| {
            let mut values: Vec<Option<bool>> = Vec::new();
            for (position, file) in self.all.iter().enumerate() {
                let in_config = file.file.starts_with("config/") || file.file.contains("/config/");
                if file.language != Language::Ruby || !in_config {
                    continue;
                }
                if let Some(parsed) = self.all_parsed.tree(position) {
                    read_api_only(parsed.root_node(), &parsed.source, false, &mut values);
                }
            }
            if values.iter().any(Option::is_none)
                || (values.contains(&Some(true)) && values.contains(&Some(false)))
            {
                ApiOnly::Unknown
            } else if values.contains(&Some(true)) {
                ApiOnly::On
            } else {
                ApiOnly::Off
            }
        })
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

    /// The action symbol and the guards of a controller. `controller` is `admin/users`, and
    /// `modules` are the modules that `namespace` and `scope module:` add. A controller path
    /// with a leading `/` ignores the modules. The class must have the same chain of modules as its
    /// container path. A route without a module matches only a class outside each module. The
    /// index keeps the result for each class and action. If the inflections of the application
    /// have an acronym that omen cannot read, the controller is not known.
    fn rails_handler(&self, modules: &[String], controller: &str, action: &str) -> Handler {
        let inflections = self.rails_inflections();
        if inflections.unreadable_acronym {
            return (None, Vec::new());
        }
        let mut segments: Vec<String> = if controller.starts_with('/') {
            Vec::new()
        } else {
            modules.to_vec()
        };
        segments.extend(split_segments(controller));
        let Some(last) = segments.pop() else {
            return (None, Vec::new());
        };
        let mut qualified: Vec<String> = segments
            .iter()
            .map(|module| camelize(module, &inflections.acronyms))
            .collect();
        qualified.push(camelize(
            &format!("{last}_controller"),
            &inflections.acronyms,
        ));
        let identity = qualified.join(".");
        let shared = self.rails_shared();
        let key = (identity.clone(), action.to_string());
        if let Some(found) = shared.handled.lock().get(&key) {
            return found.clone();
        }
        let result = self.rails_resolve(shared, &identity, action);
        shared.handled.lock().insert(key, result.clone());
        result
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
        let mut passed: Vec<String> = Vec::new();
        let mut current = identity.to_string();
        let reach = loop {
            if let Some(found) = shared
                .actions
                .lock()
                .get(&(current.clone(), action.to_string()))
            {
                break *found;
            }
            passed.push(current.clone());
            let visibility = self.rails_visibility(shared, &current);
            if visibility.unreadable {
                break Reach::Hidden;
            }
            if visibility.defines.contains(action) {
                break if visibility.public_names.contains(action) {
                    Reach::Public
                } else {
                    Reach::Hidden
                };
            }
            if let Some(public) = visibility.overrides.get(action) {
                break if *public {
                    Reach::Public
                } else {
                    Reach::Hidden
                };
            }
            match shared.parents.get(&current) {
                Some(Some(parent)) => current = parent.clone(),
                _ => break Reach::Public,
            }
        };
        {
            let mut memo = shared.actions.lock();
            for name in passed {
                memo.insert((name, action.to_string()), reach);
            }
        }
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
        let mut current = Some(identity.to_string());
        let mut program: Option<Arc<Program>> = None;
        while let Some(Some(parent)) = current.as_ref().and_then(|name| shared.parents.get(name)) {
            if let Some(found) = shared.programs.lock().get(parent) {
                program = Some(Arc::clone(found));
                break;
            }
            chain.push(parent.clone());
            current = Some(parent.clone());
        }
        for (depth, class) in chain.iter().enumerate().rev() {
            let mut own: Vec<Callback> = Vec::new();
            for (position, index) in shared.classes.get(class).into_iter().flatten() {
                own.extend(
                    self.rails_callbacks(shared, *position, *index)
                        .iter()
                        .cloned(),
                );
            }
            if !own.is_empty() {
                program = Some(Arc::new(Program::layer(program.take(), own)));
            }
            let branching = shared
                .child_counts
                .get(class)
                .is_some_and(|count| *count >= 2);
            if depth > 0 && branching {
                if let Some(program) = &program {
                    shared
                        .programs
                        .lock()
                        .insert(class.clone(), Arc::clone(program));
                }
            }
        }
        let program = program.unwrap_or_else(|| Arc::new(Program::layer(None, Vec::new())));
        shared
            .programs
            .lock()
            .insert(identity.to_string(), Arc::clone(&program));
        program
    }

    /// The callback calls in the body of the class, in source order. A call in a method, or in a
    /// nested class or module, is not a callback of this class. The index keeps the result.
    fn rails_callbacks(
        &self,
        shared: &RailsShared,
        position: usize,
        index: usize,
    ) -> Arc<Vec<Callback>> {
        if let Some(found) = shared.callbacks.lock().get(&(position, index)) {
            return Arc::clone(found);
        }
        let mut callbacks = Vec::new();
        self.with_class_body(position, index, |reader, node| {
            reader.collect_callbacks(node, false, &mut callbacks);
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
                    if self.defines_methods(statement) {
                        out.unreadable = true;
                    }
                }
            }
        }
    }

    /// Tells if the code has a `def`, or a call of `private`, `protected`, or `public`, outside a
    /// nested class or module.
    fn defines_methods(&self, node: Node<'_>) -> bool {
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
            .any(|child| self.defines_methods(child))
    }

    /// Reads the callbacks in the children of the node. `conditional` tells that a branch that
    /// omen cannot evaluate holds the node. A callback in such a branch is conditional. A literal
    /// condition makes a branch dead or live, as it does for a route.
    fn collect_callbacks(&self, node: Node<'_>, conditional: bool, out: &mut Vec<Callback>) {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        for child in children {
            self.collect_callback_node(child, conditional, out);
        }
    }

    fn collect_callback_node(&self, node: Node<'_>, conditional: bool, out: &mut Vec<Callback>) {
        match node.kind() {
            "method" | "singleton_method" | "class" | "module" | "singleton_class" => {}
            "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
                let condition = node.child_by_field_name("condition");
                let literal = condition
                    .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
                let mut cursor = node.walk();
                let parts: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
                for part in parts {
                    if Some(part) == condition || Self::is_dead_branch(node, part) {
                        continue;
                    }
                    self.collect_callback_node(part, conditional || !literal, out);
                }
            }
            "case" | "case_match" | "do_block" | "block" | "lambda" => {
                self.collect_callbacks(node, true, out)
            }
            "call" if node.child_by_field_name("receiver").is_none() => {
                match self.rails_callback(node) {
                    Some(mut callback) => {
                        callback.conditional |= conditional;
                        out.push(callback);
                    }
                    None => self.collect_callbacks(node, conditional, out),
                }
            }
            _ => self.collect_callbacks(node, conditional, out),
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
            .filter_map(|argument| self.rails_literal(argument))
            .collect();
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
        Some(Callback {
            skip,
            names,
            only: only.flatten(),
            except: except.flatten(),
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
    only: Option<Vec<String>>,
    except: Option<Vec<String>>,
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
            && self
                .only
                .as_ref()
                .is_none_or(|only| only.iter().any(|name| name == action))
            && self
                .except
                .as_ref()
                .is_none_or(|except| !except.iter().any(|name| name == action))
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
    slots: HashMap<String, Slot>,
    names: Vec<String>,
}

/// The callbacks of a chain of classes in the order of Rails, ready to answer for an action.
///
/// A callback replaces the callbacks with its names, and it adds its names when it applies. Thus
/// the last callback of a name decides the guard of that name. Only the lists `only:` and
/// `except:` make the result depend on the action. The program keeps the result for an action that
/// no list names. For another action, it works out again the names of the callbacks that list the
/// action.
///
/// A program is a layer: the callbacks of one class, and the program of its superclass. A layer
/// does not copy the callbacks of the superclass.
struct Program {
    parent: Option<Arc<Program>>,
    /// The number of callbacks in the layers of the superclasses.
    offset: usize,
    callbacks: Vec<Callback>,
    by_action: HashMap<String, Vec<usize>>,
    by_name: HashMap<String, Vec<usize>>,
    default: std::sync::OnceLock<DefaultGuards>,
}

impl Program {
    fn layer(parent: Option<Arc<Program>>, callbacks: Vec<Callback>) -> Self {
        let offset = parent
            .as_ref()
            .map_or(0, |parent| parent.offset + parent.callbacks.len());
        let mut by_action: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, callback) in callbacks.iter().enumerate() {
            for name in &callback.names {
                by_name.entry(name.clone()).or_default().push(index);
            }
            if callback.unreadable {
                continue;
            }
            for action in callback.only.iter().chain(&callback.except).flatten() {
                by_action.entry(action.clone()).or_default().push(index);
            }
        }
        Self {
            parent,
            offset,
            callbacks,
            by_action,
            by_name,
            default: std::sync::OnceLock::new(),
        }
    }

    /// This layer, then the layers of the superclasses.
    fn layers(&self) -> impl Iterator<Item = &Program> {
        std::iter::successors(Some(self), |layer| layer.parent.as_deref())
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
                    let place = callback.names.iter().position(|n| n == name).unwrap_or(0);
                    (self.offset + *index, place, callback.prepend)
                }));
            }
        }
        Decision::Undecided
    }

    /// The slot of the guard with the name, or `None` when the action has no such guard.
    fn slot_of(&self, name: &str, action: &str) -> Option<Slot> {
        for layer in self.layers() {
            if let Decision::Decided(slot) = layer.decide(name, action) {
                return slot;
            }
        }
        None
    }

    /// The default guards. The search goes up to the nearest layer that has them, and it applies
    /// the layers below it one time each.
    fn default_guards(&self) -> &DefaultGuards {
        self.default.get_or_init(|| {
            let mut pending = vec![self];
            let mut slots: HashMap<String, Slot> = HashMap::new();
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
                        Decision::Decided(Some(slot)) => {
                            slots.insert(name.clone(), slot);
                        }
                        Decision::Decided(None) => {
                            slots.remove(name);
                        }
                        Decision::Undecided => {}
                    }
                }
            }
            let mut ordered: Vec<(&str, Slot)> = slots
                .iter()
                .map(|(name, slot)| (name.as_str(), *slot))
                .collect();
            order_slots(&mut ordered);
            let names = ordered
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect();
            DefaultGuards { slots, names }
        })
    }

    fn guards_for(&self, action: &str) -> Vec<String> {
        let default = self.default_guards();
        let mut affected: HashSet<&str> = HashSet::new();
        for layer in self.layers() {
            for index in layer.by_action.get(action).into_iter().flatten() {
                affected.extend(layer.callbacks[*index].names.iter().map(String::as_str));
            }
        }
        if affected.is_empty() {
            return default.names.clone();
        }
        let mut slots: Vec<(&str, Slot)> = default
            .slots
            .iter()
            .filter(|(name, _)| !affected.contains(name.as_str()))
            .map(|(name, slot)| (name.as_str(), *slot))
            .collect();
        for name in affected {
            if let Some(slot) = self.slot_of(name, action) {
                slots.push((name, slot));
            }
        }
        order_slots(&mut slots);
        slots
            .into_iter()
            .map(|(name, _)| name.to_string())
            .collect()
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
type Handler = (Option<String>, Vec<String>);

/// The nearest block call above a node.
enum Frame<'t> {
    Invalid,
    Root,
    Call(Node<'t>),
}

/// A symbol of the index: the position of the file, and the position in the symbols of the file.
type Placed = (usize, usize);

/// The class that a short name gives. A name that belongs to two classes is ambiguous.
enum UniqueClass {
    One(String),
    Many,
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

/// The value of `config.api_only` of the application.
#[derive(Clone, Copy)]
enum ApiOnly {
    On,
    Off,
    Unknown,
}

/// Reads the assignments `x.api_only = value` in the code. A literal `true` or `false` gives the
/// value. Any other value, and an assignment in a branch that omen cannot evaluate, give `None`.
/// A dead branch has no effect. A block is a branch that omen cannot evaluate, except the block of
/// `configure`.
fn read_api_only(node: Node<'_>, source: &[u8], conditional: bool, out: &mut Vec<Option<bool>>) {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    match node.kind() {
        "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
            let condition = node.child_by_field_name("condition");
            let literal = condition
                .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
            for part in children {
                if Some(part) == condition || Context::is_dead_branch(node, part) {
                    continue;
                }
                read_api_only(part, source, conditional || !literal, out);
            }
            return;
        }
        "case" | "case_match" | "lambda" | "method" | "singleton_method" | "for" | "while"
        | "until" | "while_modifier" | "until_modifier" => {
            for child in children {
                read_api_only(child, source, true, out);
            }
            return;
        }
        "assignment" => {
            let assigns_api_only = node.child_by_field_name("left").is_some_and(|left| {
                left.kind() == "call"
                    && left
                        .child_by_field_name("method")
                        .is_some_and(|method| super::syntax::text(method, source) == "api_only")
            });
            if assigns_api_only {
                let value = match node.child_by_field_name("right").map(|right| right.kind()) {
                    Some("true") => Some(true),
                    Some("false") => Some(false),
                    _ => None,
                };
                out.push(if conditional { None } else { value });
            }
        }
        _ => {}
    }
    let block_conditional = conditional
        || node.kind() != "call"
        || node
            .child_by_field_name("method")
            .is_none_or(|method| super::syntax::text(method, source) != "configure");
    for child in children {
        let is_block = matches!(child.kind(), "do_block" | "block");
        read_api_only(
            child,
            source,
            if is_block {
                block_conditional
            } else {
                conditional
            },
            out,
        );
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
            .is_some_and(|receiver| {
                super::syntax::text(receiver, source)
                    .trim_start_matches("::")
                    .ends_with("Inflector")
            })
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

/// Reads the inflection calls in the code: `acronym` with a string, and the calls that change
/// a singular or a plural form. Only a call on the object of the inflections counts: the
/// parameter of the block of `ActiveSupport::Inflector.inflections`, or the result of that call.
/// A call in a dead branch has no effect. A call in a conditional, a loop, a method, or a block
/// other than the block of `inflections` is not known: the inflections are then unreadable.
fn read_inflection_calls(
    node: Node<'_>,
    source: &[u8],
    conditional: bool,
    params: &[String],
    out: &mut Inflections,
) {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    match node.kind() {
        "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional" => {
            let condition = node.child_by_field_name("condition");
            let literal = condition
                .is_some_and(|condition| matches!(condition.kind(), "true" | "false" | "nil"));
            for part in children {
                if Some(part) == condition || Context::is_dead_branch(node, part) {
                    continue;
                }
                read_inflection_calls(part, source, conditional || !literal, params, out);
            }
            return;
        }
        "case" | "case_match" | "lambda" | "method" | "singleton_method" | "for" | "while"
        | "until" | "while_modifier" | "until_modifier" => {
            for child in children {
                read_inflection_calls(child, source, true, params, out);
            }
            return;
        }
        _ => {}
    }
    let mut block_conditional = conditional;
    let mut block_params: Vec<String> = params.to_vec();
    if node.kind() == "call" {
        let method = node
            .child_by_field_name("method")
            .map_or("", |method| super::syntax::text(method, source));
        let receiver = node.child_by_field_name("receiver");
        let opens_inflections = method == "inflections"
            && receiver.is_some_and(|receiver| {
                super::syntax::text(receiver, source)
                    .trim_start_matches("::")
                    .ends_with("Inflector")
            });
        block_conditional = conditional || !opens_inflections;
        if opens_inflections {
            block_params.extend(block_parameter_names(node, source));
        }
        let on_object =
            receiver.is_some_and(|receiver| is_inflection_object(receiver, source, params));
        let affects = on_object
            && matches!(
                method,
                "acronym" | "irregular" | "uncountable" | "human" | "clear" | "singular" | "plural"
            );
        if affects && conditional {
            out.unreadable_acronym = true;
            out.changes_singulars = true;
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
                            let mut inner = argument.walk();
                            let interpolated = argument
                                .named_children(&mut inner)
                                .any(|child| child.kind() == "interpolation");
                            (argument.kind() == "string" && !interpolated).then(|| {
                                super::syntax::unquote(super::syntax::text(*argument, source))
                                    .to_string()
                            })
                        })
                        .collect();
                    if words.is_empty() || words.len() != arguments.len() {
                        out.unreadable_acronym = true;
                    }
                    for word in words {
                        out.acronyms.insert(word.to_lowercase(), word);
                    }
                }
                "clear" => {
                    out.unreadable_acronym = true;
                    out.changes_singulars = true;
                }
                _ => out.changes_singulars = true,
            }
        }
    }
    for child in children {
        let is_block = matches!(child.kind(), "do_block" | "block");
        if is_block {
            read_inflection_calls(child, source, block_conditional, &block_params, out);
        } else {
            read_inflection_calls(child, source, conditional, params, out);
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
    child_counts: HashMap<String, usize>,
    inflections: std::sync::OnceLock<Inflections>,
    api_only: std::sync::OnceLock<ApiOnly>,
    callbacks: Mutex<HashMap<Placed, Arc<Vec<Callback>>>>,
    visibility: Mutex<HashMap<String, Arc<Visibility>>>,
    programs: Mutex<HashMap<String, Arc<Program>>>,
    handled: Mutex<HashMap<ActionKey, Handler>>,
    actions: Mutex<HashMap<ActionKey, Reach>>,
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
        let (parents, child_counts) = resolve_parents(all, &classes, &superclasses);
        Self {
            classes,
            methods,
            parents,
            child_counts,
            inflections: std::sync::OnceLock::new(),
            api_only: std::sync::OnceLock::new(),
            callbacks: Mutex::new(HashMap::new()),
            visibility: Mutex::new(HashMap::new()),
            programs: Mutex::new(HashMap::new()),
            handled: Mutex::new(HashMap::new()),
            actions: Mutex::new(HashMap::new()),
        }
    }
}

/// The superclass identity of each class, and the number of subclasses of each class. A
/// qualified superclass name, a name of two classes, and a class in a loop of superclasses give
/// no superclass. A class that leads into a loop keeps its superclass. Omen visits each class one
/// time.
fn resolve_parents(
    all: &[FileSymbols],
    classes: &HashMap<String, Vec<Placed>>,
    superclasses: &HashMap<&str, UniqueClass>,
) -> (HashMap<String, Option<String>>, HashMap<String, usize>) {
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
    let mut child_counts: HashMap<String, usize> = HashMap::new();
    for parent in parents.values().flatten() {
        *child_counts.entry(parent.clone()).or_default() += 1;
    }
    (parents, child_counts)
}

/// The chain of containers and the name of a class, joined with `.`. The key of the symbol has
/// the chain, so the key tells `Admin::V1::UsersController` from `Other::V1::UsersController`.
fn qualified_name(file: &FileSymbols, symbol: &Symbol) -> String {
    let key = symbol.key.to_string();
    let chain = key.strip_prefix(&format!("{}#", file.file)).unwrap_or(&key);
    let chain = chain.split('~').next().unwrap_or(chain);
    chain.replace("::", ".")
}

fn split_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
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
    modules: Vec<String>,
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
