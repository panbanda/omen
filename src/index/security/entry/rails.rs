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
//! and the files with `inflect` in the name. If one of them calls `irregular`,
//! `uncountable`, or `inflect.singular`, omen skips the nested routes. It keeps the `member`
//! routes and the `collection` routes. Comments do not change the inflections. Omen does not find
//! a change in a different file.
//!
//! Omen reads a literal `inflect.acronym "API"` call in the same files. It builds the class name
//! as `camelize` of ActiveSupport does, so `api/users` gives `API::UsersController`. If an
//! `acronym` argument is not a string, or the files call `clear`, omen does not know the class
//! names. Then no route has a symbol or a guard.
//!
//! A route matches only an instance method that is public: a `def self.x` method, a method in
//! `class << self`, and a method after `private` or `protected` do not match. A method that
//! `private :x` or `private def x` hides does not match. If a class has a definition in a block,
//! or a visibility call that omen cannot read, the route has no symbol.
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
//! route does. A callback in any other `if`, `unless`, or `case` is conditional. A callback in a
//! block, such as `each do` or a lambda, is also conditional. It adds no guard, and a
//! `skip_before_action` call in it removes the guard for the actions that it names. Omen ignores
//! a callback in a method.
//!
//! A `before_action` call with `prepend: true`, and a `prepend_before_action` call, put the
//! callback at the front of the chain. If the `prepend:` value is not a literal, the callback adds
//! no guard.
//!
//! A controller path with a leading `/` ignores the module of the `namespace`. The controller
//! class must have the same chain of modules as the route. A route without a module matches
//! only a class that is in no module. Omen builds the index of the classes one time for each
//! analysis, and each routes file uses it. A route matches a `draw` call only on
//! `Rails.application.routes`, `Name::Application.routes`, or `Name::Engine.routes`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
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

