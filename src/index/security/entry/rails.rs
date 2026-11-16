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
//! - A `resources` call with an option that changes the paths in a way that omen
//!   does not read. Examples are `param:`, `shallow:`, `path_names:`, and `singular:`.
//! - A `scope` path with an optional segment, such as `"(:locale)"`.
//! - The calls `concerns`, `draw(:file)`, `mount`, `devise_for`, `direct`, and
//!   `resolve`. These calls give no route that omen reads.
//!
//! A nested route needs the singular name of its parent resource. Omen reads a
//! regular English plural. For an irregular plural, such as `people`, omen skips
//! the nested routes. It keeps the `member` routes and the `collection` routes.
//! A `resource` call uses the plural controller. If the plural is irregular, the
//! route has no symbol.

use tree_sitter::Node;

use super::{join_path, route, Context, Draft, EntryBasis, EntryKind};
use crate::core::Language;
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

/// The singular of a regular English plural, or `None` when the word needs a dictionary.
fn singular_of(plural: &str) -> Option<String> {
    if let Some(stem) = plural.strip_suffix("ies") {
        return (!stem.is_empty()).then(|| format!("{stem}y"));
    }
    let irregular_endings = ["ses", "xes", "zes", "ches", "shes", "ss", "us", "is", "ves"];
    if irregular_endings
        .iter()
        .any(|ending| plural.ends_with(ending))
    {
        return None;
    }
    plural
        .strip_suffix('s')
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
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
                "for" | "while" | "until" | "while_modifier" | "until_modifier" => return None,
                _ => {}
            }
            child = parent;
        }
        let mut scope = Scope {
            prefix: Some(String::from("/")),
            ..Scope::default()
        };
        for frame in frames.into_iter().rev() {
            scope = self.rails_enter(frame, scope)?;
        }
        Some(scope)
    }

    fn rails_enter(&self, call: Node<'_>, mut scope: Scope) -> Option<Scope> {
        let method = self.text(call.child_by_field_name("method")?);
        let receiver = call.child_by_field_name("receiver").is_some();
        if TRANSPARENT_BLOCKS.contains(&method) && (method == "draw" || !receiver) {
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
                let paths = definition.paths(singular, &scope, name);
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
            let paths = definition.paths(singular, scope, name);
            let Some(base) = paths.base else {
                continue;
            };
            let controller = definition.controller_for(singular, name);
            for (action, verb, suffix) in table {
                if !definition.allows(action) {
                    continue;
                }
                let path = format!("{base}{suffix}");
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
                    variant: Some((*action).to_string()),
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

    /// The action symbol and the guards of a controller. `controller` is `admin/users`, and
    /// `modules` are the modules that `namespace` and `scope module:` add.
    fn rails_handler(
        &self,
        modules: &[String],
        controller: &str,
        action: &str,
    ) -> (Option<String>, Vec<String>) {
        let mut segments: Vec<String> = modules.to_vec();
        segments.extend(split_segments(controller));
        let Some(last) = segments.pop() else {
            return (None, Vec::new());
        };
        let class = format!("{}Controller", camelize(&last));
        let container = segments.last().map(|module| camelize(module));
        let symbol = match &container {
            None => self.symbol_named(action, Some(&class)),
            Some(container) => self.rails_module_action(container, &class, action),
        };
        (
            symbol,
            self.rails_guards(&class, container.as_deref(), action),
        )
    }

    /// The action of a controller class that is in a module. The index must give the module as the
    /// container of the class.
    fn rails_module_action(&self, module: &str, class: &str, action: &str) -> Option<String> {
        let mut found = Vec::new();
        for file in self
            .all
            .iter()
            .filter(|file| file.language == Language::Ruby)
        {
            for owner in file.symbols.iter().filter(|symbol| {
                symbol.kind.is_type()
                    && symbol.name == class
                    && symbol.container.as_deref() == Some(module)
            }) {
                found.extend(file.symbols.iter().filter(|symbol| {
                    symbol.is_callable()
                        && symbol.name == action
                        && symbol.container.as_deref() == Some(class)
                        && symbol.range.start_byte >= owner.range.start_byte
                        && symbol.range.end_byte <= owner.range.end_byte
                }));
            }
        }
        (found.len() == 1).then(|| found[0].key.to_string())
    }

    /// The guards of an action: the `before_action` callbacks of the controller class and of
    /// its superclasses in the repository, after the `skip_before_action` calls.
    fn rails_guards(&self, class: &str, container: Option<&str>, action: &str) -> Vec<String> {
        let mut guards = Vec::new();
        for (position, file) in self.all.iter().enumerate() {
            if file.language != Language::Ruby {
                continue;
            }
            for symbol in file.symbols.iter().filter(|symbol| {
                symbol.name == class
                    && symbol.kind.is_type()
                    && container.is_none_or(|wanted| symbol.container.as_deref() == Some(wanted))
            }) {
                for guard in self.rails_chain_guards(position, symbol, action) {
                    if !guards.contains(&guard) {
                        guards.push(guard);
                    }
                }
            }
        }
        guards
    }

    /// The class, then its superclass, as far as the repository has exactly one class for each
    /// name. A qualified superclass name ends the chain.
    fn rails_chain_guards(&self, position: usize, class: &Symbol, action: &str) -> Vec<String> {
        let mut chain = vec![(position, class)];
        while let Some((_, last)) = chain.last() {
            let Some(parent) = last.supertypes.first() else {
                break;
            };
            if parent.contains("::") {
                break;
            }
            let candidates: Vec<(usize, &Symbol)> = self
                .all
                .iter()
                .enumerate()
                .filter(|(_, file)| file.language == Language::Ruby)
                .flat_map(|(position, file)| {
                    file.symbols
                        .iter()
                        .filter(|symbol| symbol.kind.is_type() && symbol.name == *parent)
                        .map(move |symbol| (position, symbol))
                })
                .collect();
            let [(parent_position, parent_symbol)] = candidates.as_slice() else {
                break;
            };
            if chain.iter().any(|(_, seen)| seen.key == parent_symbol.key) {
                break;
            }
            chain.push((*parent_position, parent_symbol));
        }
        let mut active: Vec<String> = Vec::new();
        for (position, symbol) in chain.into_iter().rev() {
            for callback in self.rails_callbacks(position, symbol) {
                if !callback.applies_to(action) {
                    continue;
                }
                if callback.skip {
                    active.retain(|name| !callback.names.contains(name));
                } else {
                    for name in callback.names {
                        if !active.contains(&name) {
                            active.push(name);
                        }
                    }
                }
            }
        }
        active
    }

    /// The callback calls in the body of the class, in source order. A call in a method, or in a
    /// nested class or module, is not a callback of this class.
    fn rails_callbacks(&self, position: usize, class: &Symbol) -> Vec<Callback> {
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
        reader.collect_callbacks(node, &mut callbacks);
        callbacks
    }

    fn collect_callbacks(&self, node: Node<'_>, out: &mut Vec<Callback>) {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        for child in children {
            match child.kind() {
                "method" | "singleton_method" | "class" | "module" | "singleton_class" => {}
                "call" if child.child_by_field_name("receiver").is_none() => {
                    match self.rails_callback(child) {
                        Some(callback) => out.push(callback),
                        None => self.collect_callbacks(child, out),
                    }
                }
                _ => self.collect_callbacks(child, out),
            }
        }
    }

    /// The names, the `only:` and `except:` lists, and the conditions of a callback call.
    /// A list that is not a literal gives no condition, so the callback applies to each action.
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
                .and_then(|value| self.rails_names(value))
        };
        let conditional = options
            .as_ref()
            .is_some_and(|options| options.get("if").is_some() || options.get("unless").is_some());
        Some(Callback {
            skip,
            names,
            only: list("only"),
            except: list("except"),
            conditional,
        })
    }
}

struct Callback {
    skip: bool,
    names: Vec<String>,
    only: Option<Vec<String>>,
    except: Option<Vec<String>>,
    /// An `if:` or `unless:` option. A condition does not decide the action.
    conditional: bool,
}

impl Callback {
    /// A `skip_before_action` call with a condition removes no guard, because omen does not
    /// evaluate the condition.
    fn applies_to(&self, action: &str) -> bool {
        (!self.skip || !self.conditional)
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

    fn paths(&self, singular: bool, scope: &Scope, name: &str) -> ResourceWays {
        let segment = self.path.clone().unwrap_or_else(|| name.to_string());
        let base = append(&scope.prefix, &segment);
        let (member, nested) = if singular {
            (base.clone(), base.clone())
        } else {
            let nested = singular_of(name).and_then(|singular_name| {
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
