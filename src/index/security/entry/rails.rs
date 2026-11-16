//! Rails routes: the entry points that `routes.rb` registers.
//!
//! Omen reads these forms:
//!
//! - The calls `get`, `post`, `put`, `patch`, and `delete` with a path.
//! - The call `match` with `via:`, and the call `root`.
//! - The calls `resources` and `resource` with `only:`, `except:`, `path:`,
//!   `controller:`, and `module:`.
//! - The blocks `namespace`, `scope`, `member`, `collection`, `controller`,
//!   `constraints`, `defaults`, and the Devise `authenticate` family.
//! - The options `controller:`, `action:`, `to:`, and `on:`.
//!
//! Omen skips a route that it cannot read exactly. It does not guess. Omen skips
//! these forms:
//!
//! - A path, a name, or an option that is not a literal. Examples are a string with
//!   interpolation, a variable, and a constant.
//! - A route in a block that omen does not know, such as `each`, `times`, or
//!   `concern`. A route in a `for` loop or a `while` loop is also skipped.
//! - A route in a branch that a literal condition makes dead: `if false`, `if nil`,
//!   `unless true`, and the `else` branch of `if true`.
//!
//! A route in any other `if`, `unless`, `case`, or ternary is registered. The condition can be
//! true at run time, as `if Rails.env.development?` can be. A route in a method, a lambda, or a
//! class body is not registered. A route outside a `routes.draw` block is not registered.
//!
//! Omen also skips these forms:
//!
//! - A `resources` call with an option that changes the paths in a way that omen
//!   does not read. Examples are `param:`, `shallow:`, `path_names:`, and `singular:`.
//! - A `scope` path with an optional segment, such as `"(:locale)"`.
//! - The calls `concerns`, `draw(:file)`, `mount`, `devise_for`, `direct`, and
//!   `resolve`. These calls give no route that omen reads.
//!
//! A nested route needs the singular name of its parent resource. Omen uses the default
//! inflections of Rails 7.2.2. For `categories`, the parameter is `:category_id`. For `jeans`,
//! the word is uncountable, and the parameter is `:jeans_id`.
//!
//! An application can change the inflections. Omen checks the files in `config/initializers`
//! and the files with `inflect` in the name. If one of them calls `inflections`, `irregular`,
//! `uncountable`, `acronym`, or `inflect.singular`, omen skips the nested routes. It keeps the
//! `member` routes and the `collection` routes. Comments do not change the inflections. Omen does not
//! find a change in a different file.
//!
//! A `resource` call uses the plural controller. If the plural is irregular, the route has no
//! symbol.
//!
//! The guards of a route are the `before_action` callbacks of the action. They are in the order
//! that Rails runs them. Omen reads the callbacks from the syntax tree. It reads the symbol
//! names, the `only:` list, and the `except:` list. The callbacks of a superclass that the
//! repository has come first. A `skip_before_action` call removes a guard for the actions that
//! it lists.
//!
//! Omen treats the openings of a class with one chain of modules and one name as one class. It
//! reads the callbacks in the order of the files and then of the positions. If a superclass name
//! belongs to two different classes, omen reads no guards from the superclass.
//!
//! A guard never claims more than omen can read. A `before_action` call gives no guard in two
//! cases. The first case is an `if:` or `unless:` option. The second case is an `only:` or
//! `except:` value that is not a literal list. A `skip_before_action` call with `if:` or
//! `unless:` removes the guard for the actions that it names. A `skip_before_action` call with
//! a value that is not a literal list removes the guard for each action.
//!
//! A `before_action` call replaces an earlier callback with the same name, as Rails does. The
//! replaced callback is gone for each action. This holds across the superclasses and the
//! openings of a class. A callback in a class body follows the same rule for dead branches as a
//! route does. A callback in any other `if`, `unless`, or `case` is conditional. It adds no
//! guard, and a `skip_before_action` call in it removes the guard for the actions that it names.
//!
//! A controller path with a leading `/` ignores the module of the `namespace`. The controller
//! class must have the same chain of modules as the route. A route without a module matches
//! only a class that is in no module. Omen builds the index of the classes one time for each
//! routes file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use tree_sitter::Node;