/// Tells if the text is `Rails.application.routes`, or a constant path that ends with
/// `::Application` or `::Engine`, and then `.routes`.
fn is_routes_object(text: &str) -> bool {
    let Some(base) = text.trim().strip_suffix(".routes") else {
        return false;
    };
    let constant_path = base.split("::").all(|part| {
        part.chars()
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    base == "Rails.application"
        || (constant_path && (base.ends_with("::Application") || base.ends_with("::Engine")))
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

    /// Tells if the call is `draw` of the routes of the application or of an engine:
    /// `Rails.application.routes`, `Name::Application.routes`, or `Name::Engine.routes`.
    fn is_draw_block(&self, call: Node<'_>) -> bool {
        call.child_by_field_name("method")
            .is_some_and(|method| self.text(method) == "draw")
            && call
                .child_by_field_name("receiver")
                .is_some_and(|receiver| is_routes_object(self.text(receiver)))
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
                read_inflection_calls(parsed.root_node(), &parsed.source, &mut found);
            }
        }
        found
    }

    /// The singular name that a nested route needs, or `None` when the inflections of the
    /// application can change the name.
    fn rails_singular(&self, name: &str) -> Option<String> {
        (!self.rails_inflections().changes_singulars).then(|| singularize(name))
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
        let symbol = self.rails_action_symbol(shared, identity, action);
        let guards = self.rails_program(shared, identity).guards_for(action);
        (symbol, guards)
    }

    /// The one public instance method with the name of the action. A singleton method, a method
    /// that `private` or `protected` hides, and a class with a visibility that omen cannot read
    /// give no symbol.
    fn rails_action_symbol(
        &self,
        shared: &RailsShared,
        identity: &str,
        action: &str,
    ) -> Option<String> {
        let candidates = shared.methods.get(identity)?.get(action)?;
        let visibility = self.rails_visibility(shared, identity);
        if visibility.unreadable {
            return None;
        }
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
    }

    /// The callbacks of the chain of a class, in the order of Rails. The program of a class with
    /// no callback of its own is the program of its superclass. The index keeps the program of
    /// each class that a route reaches, and of each superclass with two or more subclasses.
    fn rails_program(&self, shared: &RailsShared, identity: &str) -> Arc<Program> {
        if let Some(found) = shared.programs.lock().get(identity) {
            return Arc::clone(found);
        }
        let mut chain = vec![identity.to_string()];
        let mut base: Option<Arc<Program>> = None;
        let mut current = identity.to_string();
        while let Some(Some(parent)) = shared.parents.get(&current) {
            if let Some(found) = shared.programs.lock().get(parent) {
                base = Some(Arc::clone(found));
                break;
            }
            chain.push(parent.clone());
            current = parent.clone();
        }
        let mut callbacks: Vec<Callback> = base
            .as_ref()
            .map(|program| program.callbacks.clone())
            .unwrap_or_default();
        let inherited = callbacks.len();
        for (depth, class) in chain.iter().enumerate().rev() {
            for (position, index) in shared.classes.get(class).into_iter().flatten() {
                callbacks.extend(
                    self.rails_callbacks(shared, *position, *index)
                        .iter()
                        .cloned(),
                );
            }
            let branching = shared
                .child_counts
                .get(class)
                .is_some_and(|count| *count >= 2);
            if depth > 0 && branching {
                shared
                    .programs
                    .lock()
                    .insert(class.clone(), Arc::new(Program::new(callbacks.clone())));
            }
        }
        let program = match base {
            Some(base) if callbacks.len() == inherited => base,
            _ => Arc::new(Program::new(callbacks)),
        };
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

    /// The public instance methods of the openings of a class. The index keeps the result.
    fn rails_visibility(&self, shared: &RailsShared, identity: &str) -> Arc<Visibility> {
        if let Some(found) = shared.visibility.lock().get(identity) {
            return Arc::clone(found);
        }
        let mut visibility = Visibility::default();
        for (position, index) in shared.classes.get(identity).into_iter().flatten() {
            self.with_class_body(*position, *index, |reader, node| {
                reader.read_visibility(node, *position, &mut visibility);
            });
        }
        let visibility = Arc::new(visibility);
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
    fn read_visibility(&self, class: Node<'_>, position: usize, out: &mut Visibility) {
        struct Def {
            name: String,
            start: usize,
            public: bool,
        }
        let mut cursor = class.walk();
        let statements: Vec<Node<'_>> = match class.child_by_field_name("body") {
            Some(body) => {
                let mut body_cursor = body.walk();
                body.named_children(&mut body_cursor).collect()
            }
            None => class.named_children(&mut cursor).collect(),
        };
        let mut defs: Vec<Def> = Vec::new();
        let mut explicit: HashMap<String, bool> = HashMap::new();
        let mut public = true;
        let define = |node: Node<'_>, public: bool, defs: &mut Vec<Def>| {
            if let Some(name) = node.child_by_field_name("name") {
                defs.push(Def {
                    name: self.text(name).to_string(),
                    start: name.start_byte(),
                    public,
                });
            }
        };
        for statement in statements {
            match statement.kind() {
                "method" => define(statement, public, &mut defs),
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
                            "method" => define(argument, makes_public, &mut defs),
                            "simple_symbol" | "string" => match self.rails_literal(argument) {
                                Some(name) => {
                                    explicit.insert(name, makes_public);
                                }
                                None => out.unreadable = true,
                            },
                            "array" => match self.rails_names(argument) {
                                Some(names) => {
                                    for name in names {
                                        explicit.insert(name, makes_public);
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
        for def in defs {
            if explicit.get(&def.name).copied().unwrap_or(def.public) {
                out.public.insert((position, def.start));
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

/// The callbacks of a chain of classes in the order of Rails, ready to answer for an action.
///
/// A callback replaces the callbacks with its names, and it adds its names when it applies. Thus
/// the last callback of a name decides the guard of that name. Only the lists `only:` and
/// `except:` make the result depend on the action. The program keeps the result for an action that
/// no list names. For another action, it works out again the names of the callbacks that list the
/// action.
struct Program {
    callbacks: Vec<Callback>,
    by_action: HashMap<String, Vec<usize>>,
    by_name: HashMap<String, Vec<usize>>,
    default_slots: HashMap<String, Slot>,
    default_names: Vec<String>,
}

impl Program {
    fn new(callbacks: Vec<Callback>) -> Self {
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
        let mut program = Self {
            callbacks,
            by_action,
            by_name,
            default_slots: HashMap::new(),
            default_names: Vec::new(),
        };
        let names: Vec<String> = program.by_name.keys().cloned().collect();
        for name in names {
            if let Some(slot) = program.slot_of(&name, "") {
                program.default_slots.insert(name, slot);
            }
        }
        let mut slots: Vec<(&str, Slot)> = program
            .default_slots
            .iter()
            .map(|(name, slot)| (name.as_str(), *slot))
            .collect();
        order_slots(&mut slots);
        program.default_names = slots.iter().map(|(name, _)| (*name).to_string()).collect();
        program
    }

    /// The slot of the guard with the name, or `None` when the action has no such guard.
    fn slot_of(&self, name: &str, action: &str) -> Option<Slot> {
        for index in self.by_name.get(name)?.iter().rev() {
            let callback = &self.callbacks[*index];
            let applies = callback.applies_to(action);
            if callback.skip {
                if applies {
                    return None;
                }
            } else {
                return applies.then(|| {
                    let place = callback.names.iter().position(|n| n == name).unwrap_or(0);
                    (*index, place, callback.prepend)
                });
            }
        }
        None
    }

    fn guards_for(&self, action: &str) -> Vec<String> {
        let Some(listing) = self.by_action.get(action) else {
            return self.default_names.clone();
        };
        let affected: HashSet<&str> = listing
            .iter()
            .flat_map(|index| self.callbacks[*index].names.iter().map(String::as_str))
            .collect();
        let mut slots: Vec<(&str, Slot)> = self
            .default_slots
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

/// A symbol of the index: the position of the file, and the position in the symbols of the file.
type Placed = (usize, usize);

/// The class that a short name gives. A name that belongs to two classes is ambiguous.
enum UniqueClass {
    One(String),
    Many,
}

/// The public instance methods of the openings of a class, by the position of the file and the
/// start of the name. `unreadable` tells that omen cannot read the visibility of a method.
#[derive(Default)]
struct Visibility {
    public: HashSet<(usize, usize)>,
    unreadable: bool,
}

/// What the files of the inflections say.
#[derive(Default)]
pub(super) struct Inflections {
    /// The acronyms, by the lower case word.
    acronyms: HashMap<String, String>,
    /// An `acronym` call that omen cannot read, or a `clear` call. The name of a controller is
    /// not known.
    unreadable_acronym: bool,
    /// A call that can change the singular of a word.
    changes_singulars: bool,
}

/// Reads the inflection calls in the code: `acronym` with a string, and the calls that change
/// a singular form.
fn read_inflection_calls(node: Node<'_>, source: &[u8], out: &mut Inflections) {
    if node.kind() == "call" {
        let method = node
            .child_by_field_name("method")
            .map_or("", |method| super::syntax::text(method, source));
        let has_receiver = node.child_by_field_name("receiver").is_some();
        match method {
            "acronym" => {
                let mut cursor = node.walk();
                let arguments: Vec<Node<'_>> = node
                    .child_by_field_name("arguments")
                    .map(|list| list.named_children(&mut cursor).collect())
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
            "clear" if has_receiver => {
                out.unreadable_acronym = true;
                out.changes_singulars = true;
            }
            "irregular" | "uncountable" | "human" => out.changes_singulars = true,
            "singular" | "plural" if has_receiver => out.changes_singulars = true,
            _ => {}
        }
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    for child in children {
        read_inflection_calls(child, source, out);
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
    callbacks: Mutex<HashMap<Placed, Arc<Vec<Callback>>>>,
    visibility: Mutex<HashMap<String, Arc<Visibility>>>,
    programs: Mutex<HashMap<String, Arc<Program>>>,
    handled: Mutex<HashMap<ActionKey, Handler>>,
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
            callbacks: Mutex::new(HashMap::new()),
            visibility: Mutex::new(HashMap::new()),
            programs: Mutex::new(HashMap::new()),
            handled: Mutex::new(HashMap::new()),
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