use super::{join_path, route, Context, Draft, EntryBasis, EntryKind};
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
const HARMLESS_OPTIONS: [&str; 5] = ["as", "constraints", "defaults", "format", "concerns"];

/// Blocks that pass the scope of their parent on to the routes inside.
const TRANSPARENT_BLOCKS: [&str; 7] = [
    "draw",
    "constraints",
    "defaults",
    "authenticate",
    "authenticated",
    "unauthenticated",
    "devise_scope",
];

#[derive(Clone, Default)]
struct Scope {
    /// The path of the enclosing blocks. `None` means that the path is not known.
    prefix: Option<String>,
    modules: Vec<String>,
    controller: Option<String>,
    resource: Option<ResourcePaths>,
}

#[derive(Clone)]
struct ResourcePaths {
    collection: Option<String>,
    member: Option<String>,
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

fn camelize(part: &str) -> String {
    part.split('_')
        .map(|word| {
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

/// The singular of a word as Rails 7.2.2 gives it with its default inflections. Rails uses an
/// uncountable word as it is. The rules of an application can change the result. See
/// `Context::rails_has_custom_inflections`.
fn singularize(word: &str) -> String {
    static RULES: std::sync::LazyLock<Vec<(regex::Regex, &'static str)>> =
        std::sync::LazyLock::new(|| {
            SINGULAR_RULES
                .iter()
                .rev()
                .map(|(pattern, replacement)| {
                    (
                        regex::Regex::new(&format!("(?i){pattern}")).expect("a fixed pattern"),
                        *replacement,
                    )
                })
                .collect()
        });
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
    for (rule, replacement) in RULES.iter() {
        if rule.is_match(word) {
            return rule.replace(word, *replacement).into_owned();
        }
    }
    word.to_string()
}

/// The plural of a regular English noun, or `None` when the word needs a dictionary.
fn plural_of(singular: &str) -> Option<String> {
    let irregular = ["person", "man", "woman", "child", "foot", "tooth", "mouse"];
    if singular.ends_with('s') || irregular.iter().any(|word| singular.ends_with(word)) {
        return None;
    }
    let vowels = ['a', 'e', 'i', 'o', 'u'];
    let mut characters = singular.chars().rev();
    let last = characters.next()?;
    let before = characters.next();
    if last == 'y' && before.is_some_and(|c| !vowels.contains(&c)) {
        return Some(format!("{}ies", &singular[..singular.len() - 1]));
    }
    if singular.ends_with(['x', 'z']) || singular.ends_with("ch") || singular.ends_with("sh") {
        return Some(format!("{singular}es"));
    }
    Some(format!("{singular}s"))
}

fn append(prefix: &Option<String>, part: &str) -> Option<String> {
    prefix.as_ref().map(|prefix| join_path(prefix, part))
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
    fn rails_scope(&self, node: Node<'_>) -> Option<Scope> {
        let mut frames = Vec::new();
        let mut child = node;
        while let Some(parent) = child.parent() {
            match parent.kind() {
                "call" if parent.child_by_field_name("block") == Some(child) => frames.push(parent),
                "for" | "while" | "until" | "while_modifier" | "until_modifier" | "method"
                | "singleton_method" | "lambda" | "class" | "module" | "singleton_class" => {
                    return None
                }
                "if" | "elsif" | "unless" | "if_modifier" | "unless_modifier" | "conditional"
                    if Self::is_dead_branch(parent, child) =>
                {
                    return None
                }
                _ => {}
            }
            child = parent;
        }
        let mut scope = Scope {
            prefix: Some(String::from("/")),
            ..Scope::default()
        };
        let draws = |frame: &Node<'_>| self.is_draw_block(*frame);
        if !frames.iter().any(draws) {
            return None;
        }
        for frame in frames.into_iter().rev() {
            scope = self.rails_enter(frame, scope)?;
        }
        Some(scope)
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

    /// Tells if the call is `routes.draw do`, as `Rails.application.routes.draw` is.
    fn is_draw_block(&self, call: Node<'_>) -> bool {
        call.child_by_field_name("method")
            .is_some_and(|method| self.text(method) == "draw")
            && call
                .child_by_field_name("receiver")
                .is_some_and(|receiver| self.text(receiver).ends_with("routes"))
    }

    fn rails_enter(&self, call: Node<'_>, mut scope: Scope) -> Option<Scope> {
        let method = self.text(call.child_by_field_name("method")?);
        let receiver = call.child_by_field_name("receiver").is_some();
        if TRANSPARENT_BLOCKS.contains(&method)
            && (self.is_draw_block(call) || (method != "draw" && !receiver))
        {
            return Some(scope);
        }
        let arguments = Self::arguments_of(call);
        let options = self.rails_options(&arguments)?;
        let positional = Self::rails_positional(&arguments);
        if receiver {
            return None;
        }
        match method {
            "namespace" => {
                let name = self.rails_literal(*positional.first()?)?;
                let path = options
                    .get("path")
                    .map_or(Some(name.clone()), |value| self.rails_literal(value))?;
                let module = options
                    .get("module")
                    .map_or(Some(name), |value| self.rails_literal(value))?;
                scope.prefix = append(&scope.prefix, &path);
                scope.modules.extend(split_segments(&module));
                scope.resource = None;
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
                    if path.contains('(') {
                        return None;
                    }
                    scope.prefix = append(&scope.prefix, &path);
                }
                if let Some(value) = options.get("module") {
                    scope
                        .modules
                        .extend(split_segments(&self.rails_literal(value)?));
                }
                if let Some(value) = options.get("controller") {
                    scope.controller = Some(self.rails_literal(value)?);
                }
                scope.resource = None;
                Some(scope)
            }
            "controller" => {
                scope.controller = Some(self.rails_literal(*positional.first()?)?);
                Some(scope)
            }
            "member" | "collection" => {
                let resource = scope.resource.clone()?;
                let member = method == "member";
                scope.prefix = if member {
                    resource.member
                } else {
                    resource.collection
                };
                scope.prefix.as_ref()?;
                Some(scope)
            }
            "resources" | "resource" => {
                let singular = method == "resource";
                let definition = self.rails_resource_definition(&positional, &options, &scope)?;
                if definition.names.len() != 1 {
                    return None;
                }
                let name = &definition.names[0];
                let paths = definition.paths(singular, &scope, name, self.rails_singular(name));
                scope.controller = Some(definition.controller_for(singular, name)?);
                scope.modules = definition.modules.clone();
                scope.prefix = paths.nested.clone();
                scope.resource = Some(ResourcePaths {
                    collection: paths.collection,
                    member: paths.member,
                });
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
        let table: &[(&str, &str, &str)] = if singular {
            &SINGULAR_ACTIONS
        } else {
            &COLLECTION_ACTIONS
        };
        for name in &definition.names {
            let paths = definition.paths(singular, scope, name, self.rails_singular(name));
            let Some(base) = paths.base else {
                continue;
            };
            let controller = definition.controller_for(singular, name);
            for (action, verb, suffix) in table {
                if !definition.allows(action) {
                    continue;
                }
                let path = format!("{base}{suffix}");
                let route_path = path.clone();
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
                    variant: Some(format!("{action}@{name}@{route_path}")),
                });
            }
        }
    }

    /// `get`, `post`, `put`, `patch`, `delete`, `match`, and `root`.
    fn rails_plain_route(&self, node: Node<'_>, name: &str, scope: &Scope, out: &mut Vec<Draft>) {
        let arguments = Self::arguments_of(node);
        let Some(options) = self.rails_options(&arguments) else {
            return;
        };
        let positional = Self::rails_positional(&arguments);
        let rocket = options.0.iter().find(|option| option.string_key);
        let root = name == "root";

        let mut target_text = None;
        let mut path = None;
        if root {
            match positional.as_slice() {
                [] => {}
                [one] => target_text = Some(*one),
                _ => return,
            }
        } else {
            match (positional.as_slice(), rocket) {
                ([one], None) => path = Some(self.rails_literal(*one)),
                ([], Some(option)) => {
                    path = Some(Some(option.key.clone()));
                    target_text = Some(option.value);
                }
                _ => return,
            }
        }
        if let Some(to) = options.get("to") {
            target_text = Some(to);
        }
        let path_text = if root {
            String::new()
        } else {
            let Some(Some(text)) = &path else {
                return;
            };
            text.clone()
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
                let action = options
                    .get("action")
                    .and_then(|value| self.rails_literal(value))
                    .or_else(|| {
                        (controller.is_some()
                            && target_text.is_none()
                            && !root
                            && !path_text.contains('/'))
                        .then(|| path_text.clone())
                    });
                Target { controller, action }
            }
        };

        let prefix = match options
            .get("on")
            .and_then(|value| self.rails_literal(value))
        {
            Some(on) => {
                let Some(resource) = &scope.resource else {
                    return;
                };
                match on.as_str() {
                    "member" => resource.member.clone(),
                    "collection" => resource.collection.clone(),
                    _ => return,
                }
            }
            None => scope.prefix.clone(),
        };
        let Some(prefix) = prefix else {
            return;
        };
        let full_path = join_path(&prefix, &path_text);

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

    /// The singular name that a nested route needs, or `None` when the inflections of the
    /// application can change the name.
    fn rails_singular(&self, name: &str) -> Option<String> {
        (!self.rails_has_custom_inflections()).then(|| singularize(name))
    }

    /// Tells if a file in `config/initializers`, or a file with `inflect` in its name, changes the
    /// inflections of Rails. The comments of the generated `inflections.rb` do not change them.
    fn rails_has_custom_inflections(&self) -> bool {
        let index = self.rails.get_or_init(|| RailsIndex::build(self.all));
        *index.custom_inflections.get_or_init(|| {
            self.all.iter().enumerate().any(|(position, file)| {
                let name = file.file.rsplit('/').next().unwrap_or("");
                file.language == Language::Ruby
                    && (file.file.contains("config/initializers/") || name.contains("inflect"))
                    && self.all_parsed.tree(position).is_some_and(|parsed| {
                        changes_inflections(parsed.root_node(), &parsed.source)
                    })
            })
        })
    }

    /// The action symbol and the guards of a controller. `controller` is `admin/users`, and
    /// `modules` are the modules that `namespace` and `scope module:` add. A controller path
    /// with a leading `/` ignores the modules. The class must have the same chain of modules as its
    /// container path. A route without a module matches only a class outside each module. The
    /// index keeps the result for each class and action.
    fn rails_handler(
        &self,
        modules: &[String],
        controller: &str,
        action: &str,
    ) -> (Option<String>, Vec<String>) {
        let mut segments: Vec<String> = if controller.starts_with('/') {
            Vec::new()
        } else {
            modules.to_vec()
        };
        segments.extend(split_segments(controller));
        let Some(last) = segments.pop() else {
            return (None, Vec::new());
        };
        let mut qualified: Vec<String> = segments.iter().map(|module| camelize(module)).collect();
        qualified.push(format!("{}Controller", camelize(&last)));
        let identity = qualified.join(".");
        let index = self.rails.get_or_init(|| RailsIndex::build(self.all));
        let key = (identity.clone(), action.to_string());
        if let Some(found) = index.handled.borrow().get(&key) {
            return found.clone();
        }
        let result = self.rails_resolve(index, &identity, action);
        index.handled.borrow_mut().insert(key, result.clone());
        result
    }

    fn rails_resolve(
        &self,
        index: &RailsIndex<'a>,
        identity: &str,
        action: &str,
    ) -> (Option<String>, Vec<String>) {
        let Some(classes) = index.classes.get(identity) else {
            return (None, Vec::new());
        };
        let mut actions: Vec<&Symbol> = Vec::new();
        let mut seen_files = std::collections::HashSet::new();
        for (position, _) in classes {
            if !seen_files.insert(*position) {
                continue;
            }
            if let Some(methods) = index
                .methods
                .get(&(*position, identity.to_string()))
                .and_then(|by_action| by_action.get(action))
            {
                actions.extend(methods.iter().copied());
            }
        }
        let symbol = (actions.len() == 1).then(|| actions[0].key.to_string());
        let guards = self.rails_identity_guards(index, identity, action);
        (symbol, guards.as_ref().clone())
    }

    /// The guards of an action for a class identity: the guards of the superclass, then the
    /// callbacks of each opening of the class in the order of the files and then of the positions.
    /// A callback replaces an earlier callback with the same name, as Rails does. The index keeps
    /// the result for each identity and action.
    fn rails_identity_guards(
        &self,
        index: &RailsIndex<'a>,
        identity: &str,
        action: &str,
    ) -> Rc<Vec<String>> {
        let key = (identity.to_string(), action.to_string());
        if let Some(found) = index.guards.borrow().get(&key) {
            return Rc::clone(found);
        }
        let inherited = match self.rails_parent_identity(index, identity) {
            Some(parent) => self.rails_identity_guards(index, &parent, action),
            None => Rc::new(Vec::new()),
        };
        let mut own: Vec<Rc<Vec<Callback>>> = Vec::new();
        for (position, symbol) in index.classes.get(identity).into_iter().flatten() {
            own.push(self.rails_callbacks(index, *position, symbol));
        }
        let result = if own.iter().all(|callbacks| callbacks.is_empty()) {
            inherited
        } else {
            let mut active = ActiveGuards::new(&inherited);
            for callback in own.iter().flat_map(|callbacks| callbacks.iter()) {
                active.apply(callback, action);
            }
            Rc::new(active.into_vec())
        };
        index.guards.borrow_mut().insert(key, Rc::clone(&result));
        result
    }

    /// The identity of the superclass, when the repository has exactly one class for its name.
    /// A qualified name, a name of two classes, and a loop of superclasses give `None`.
    fn rails_parent_identity(&self, index: &RailsIndex<'a>, identity: &str) -> Option<String> {
        if let Some(found) = index.parents.borrow().get(identity) {
            return found.clone();
        }
        let raw = |from: &str| -> Option<String> {
            let parent = index
                .classes
                .get(from)?
                .iter()
                .find_map(|(_, symbol)| symbol.supertypes.first())?;
            if parent.contains("::") {
                return None;
            }
            match index.superclasses.get(parent.as_str())? {
                UniqueClass::One(found) => Some(found.clone()),
                UniqueClass::Many => None,
            }
        };
        let mut result = raw(identity);
        let mut walker = result.clone();
        let mut steps = 0;
        while let Some(current) = walker {
            if current == identity || steps > index.classes.len() {
                result = None;
                break;
            }
            walker = raw(&current);
            steps += 1;
        }
        index
            .parents
            .borrow_mut()
            .insert(identity.to_string(), result.clone());
        result
    }

    /// The callback calls in the body of the class, in source order. A call in a method, or in a
    /// nested class or module, is not a callback of this class. The index keeps the result.
    fn rails_callbacks(
        &self,
        index: &RailsIndex<'a>,
        position: usize,
        class: &Symbol,
    ) -> Rc<Vec<Callback>> {
        if let Some(found) = index.callbacks.borrow().get(&*class.key) {
            return Rc::clone(found);
        }
        let callbacks = Rc::new(self.read_callbacks(position, class));
        index
            .callbacks
            .borrow_mut()
            .insert(class.key.to_string(), Rc::clone(&callbacks));
        callbacks
    }

    fn read_callbacks(&self, position: usize, class: &Symbol) -> Vec<Callback> {
        let Some(parsed) = self.all_parsed.tree(position) else {
            return Vec::new();
        };
        let Some(mut node) = parsed
            .root_node()
            .descendant_for_byte_range(class.range.start_byte, class.range.end_byte)
        else {
            return Vec::new();
        };
        while !matches!(node.kind(), "class" | "module") {
            let Some(parent) = node.parent() else {
                return Vec::new();
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
        let mut callbacks = Vec::new();
        reader.collect_callbacks(node, false, &mut callbacks);
        callbacks
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
            "case" | "case_match" => self.collect_callbacks(node, true, out),
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
        let skip = match self.text(call.child_by_field_name("method")?) {
            "before_action" | "before_filter" => false,
            "skip_before_action" | "skip_before_filter" => true,
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
        let unreadable =
            options.is_none() || matches!(only, Some(None)) || matches!(except, Some(None));
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
        })
    }
}

struct Callback {
    skip: bool,
    names: Vec<String>,
    only: Option<Vec<String>>,
    except: Option<Vec<String>>,
    /// An `only:` or `except:` value that is not a literal list.
    unreadable: bool,
    /// An `if:` or `unless:` option. A condition does not decide the action.
    conditional: bool,
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

/// A class symbol and the position of its file.
type Placed<'a> = (usize, &'a Symbol);

/// A class identity and an action name.
type ActionKey = (String, String);

/// The symbol and the guards of an action.
type Handler = (Option<String>, Vec<String>);

/// The class that a short name gives. A name that belongs to two classes is ambiguous.
enum UniqueClass {
    One(String),
    Many,
}

/// The controller classes by their chain of modules, and their methods by the class that holds
/// them. The key of a method has the chain of its class. The index also keeps each result that it
/// computes. Omen builds the index one time for each routes file.
pub(super) struct RailsIndex<'a> {
    classes: HashMap<String, Vec<Placed<'a>>>,
    superclasses: HashMap<&'a str, UniqueClass>,
    methods: HashMap<(usize, String), HashMap<&'a str, Vec<&'a Symbol>>>,
    callbacks: RefCell<HashMap<String, Rc<Vec<Callback>>>>,
    parents: RefCell<HashMap<String, Option<String>>>,
    guards: RefCell<HashMap<ActionKey, Rc<Vec<String>>>>,
    handled: RefCell<HashMap<ActionKey, Handler>>,
    custom_inflections: std::cell::OnceCell<bool>,
}

impl<'a> RailsIndex<'a> {
    pub(super) fn build(all: &'a [FileSymbols]) -> Self {
        let mut index = Self {
            classes: HashMap::new(),
            superclasses: HashMap::new(),
            methods: HashMap::new(),
            callbacks: RefCell::new(HashMap::new()),
            parents: RefCell::new(HashMap::new()),
            guards: RefCell::new(HashMap::new()),
            handled: RefCell::new(HashMap::new()),
            custom_inflections: std::cell::OnceCell::new(),
        };
        for (position, file) in all.iter().enumerate() {
            if file.language != Language::Ruby {
                continue;
            }
            for symbol in &file.symbols {
                if symbol.kind.is_type() {
                    let identity = qualified_name(file, symbol);
                    let unique = match index.superclasses.get(symbol.name.as_str()) {
                        None => UniqueClass::One(identity.clone()),
                        Some(UniqueClass::One(existing)) if *existing == identity => {
                            UniqueClass::One(identity.clone())
                        }
                        Some(_) => UniqueClass::Many,
                    };
                    index.superclasses.insert(symbol.name.as_str(), unique);
                    index
                        .classes
                        .entry(identity)
                        .or_default()
                        .push((position, symbol));
                } else if symbol.is_callable() {
                    let qualified = qualified_name(file, symbol);
                    if let Some(owner) = qualified.strip_suffix(&format!(".{}", symbol.name)) {
                        index
                            .methods
                            .entry((position, owner.to_string()))
                            .or_default()
                            .entry(symbol.name.as_str())
                            .or_default()
                            .push(symbol);
                    }
                }
            }
        }
        for group in index.classes.values_mut() {
            group.sort_by_key(|(position, symbol)| (*position, symbol.range.start_byte));
        }
        index
    }
}

/// The names of the active guards in the order of Rails. A new callback replaces the callback
/// with the same name, so the new callback comes last.
struct ActiveGuards {
    slots: Vec<Option<String>>,
    at: HashMap<String, usize>,
}

impl ActiveGuards {
    fn new(names: &[String]) -> Self {
        let mut active = Self {
            slots: Vec::with_capacity(names.len()),
            at: HashMap::with_capacity(names.len()),
        };
        for name in names {
            active.push(name);
        }
        active
    }

    fn remove(&mut self, name: &str) {
        if let Some(slot) = self.at.remove(name) {
            self.slots[slot] = None;
        }
    }

    fn push(&mut self, name: &str) {
        self.remove(name);
        self.at.insert(name.to_string(), self.slots.len());
        self.slots.push(Some(name.to_string()));
    }

    /// A `before_action` call replaces the earlier callbacks with its names, and it adds a guard
    /// only when it applies to the action. A `skip_before_action` call removes the guards of the
    /// actions that it applies to.
    fn apply(&mut self, callback: &Callback, action: &str) {
        let applies = callback.applies_to(action);
        for name in &callback.names {
            if callback.skip {
                if applies {
                    self.remove(name);
                }
            } else {
                self.remove(name);
                if applies {
                    self.push(name);
                }
            }
        }
    }

    fn into_vec(self) -> Vec<String> {
        self.slots.into_iter().flatten().collect()
    }
}

/// Tells if the code has a call that changes the inflections: `inflections`, `irregular`,
/// `uncountable`, `acronym`, or `singular`, `plural`, and `human` on a receiver.
fn changes_inflections(node: Node<'_>, source: &[u8]) -> bool {
    if node.kind() == "call" {
        let method = node
            .child_by_field_name("method")
            .map_or("", |method| super::syntax::text(method, source));
        let has_receiver = node.child_by_field_name("receiver").is_some();
        if matches!(
            method,
            "inflections" | "irregular" | "uncountable" | "acronym"
        ) || (has_receiver && matches!(method, "singular" | "plural" | "human"))
        {
            return true;
        }
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| changes_inflections(child, source))
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
    modules: Vec<String>,
}

struct ResourceWays {
    /// The path of the resource, such as `/users`.
    base: Option<String>,
    collection: Option<String>,
    member: Option<String>,
    /// The path that a nested route gets, such as `/users/:user_id`.
    nested: Option<String>,
}

impl ResourceDefinition {
    fn allows(&self, action: &str) -> bool {
        self.only
            .as_ref()
            .is_none_or(|only| only.iter().any(|name| name == action))
            && !self.except.iter().any(|name| name == action)
    }

    fn paths(
        &self,
        singular: bool,
        scope: &Scope,
        name: &str,
        singular_name: Option<String>,
    ) -> ResourceWays {
        let segment = self.path.clone().unwrap_or_else(|| name.to_string());
        let base = append(&scope.prefix, &segment);
        let (member, nested) = if singular {
            (base.clone(), base.clone())
        } else {
            let nested = singular_name.and_then(|singular_name| {
                base.as_ref().map(|b| format!("{b}/:{singular_name}_id"))
            });
            (base.as_ref().map(|b| format!("{b}/:id")), nested)
        };
        ResourceWays {
            collection: base.clone(),
            base,
            member,
            nested,
        }
    }

    /// The controller path of a resource. A singular resource uses the plural controller.
    fn controller_for(&self, singular: bool, name: &str) -> Option<String> {
        if let Some(controller) = &self.controller {
            return Some(controller.clone());
        }
        if singular {
            plural_of(name)
        } else {
            Some(name.to_string())
        }
    }
}
