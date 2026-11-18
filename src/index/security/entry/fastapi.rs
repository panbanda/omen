//! FastAPI routes: the declared dependencies and the request parameters of a handler.
//!
//! The section "Python routes" in `docs/SECURITY.md` lists each form that omen reads and each form
//! that it skips.
//!
//! - Guards. A declared dependency is a guard: `Depends(name)` and `Security(name)` in the
//!   `dependencies` list of a route decorator, of an `APIRouter` or `FastAPI` call, and of an
//!   `include_router` call of the same file, in a parameter default, and in `Annotated`. The
//!   guard is the name of the dependency. It is a declared fact. A dependency such as `get_db`
//!   does not authenticate.
//! - Names. `Depends`, `Security`, `Annotated`, and each framework type must come from an
//!   import. Omen resolves a name in the scope of its use, as Python does. A name with two
//!   bindings in its scope, and a name in a `global` or `nonlocal` statement, give no fact.
//! - Parameters. Each parameter of the handler that is not a dependency and not a framework
//!   object is a request parameter.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use tree_sitter::Node;

use super::{first_reason, Context, GuardsUnknownReason};
use crate::index::model::{ImportBinding, ImportKind};

const FRAMEWORK_MODULES: [&str; 2] = ["fastapi", "starlette"];
const ANNOTATED_MODULES: [&str; 2] = ["typing", "typing_extensions"];
const ROUTERS: [&str; 2] = ["FastAPI", "APIRouter"];
const DEPENDENCY_MARKERS: [&str; 2] = ["Depends", "Security"];
const PARAMETER_MARKERS: [&str; 7] = ["Query", "Path", "Header", "Cookie", "Body", "Form", "File"];
/// The types that FastAPI gives to a handler without a read of a request parameter, and the
/// response classes of the framework, which are subclasses of `Response`.
const FRAMEWORK_OBJECTS: [&str; 14] = [
    "Request",
    "Response",
    "BackgroundTasks",
    "WebSocket",
    "HTTPConnection",
    "SecurityScopes",
    "JSONResponse",
    "HTMLResponse",
    "PlainTextResponse",
    "RedirectResponse",
    "StreamingResponse",
    "FileResponse",
    "ORJSONResponse",
    "UJSONResponse",
];

/// A scope and a name that the scope binds.
type BindingKey<'a> = (usize, &'a str);

#[derive(Default)]
struct Binding<'a> {
    count: usize,
    imported: bool,
    /// The right side of the assignment that binds the name.
    value: Option<Node<'a>>,
}

struct Include<'a> {
    call: Node<'a>,
    scope: usize,
    /// The call is a statement of the body of its scope, so no condition controls it.
    plain: bool,
}

/// The guards that the `include_router` call of a router adds to the routes of the router.
struct Outer {
    /// The start of the `include_router` call. A route that the router gets after the call is
    /// not a part of the call.
    start: usize,
    scope: usize,
    /// The guards of the router that gets the routes, and then the guards of the call.
    guards: Vec<String>,
    /// The guards of the routers around the router that gets the routes. The link shares the
    /// lists of a chain of routers, so a chain costs the sum of its lists and not the square.
    above: Option<Rc<Outer>>,
}

impl Outer {
    /// The guards of each router around the router, from the outer router to the inner one.
    fn guards(&self) -> Vec<String> {
        let mut links = vec![self];
        while let Some(above) = links[links.len() - 1].above.as_deref() {
            links.push(above);
        }
        links
            .into_iter()
            .rev()
            .flat_map(|link| link.guards.iter().cloned())
            .collect()
    }
}

impl Drop for Outer {
    /// Releases a chain of links in a loop. The default drop calls itself for each link, and
    /// a long chain can overflow the stack of the thread.
    fn drop(&mut self) {
        let mut next = self.above.take();
        while let Some(link) = next {
            next = Rc::try_unwrap(link)
                .ok()
                .and_then(|mut link| link.above.take());
        }
    }
}

/// The bindings of a Python file, by scope. A scope is the module, a function, a lambda, a
/// class, or a comprehension. The id of a scope is the id of its node.
#[derive(Default)]
pub(super) struct Scopes<'a> {
    /// The nearest scope around a scope that is not a class. Python does not look for a name
    /// in a class when the use is in a function of the class.
    outward: HashMap<usize, Option<usize>>,
    bindings: HashMap<BindingKey<'a>, Binding<'a>>,
    /// The scope of each decorated definition, by the id of its node.
    sites: HashMap<usize, usize>,
    includes: Vec<Include<'a>>,
    /// The names in a `global` or a `nonlocal` statement.
    rebound: HashSet<&'a str>,
    /// The scope that binds a name for a use in a scope.
    found: RefCell<HashMap<BindingKey<'a>, Option<usize>>>,
    /// The start, the end, and the scope of each import statement, in source order.
    import_statements: Vec<(usize, usize, usize)>,
    /// The import of each local name of a scope. The value is `None` when two imports of the
    /// name in the scope differ.
    imports: OnceCell<HashMap<BindingKey<'a>, Option<&'a ImportBinding>>>,
    /// The positions of the `include_router` calls that include a router, by its binding.
    includes_of: OnceCell<HashMap<BindingKey<'a>, Vec<usize>>>,
    outer: RefCell<HashMap<BindingKey<'a>, Option<Rc<Outer>>>>,
    /// Why the guards of the routers around a router are not complete, by the binding of the
    /// router.
    outer_gaps: RefCell<HashMap<BindingKey<'a>, Option<GuardsUnknownReason>>>,
    /// The file calls `add_middleware` or `middleware`, or it gives `FastAPI` a middleware
    /// argument. Omen does not read a middleware.
    middleware: bool,
}

/// What a parameter of a handler is for FastAPI.
enum Parameter {
    Request,
    /// A dependency, with its name when the name is readable.
    Dependency(Option<String>),
    FrameworkObject,
}

fn in_modules(binding: &ImportBinding, modules: &[&str]) -> bool {
    modules.iter().any(|module| {
        binding.module_path == *module
            || binding
                .module_path
                .strip_prefix(module)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

impl<'a> Context<'a> {
    /// Notes why the guards of the route that omen reads now are not complete.
    fn note_guard_gap(&self, reason: GuardsUnknownReason) {
        self.guard_gap
            .set(first_reason(self.guard_gap.get(), Some(reason)));
    }

    fn scopes(&self) -> &Scopes<'a> {
        self.python_scopes.get_or_init(|| self.read_scopes())
    }

    /// Reads the bindings of the file in one visit. The visit keeps its own stack.
    fn read_scopes(&self) -> Scopes<'a> {
        let root = self.parsed.root_node();
        let mut scopes = Scopes::default();
        scopes.outward.insert(root.id(), None);
        let mut classes: HashSet<usize> = HashSet::new();
        // The scope around a comprehension that is not a comprehension. A target of a
        // comprehension binds in the comprehension. A `:=` in it binds in the scope around.
        let mut hosts: HashMap<usize, usize> = HashMap::new();
        let mut stack: Vec<(Node<'a>, usize, bool)> = vec![(root, root.id(), true)];
        while let Some((node, scope, plain)) = stack.pop() {
            let kind = node.kind();
            let mut inner = scope;
            match kind {
                "decorated_definition" => {
                    scopes.sites.insert(node.id(), scope);
                }
                "function_definition"
                | "class_definition"
                | "lambda"
                | "list_comprehension"
                | "set_comprehension"
                | "dictionary_comprehension"
                | "generator_expression" => {
                    if let Some(name) = node.child_by_field_name("name") {
                        self.bind(&mut scopes, scope, name, None, false);
                    }
                    inner = node.id();
                    if kind.ends_with("comprehension") || kind == "generator_expression" {
                        let host = hosts.get(&scope).copied().unwrap_or(scope);
                        hosts.insert(inner, host);
                    }
                    let outward = if classes.contains(&scope) {
                        scopes.outward[&scope]
                    } else {
                        Some(scope)
                    };
                    scopes.outward.insert(inner, outward);
                    if kind == "class_definition" {
                        classes.insert(inner);
                    }
                }
                "parameters" | "lambda_parameters" => {
                    for parameter in named_children(node) {
                        let name = match parameter.kind() {
                            "default_parameter" | "typed_default_parameter" => {
                                parameter.child_by_field_name("name")
                            }
                            "typed_parameter" => parameter.named_child(0),
                            _ => Some(parameter),
                        };
                        if let Some(name) = name {
                            self.bind_each(&mut scopes, scope, name);
                        }
                    }
                }
                "assignment" | "augmented_assignment" | "type_alias_statement" => {
                    let left = node.child_by_field_name("left");
                    let right = node.child_by_field_name("right");
                    let left = left.map(|left| {
                        if left.kind() == "type" {
                            left.named_child(0).unwrap_or(left)
                        } else {
                            left
                        }
                    });
                    match left {
                        Some(left) if left.kind() == "identifier" => {
                            let value = right.filter(|_| kind != "augmented_assignment");
                            self.bind(&mut scopes, scope, left, value, false);
                        }
                        Some(left) => self.bind_each(&mut scopes, scope, left),
                        None => {}
                    }
                }
                "named_expression" => {
                    if let Some(name) = node.child_by_field_name("name") {
                        let host = hosts.get(&scope).copied().unwrap_or(scope);
                        self.bind(&mut scopes, host, name, None, false);
                    }
                }
                "for_statement" | "for_in_clause" => {
                    if let Some(left) = node.child_by_field_name("left") {
                        self.bind_each(&mut scopes, scope, left);
                    }
                }
                "as_pattern_target" | "case_pattern" => self.bind_each(&mut scopes, scope, node),
                "import_statement" | "import_from_statement" | "future_import_statement" => {
                    scopes
                        .import_statements
                        .push((node.start_byte(), node.end_byte(), scope));
                    let mut cursor = node.walk();
                    let names: Vec<Node<'a>> =
                        node.children_by_field_name("name", &mut cursor).collect();
                    for name in names {
                        let bound = match name.kind() {
                            "aliased_import" => name.child_by_field_name("alias"),
                            // `import a.b` binds `a`. The name of a `from` import has one
                            // identifier.
                            _ => name.named_child(0),
                        };
                        if let Some(bound) = bound {
                            self.bind(&mut scopes, scope, bound, None, true);
                        }
                    }
                }
                "global_statement" | "nonlocal_statement" => {
                    for name in named_children(node) {
                        scopes.rebound.insert(self.text(name));
                    }
                }
                "call" => {
                    let includes = node
                        .child_by_field_name("function")
                        .filter(|function| function.kind() == "attribute")
                        .and_then(|function| function.child_by_field_name("attribute"))
                        .is_some_and(|attribute| self.text(attribute) == "include_router");
                    if includes {
                        scopes.includes.push(Include {
                            call: node,
                            scope,
                            plain,
                        });
                    }
                    scopes.middleware |= node
                        .child_by_field_name("function")
                        .filter(|function| function.kind() == "attribute")
                        .and_then(|function| function.child_by_field_name("attribute"))
                        .is_some_and(|attribute| {
                            matches!(self.text(attribute), "add_middleware" | "middleware")
                        });
                    scopes.middleware |= self.has_middleware_argument(node);
                }
                _ => {}
            }
            let child_plain = match kind {
                "module" | "function_definition" => true,
                "block" | "expression_statement" | "decorated_definition" => plain,
                _ => false,
            };
            let mut cursor = node.walk();
            let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
            for child in children.into_iter().rev() {
                // The name, the decorators, and the return type of a definition belong to the
                // scope around it. Only the parameters and the body are inside.
                let child_scope =
                    if matches!(child.kind(), "parameters" | "lambda_parameters" | "block")
                        || kind == "lambda"
                        || hosts.contains_key(&inner)
                    {
                        inner
                    } else {
                        scope
                    };
                stack.push((child, child_scope, child_plain));
            }
        }
        scopes
    }

    /// Tells if the call is `FastAPI(...)` with a `middleware=` argument that can hold a
    /// middleware: a value other than `None` and an empty list or tuple.
    fn has_middleware_argument(&self, call: Node<'_>) -> bool {
        let is_application = call
            .child_by_field_name("function")
            .is_some_and(|function| match function.kind() {
                "identifier" => self.text(function) == "FastAPI",
                "attribute" => function
                    .child_by_field_name("attribute")
                    .is_some_and(|attribute| self.text(attribute) == "FastAPI"),
                _ => false,
            });
        if !is_application {
            return false;
        }
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return false;
        };
        named_children(arguments).into_iter().any(|argument| {
            argument.kind() == "keyword_argument"
                && argument
                    .child_by_field_name("name")
                    .is_some_and(|name| self.text(name) == "middleware")
                && argument.child_by_field_name("value").is_some_and(|value| {
                    !(value.kind() == "none"
                        || (matches!(value.kind(), "list" | "tuple")
                            && value.named_child_count() == 0))
                })
        })
    }

    fn bind(
        &self,
        scopes: &mut Scopes<'a>,
        scope: usize,
        name: Node<'a>,
        value: Option<Node<'a>>,
        imported: bool,
    ) {
        if name.kind() != "identifier" {
            return;
        }
        let binding = scopes.bindings.entry((scope, self.text(name))).or_default();
        binding.imported = imported && (binding.count == 0 || binding.imported);
        binding.count += 1;
        binding.value = value;
    }

    /// Binds each identifier of a target, as `a, b` or `*rest` has. A target such as `a.b` or
    /// `a[0]` can count a name that it does not bind. Then the name has no known binding, and
    /// omen gives no fact for it.
    fn bind_each(&self, scopes: &mut Scopes<'a>, scope: usize, target: Node<'a>) {
        let mut stack = vec![target];
        while let Some(node) = stack.pop() {
            if node.kind() == "identifier" {
                self.bind(scopes, scope, node, None, false);
            }
            stack.extend(named_children(node));
        }
    }

    /// The one binding of the name that a use in the scope sees.
    fn sole_binding(&self, scope: usize, name: &'a str) -> Option<(usize, &Binding<'a>)> {
        self.bindings_of(scope, name)
            .filter(|(_, binding)| binding.count == 1)
    }

    /// The bindings of the name in the nearest scope that binds it, for a use in the scope.
    fn bindings_of(&self, scope: usize, name: &'a str) -> Option<(usize, &Binding<'a>)> {
        let scopes = self.scopes();
        if scopes.rebound.contains(name) {
            return None;
        }
        let mut visited = Vec::new();
        let mut current = Some(scope);
        let mut owner = None;
        while let Some(candidate) = current {
            if let Some(known) = scopes.found.borrow().get(&(candidate, name)) {
                owner = *known;
                break;
            }
            if scopes.bindings.contains_key(&(candidate, name)) {
                owner = Some(candidate);
                break;
            }
            visited.push(candidate);
            current = scopes.outward.get(&candidate).copied().flatten();
        }
        let mut found = scopes.found.borrow_mut();
        for candidate in visited {
            found.insert((candidate, name), owner);
        }
        drop(found);
        let owner = owner?;
        let binding = scopes.bindings.get(&(owner, name))?;
        Some((owner, binding))
    }

    /// The import that binds the name for a use in the scope, when no other binding hides it.
    /// Two imports of the same name of the same module, as the branches of a condition have,
    /// are one import.
    fn import_of(&self, scope: usize, name: &'a str) -> Option<&'a ImportBinding> {
        let (owner, binding) = self.bindings_of(scope, name)?;
        if !binding.imported {
            return None;
        }
        let scopes = self.scopes();
        let imports = scopes.imports.get_or_init(|| {
            let mut by_name: HashMap<BindingKey<'a>, Option<&'a ImportBinding>> = HashMap::new();
            for import in &self.file.imports {
                let Some(local) = import.local_name.as_deref() else {
                    continue;
                };
                // The statements are in source order, and no import statement is in another.
                let start = import.range.start_byte;
                let position = scopes
                    .import_statements
                    .partition_point(|(statement, _, _)| *statement <= start);
                let Some((_, end, statement_scope)) = position
                    .checked_sub(1)
                    .map(|position| scopes.import_statements[position])
                    .filter(|(_, end, _)| start < *end)
                else {
                    continue;
                };
                debug_assert!(import.range.end_byte <= end);
                by_name
                    .entry((statement_scope, local))
                    .and_modify(|known| {
                        *known = known.filter(|known| {
                            known.kind == import.kind
                                && known.module_path == import.module_path
                                && known.imported_name == import.imported_name
                        });
                    })
                    .or_insert(Some(import));
            }
            by_name
        });
        imports.get(&(owner, name)).copied().flatten()
    }

    /// The name that an identifier or `module.name` has in one of the modules.
    fn imported_as(&self, scope: usize, node: Node<'_>, modules: &[&str]) -> Option<&'a str> {
        match node.kind() {
            "identifier" => self.imported_name(scope, self.text(node), modules),
            "attribute" => {
                let object = node.child_by_field_name("object")?;
                let attribute = node.child_by_field_name("attribute")?;
                if object.kind() != "identifier" {
                    return None;
                }
                let import = self.import_of(scope, self.text(object))?;
                (import.kind == ImportKind::Module
                    && import.imported_name.as_deref() == Some(import.module_path.as_str())
                    && in_modules(import, modules))
                .then(|| self.text(attribute))
            }
            _ => None,
        }
    }

    fn imported_name(&self, scope: usize, name: &'a str, modules: &[&str]) -> Option<&'a str> {
        let import = self.import_of(scope, name)?;
        if import.kind != ImportKind::Named || !in_modules(import, modules) {
            return None;
        }
        import.imported_name.as_deref()
    }

    /// The marker of a call of a name of FastAPI, as `Depends` for `Depends(verify)`.
    fn marker_of(&self, scope: usize, node: Node<'_>) -> Option<&'a str> {
        if node.kind() != "call" {
            return None;
        }
        let function = node.child_by_field_name("function")?;
        self.imported_as(scope, function, &["fastapi"])
    }

    /// The dependency of a `Depends` or a `Security` call. The outer `Option` tells if the call
    /// is a dependency. The inner one is the name, when the dependency is a name or a dotted name.
    fn dependency_of(&self, scope: usize, node: Node<'_>) -> Option<Option<String>> {
        let marker = self.marker_of(scope, node)?;
        if !DEPENDENCY_MARKERS.contains(&marker) {
            return None;
        }
        let arguments = named_children(node.child_by_field_name("arguments")?);
        let keyword = arguments.iter().find_map(|argument| {
            let name = argument.child_by_field_name("name")?;
            (argument.kind() == "keyword_argument" && self.text(name) == "dependency")
                .then(|| argument.child_by_field_name("value"))
                .flatten()
        });
        let first = arguments
            .first()
            .copied()
            .filter(|argument| argument.kind() != "keyword_argument");
        let Some(dependency) = first.or(keyword) else {
            return Some(None);
        };
        let mut current = dependency;
        while current.kind() == "attribute" {
            let (Some(object), Some(_)) = (
                current.child_by_field_name("object"),
                current.child_by_field_name("attribute"),
            ) else {
                return Some(None);
            };
            current = object;
        }
        Some((current.kind() == "identifier").then(|| self.text(dependency).to_string()))
    }

    /// The named dependencies of the literal `dependencies` list of a call.
    fn dependencies_of(&self, scope: usize, call: Node<'_>) -> Vec<String> {
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return Vec::new();
        };
        let Some(list) = named_children(arguments).into_iter().find_map(|argument| {
            let name = argument.child_by_field_name("name")?;
            (argument.kind() == "keyword_argument" && self.text(name) == "dependencies")
                .then(|| argument.child_by_field_name("value"))
                .flatten()
        }) else {
            return Vec::new();
        };
        if !matches!(list.kind(), "list" | "tuple") {
            self.note_guard_gap(GuardsUnknownReason::UnreadableDependency);
            return Vec::new();
        }
        named_children(list)
            .into_iter()
            .filter(|item| item.kind() != "comment")
            .filter_map(|item| {
                let name = self.dependency_of(scope, item).flatten();
                if name.is_none() {
                    self.note_guard_gap(GuardsUnknownReason::UnreadableDependency);
                }
                name
            })
            .collect()
    }

    /// The binding of a router: a name with one binding, an assignment of a `FastAPI` or an
    /// `APIRouter` call. The result has the scope of the binding and the call.
    fn router_of(&self, scope: usize, name: &'a str) -> Option<(BindingKey<'a>, Node<'a>)> {
        let (owner, binding) = self.sole_binding(scope, name)?;
        let call = binding.value.filter(|value| value.kind() == "call")?;
        let marker = self.marker_of(owner, call)?;
        ROUTERS.contains(&marker).then_some(((owner, name), call))
    }

    /// The router that an `include_router` call includes, and the name of the router that
    /// gets it.
    fn include_names(&self, include: &Include<'a>) -> Option<(&'a str, &'a str)> {
        let function = include.call.child_by_field_name("function")?;
        let parent = function.child_by_field_name("object")?;
        let arguments = named_children(include.call.child_by_field_name("arguments")?);
        let keyword = arguments.iter().find_map(|argument| {
            let name = argument.child_by_field_name("name")?;
            (argument.kind() == "keyword_argument" && self.text(name) == "router")
                .then(|| argument.child_by_field_name("value"))
                .flatten()
        });
        let first = arguments
            .first()
            .copied()
            .filter(|argument| argument.kind() != "keyword_argument");
        let child = first.or(keyword)?;
        (parent.kind() == "identifier" && child.kind() == "identifier")
            .then(|| (self.text(parent), self.text(child)))
    }

    fn includes_of(&self) -> &HashMap<BindingKey<'a>, Vec<usize>> {
        let scopes = self.scopes();
        scopes.includes_of.get_or_init(|| {
            let mut by_router: HashMap<BindingKey<'a>, Vec<usize>> = HashMap::new();
            for (position, include) in scopes.includes.iter().enumerate() {
                let Some((_, child)) = self.include_names(include) else {
                    continue;
                };
                if let Some((owner, _)) = self.sole_binding(include.scope, child) {
                    by_router.entry((owner, child)).or_default().push(position);
                }
            }
            by_router
        })
    }

    /// The one `include_router` call of the file that includes the router, when no condition
    /// controls the call, and the binding of the router that gets it.
    fn sole_include(&self, router: BindingKey<'a>) -> Option<(&Include<'a>, &'a str)> {
        let scopes = self.scopes();
        let positions = self.includes_of().get(&router)?;
        let [position] = positions.as_slice() else {
            return None;
        };
        let include = &scopes.includes[*position];
        let (parent, _) = self.include_names(include)?;
        include.plain.then_some((include, parent))
    }

    /// The guards that the routers around a router add. `include_router` copies the routes that
    /// the router has at the time of the call. Thus a router that includes its child after it
    /// was itself included gives the guards of its own `include_router` call to no route of
    /// the child.
    fn outer_guards(&self, router: BindingKey<'a>) -> Option<Rc<Outer>> {
        let scopes = self.scopes();
        let mut chain: Vec<(BindingKey<'a>, Option<BindingKey<'a>>)> = Vec::new();
        let mut seen: HashSet<BindingKey<'a>> = HashSet::new();
        let mut current = Some(router);
        // Routers that include each other give more than one copy of a route. Omen does not
        // know which copy the application serves.
        let mut cyclic = false;
        while let Some(key) = current {
            if scopes.outer.borrow().contains_key(&key) {
                break;
            }
            if !seen.insert(key) {
                cyclic = true;
                break;
            }
            let parent = self.sole_include(key).and_then(|(include, parent)| {
                self.sole_binding(include.scope, parent)
                    .map(|(owner, _)| (owner, parent))
            });
            chain.push((key, parent));
            current = parent;
        }
        for (key, parent) in chain.into_iter().rev() {
            let include = self.sole_include(key).filter(|_| !cyclic);
            // The calls below note a gap for the route that asks. The gap belongs to the
            // router, so it goes to `outer_gaps`, and the note of the route comes back.
            let noted = self.guard_gap.replace(None);
            let included = self.includes_of().get(&key).map_or(0, Vec::len);
            if cyclic || (included > 0 && include.is_none()) {
                self.note_guard_gap(GuardsUnknownReason::UnreadableInclude);
            } else if included == 0 {
                let is_application = scopes
                    .bindings
                    .get(&key)
                    .and_then(|binding| binding.value)
                    .and_then(|call| self.marker_of(key.0, call))
                    == Some("FastAPI");
                if !is_application {
                    self.note_guard_gap(GuardsUnknownReason::IncludedElsewhere);
                }
            }
            let outer = include.and_then(|(include, parent_name)| {
                // A receiver that is not a router of FastAPI can have a method with the same
                // name. Its call adds no guard.
                let Some((owner, call)) = self.router_of(include.scope, parent_name) else {
                    self.note_guard_gap(self.router_gap(include.scope, parent_name));
                    return None;
                };
                let start = include.call.start_byte();
                let parent_outer = parent
                    .and_then(|parent| scopes.outer.borrow().get(&parent).cloned())
                    .flatten();
                let above = parent_outer
                    .clone()
                    .filter(|above| above.start > start && above.scope == include.scope);
                if parent_outer.is_some() && above.is_none() {
                    self.note_guard_gap(GuardsUnknownReason::UnreadableInclude);
                }
                if let Some(gap) = scopes.outer_gaps.borrow().get(&owner).copied().flatten() {
                    self.note_guard_gap(gap);
                }
                let mut guards = self.dependencies_of(owner.0, call);
                guards.extend(self.dependencies_of(include.scope, include.call));
                Some(Rc::new(Outer {
                    start,
                    scope: include.scope,
                    guards,
                    above,
                }))
            });
            scopes.outer.borrow_mut().insert(key, outer);
            let gap = self.guard_gap.replace(noted);
            scopes.outer_gaps.borrow_mut().insert(key, gap);
        }
        if let Some(gap) = scopes.outer_gaps.borrow().get(&router).copied().flatten() {
            self.note_guard_gap(gap);
        }
        scopes.outer.borrow().get(&router).cloned().flatten()
    }

    /// Why omen cannot read the router with the name: an import binds the name, or the name has
    /// no single assignment of a `FastAPI` or an `APIRouter` call.
    fn router_gap(&self, scope: usize, name: &'a str) -> GuardsUnknownReason {
        match self.bindings_of(scope, name) {
            Some((_, binding)) if binding.imported => GuardsUnknownReason::ImportedRouter,
            _ => GuardsUnknownReason::UnresolvedRouter,
        }
    }

    /// The scope of the names in an `Annotated` type, its base type, and its metadata. The type
    /// can be `Annotated[...]` or `typing.Annotated[...]`.
    fn annotated_metadata<'t>(
        &self,
        scope: usize,
        annotation: Node<'t>,
    ) -> Option<(usize, Node<'t>, Vec<Node<'t>>)> {
        let annotation = if annotation.kind() == "type" {
            annotation.named_child(0)?
        } else {
            annotation
        };
        let (head, arguments) = match annotation.kind() {
            "generic_type" => {
                let head = annotation.named_child(0)?;
                let list = annotation.named_child(1)?;
                let arguments = named_children(list)
                    .into_iter()
                    .map(|argument| {
                        if argument.kind() == "type" {
                            argument.named_child(0).unwrap_or(argument)
                        } else {
                            argument
                        }
                    })
                    .collect::<Vec<_>>();
                (head, arguments)
            }
            "subscript" => {
                let head = annotation.child_by_field_name("value")?;
                let mut cursor = annotation.walk();
                let arguments = annotation
                    .children_by_field_name("subscript", &mut cursor)
                    .collect::<Vec<_>>();
                (head, arguments)
            }
            _ => return None,
        };
        if self.imported_as(scope, head, &ANNOTATED_MODULES) != Some("Annotated") {
            return None;
        }
        let mut arguments = arguments.into_iter();
        let base = arguments.next()?;
        Some((scope, base, arguments.collect()))
    }

    /// What FastAPI does with a parameter that has the annotation and the default.
    fn parameter_kind(
        &self,
        scope: usize,
        annotation: Option<Node<'_>>,
        default: Option<Node<'_>>,
    ) -> Parameter {
        if let Some(dependency) = default.and_then(|default| self.dependency_of(scope, default)) {
            return Parameter::Dependency(dependency);
        }
        let inner = annotation.map(|annotation| {
            if annotation.kind() == "type" {
                annotation.named_child(0).unwrap_or(annotation)
            } else {
                annotation
            }
        });
        let Some(inner) = inner else {
            return Parameter::Request;
        };
        let metadata = self.annotated_metadata(scope, inner).or_else(|| {
            if inner.kind() != "identifier" {
                return None;
            }
            let (owner, binding) = self.sole_binding(scope, self.text(inner))?;
            let value = binding.value?;
            let value = if value.kind() == "type" {
                value.named_child(0)?
            } else {
                value
            };
            self.annotated_metadata(owner, value)
        });
        let (scope, inner) = match metadata {
            Some((metadata_scope, base, metadata)) => {
                // FastAPI uses the last of its own annotations.
                let last = metadata.iter().rev().find_map(|item| {
                    let marker = self.marker_of(metadata_scope, *item)?;
                    (DEPENDENCY_MARKERS.contains(&marker) || PARAMETER_MARKERS.contains(&marker))
                        .then_some((*item, marker))
                });
                match last {
                    Some((item, marker)) if DEPENDENCY_MARKERS.contains(&marker) => {
                        return Parameter::Dependency(
                            self.dependency_of(metadata_scope, item).flatten(),
                        );
                    }
                    Some(_) => return Parameter::Request,
                    // Without an annotation of FastAPI, the base type tells what FastAPI does.
                    None => (metadata_scope, base),
                }
            }
            None => (scope, inner),
        };
        let framework_name = match inner.kind() {
            "string" => {
                let name = self.text(inner).trim_matches(['"', '\'']);
                self.imported_name(scope, name, &FRAMEWORK_MODULES)
            }
            _ => self.imported_as(scope, inner, &FRAMEWORK_MODULES),
        };
        if framework_name.is_some_and(|name| FRAMEWORK_OBJECTS.contains(&name)) {
            return Parameter::FrameworkObject;
        }
        Parameter::Request
    }

    /// The guards and the request parameters that FastAPI reads for a route: the dependencies
    /// in the order in which FastAPI adds them to the route, and the parameters of the handler
    /// in the order of the signature.
    pub(super) fn fastapi_declared(
        &self,
        definition: Node<'_>,
        decorator: Node<'_>,
        route_call: Node<'_>,
        receiver: Node<'_>,
    ) -> (Vec<String>, Vec<String>) {
        let scopes = self.scopes();
        let Some(scope) = scopes.sites.get(&definition.id()).copied() else {
            self.note_guard_gap(GuardsUnknownReason::UnresolvedRouter);
            return (Vec::new(), Vec::new());
        };
        if scopes.middleware {
            self.note_guard_gap(GuardsUnknownReason::LayerNotRead);
        }
        if self
            .file
            .imports
            .iter()
            .any(|import| import.kind == ImportKind::Wildcard)
        {
            self.note_guard_gap(GuardsUnknownReason::WildcardImport);
        }
        let mut guards = Vec::new();
        let router = (receiver.kind() == "identifier")
            .then(|| self.router_of(scope, self.text(receiver)))
            .flatten();
        match router {
            Some((router, call)) => {
                if let Some(outer) = self.outer_guards(router) {
                    if outer.start > decorator.start_byte() && outer.scope == scope {
                        guards.extend(outer.guards());
                    } else {
                        self.note_guard_gap(GuardsUnknownReason::UnreadableInclude);
                    }
                }
                guards.extend(self.dependencies_of(router.0, call));
            }
            None if receiver.kind() == "identifier" => {
                self.note_guard_gap(self.router_gap(scope, self.text(receiver)));
            }
            None => self.note_guard_gap(GuardsUnknownReason::UnresolvedRouter),
        }
        guards.extend(self.dependencies_of(scope, route_call));
        let mut parameters = Vec::new();
        let list = definition
            .child_by_field_name("definition")
            .and_then(|function| function.child_by_field_name("parameters"));
        for parameter in list.map(named_children).unwrap_or_default() {
            let (name, annotation, default) = match parameter.kind() {
                "identifier" => (Some(parameter), None, None),
                "typed_parameter" => (
                    parameter.named_child(0),
                    parameter.child_by_field_name("type"),
                    None,
                ),
                "default_parameter" | "typed_default_parameter" => (
                    parameter.child_by_field_name("name"),
                    parameter.child_by_field_name("type"),
                    parameter.child_by_field_name("value"),
                ),
                _ => continue,
            };
            let Some(name) = name.filter(|name| name.kind() == "identifier") else {
                continue;
            };
            match self.parameter_kind(scope, annotation, default) {
                Parameter::Request => parameters.push(self.text(name).to_string()),
                Parameter::Dependency(Some(dependency)) => guards.push(dependency),
                Parameter::Dependency(None) => {
                    self.note_guard_gap(GuardsUnknownReason::UnreadableDependency);
                }
                Parameter::FrameworkObject => {}
            }
        }
        (guards, parameters)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{assert_scales_linearly, entries, entries_lean};
    use crate::core::Language;
    use crate::index::security::entry::{EntryKind, EntryPoint};

    fn routes(source: &str) -> Vec<EntryPoint> {
        entries(Language::Python, &[("main.py", source)])
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::HttpHandler)
            .collect()
    }

    /// The path and the guards of each route of the source.
    fn guards(source: &str) -> Vec<(String, Vec<String>)> {
        routes(source)
            .into_iter()
            .map(|entry| (entry.route.unwrap().path, entry.guards))
            .collect()
    }

    /// The path and the parameters of each route of the source.
    fn parameters(source: &str) -> Vec<(String, Vec<String>)> {
        routes(source)
            .into_iter()
            .map(|entry| {
                let route = entry.route.unwrap();
                (route.path, route.parameters)
            })
            .collect()
    }

    fn expected(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        rows.iter()
            .map(|(path, names)| {
                (
                    path.to_string(),
                    names.iter().map(|name| name.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_dependencies_of_a_route_decorator_are_guards() {
        let source = "from fastapi import APIRouter, Depends, Security\n\nrouter = APIRouter()\n\n@router.post(\n    \"/a\",\n    tags=[\"x\"],\n    dependencies=[Depends(verify_key), Security(verify_scope, scopes=[\"admin\"])],\n)\nasync def create():\n    return 1\n\n@router.get(\"/b\", dependencies=(Depends(auth.verify),))\nasync def read():\n    return 2\n\n@router.get(\"/c\")\nasync def plain():\n    return 3\n";
        assert_eq!(
            guards(source),
            expected(&[
                ("/a", &["verify_key", "verify_scope"]),
                ("/b", &["auth.verify"]),
                ("/c", &[]),
            ])
        );
    }

    #[test]
    fn the_dependencies_of_a_router_or_an_application_guard_each_of_its_routes() {
        let source = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI(dependencies=[Depends(count_request)])\nrouter = APIRouter(prefix=\"/v1\", dependencies=[Depends(verify_key)])\nopen_router = APIRouter()\n\n@app.get(\"/a\")\nasync def a():\n    return 1\n\n@router.get(\"/b\", dependencies=[Depends(verify_admin)])\nasync def b():\n    return 2\n\n@open_router.get(\"/c\")\nasync def c():\n    return 3\n";
        assert_eq!(
            guards(source),
            expected(&[
                ("/a", &["count_request"]),
                ("/b", &["verify_key", "verify_admin"]),
                ("/c", &[]),
            ])
        );
    }

    #[test]
    fn a_router_name_with_two_bindings_gives_no_router_guard() {
        let source = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter(dependencies=[Depends(verify_key)])\nrouter = APIRouter()\n\n@router.get(\"/a\", dependencies=[Depends(verify_admin)])\nasync def a():\n    return 1\n";
        assert_eq!(guards(source), expected(&[("/a", &["verify_admin"])]));
        let rebound = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter(dependencies=[Depends(verify_key)])\n\ndef reset():\n    global router\n    router = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(guards(rebound), expected(&[("/a", &[])]));
    }

    #[test]
    fn a_binding_in_another_scope_does_not_hide_the_router() {
        let source = "from fastapi import APIRouter, Depends, FastAPI\n\nrouter = APIRouter(dependencies=[Depends(verify_key)])\n\ndef attach(router):\n    return router\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\ndef make():\n    app = FastAPI(dependencies=[Depends(count_request)])\n\n    @app.get(\"/b\")\n    async def b():\n        return 2\n\n    return app\n\ndef other():\n    app = FastAPI()\n\n    @app.get(\"/c\")\n    async def c():\n        return 3\n\n    return app\n";
        assert_eq!(
            guards(source),
            expected(&[
                ("/a", &["verify_key"]),
                ("/b", &["count_request"]),
                ("/c", &[]),
            ])
        );
    }

    #[test]
    fn include_router_gives_its_dependencies_to_the_routes_of_a_router_of_the_file() {
        let source = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI(dependencies=[Depends(count_request)])\nparent = APIRouter(dependencies=[Depends(verify_tenant)])\nrouter = APIRouter(dependencies=[Depends(verify_key)])\n\n@router.get(\"/a\", dependencies=[Depends(verify_admin)])\nasync def a():\n    return 1\n\nparent.include_router(router, dependencies=[Depends(verify_plan)])\napp.include_router(parent, prefix=\"/v1\", dependencies=[Depends(verify_region)])\n\n@router.get(\"/late\")\nasync def late():\n    return 2\n";
        assert_eq!(
            guards(source),
            expected(&[
                (
                    "/a",
                    &[
                        "count_request",
                        "verify_region",
                        "verify_tenant",
                        "verify_plan",
                        "verify_key",
                        "verify_admin"
                    ]
                ),
                ("/late", &["verify_key"]),
            ])
        );
    }

    #[test]
    fn an_include_router_call_that_omen_cannot_attribute_gives_no_guard() {
        let twice = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(router, dependencies=[Depends(verify_key)])\napp.include_router(router, prefix=\"/open\")\n";
        assert_eq!(guards(twice), expected(&[("/a", &[])]));
        let conditional = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\nif FLAG:\n    app.include_router(router, dependencies=[Depends(verify_key)])\n";
        assert_eq!(guards(conditional), expected(&[("/a", &[])]));
        let other_scope = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\ndef attach():\n    app.include_router(router, dependencies=[Depends(verify_key)])\n";
        assert_eq!(guards(other_scope), expected(&[("/a", &[])]));
        let parent_first = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nparent = APIRouter()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(parent, dependencies=[Depends(verify_region)])\nparent.include_router(router, dependencies=[Depends(verify_plan)])\n";
        assert_eq!(guards(parent_first), expected(&[("/a", &["verify_plan"])]));
        let cycle = "from fastapi import APIRouter, Depends\n\nfirst = APIRouter()\nsecond = APIRouter()\n\n@first.get(\"/a\")\nasync def a():\n    return 1\n\nsecond.include_router(first, dependencies=[Depends(verify_one)])\nfirst.include_router(second, dependencies=[Depends(verify_two)])\n";
        assert_eq!(guards(cycle), expected(&[("/a", &[])]));
    }

    #[test]
    fn a_parameter_with_a_dependency_default_is_a_guard() {
        let source = "from fastapi import APIRouter, Depends, Security\n\nrouter = APIRouter()\n\n@router.post(\"/a\", dependencies=[Depends(verify_key)])\nasync def a(\n    body: Item,\n    user: User = Depends(verify_key),\n    session=Depends(get_db),\n    scoped: User = Security(verify_scope, scopes=[\"a\"]),\n):\n    return 1\n";
        assert_eq!(
            guards(source),
            expected(&[("/a", &["verify_key", "get_db", "verify_scope"])])
        );
        assert_eq!(parameters(source), expected(&[("/a", &["body"])]));
    }

    #[test]
    fn an_annotated_dependency_is_a_guard() {
        let source = "import typing\nfrom typing import Annotated\nfrom typing_extensions import Annotated as Marked\nfrom fastapi import APIRouter, Depends, Query\n\nrouter = APIRouter()\nUserDep = Annotated[User, Depends(get_user)]\ntype Session = Annotated[Database, Depends(get_db)]\n\n@router.get(\"/a\")\nasync def a(\n    first: Annotated[User, Depends(verify_key)],\n    second: typing.Annotated[User, Depends(verify_scope)],\n    third: Marked[User, Depends(verify_plan)],\n    user: UserDep,\n    session: Session,\n    limit: Annotated[int, Query(le=100)] = 10,\n    mixed: Annotated[int, Depends(skipped), Query()] = 1,\n):\n    return 1\n";
        assert_eq!(
            guards(source),
            expected(&[(
                "/a",
                &[
                    "verify_key",
                    "verify_scope",
                    "verify_plan",
                    "get_user",
                    "get_db"
                ]
            )])
        );
        assert_eq!(parameters(source), expected(&[("/a", &["limit", "mixed"])]));
    }

    #[test]
    fn annotated_needs_the_import_of_typing() {
        let source = "from fastapi import APIRouter, Depends\nfrom mylib import Annotated\n\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a(first: Annotated[User, Depends(verify_key)]):\n    return 1\n";
        assert_eq!(guards(source), expected(&[("/a", &[])]));
        assert_eq!(parameters(source), expected(&[("/a", &["first"])]));
    }

    #[test]
    fn a_dependency_without_a_name_gives_no_guard_and_no_parameter() {
        let source = "from typing import Annotated\nfrom fastapi import APIRouter, Depends\n\nrouter = APIRouter()\n\n@router.get(\"/a\", dependencies=[Depends(RoleChecker(\"admin\")), Depends(lambda: 1), Depends(), COMMON, *MORE, Depends(checks[0])])\nasync def a(\n    other: Annotated[CommonParams, Depends()],\n    commons: CommonParams = Depends(),\n    role: Role = Depends(RoleChecker(\"admin\")),\n):\n    return 1\n\n@router.get(\"/b\", dependencies=COMMON)\nasync def b():\n    return 2\n\n@router.get(\"/c\", dependencies=[Depends(dependency=verify_key, use_cache=False)])\nasync def c():\n    return 3\n";
        assert_eq!(
            guards(source),
            expected(&[("/a", &[]), ("/b", &[]), ("/c", &["verify_key"])])
        );
        assert_eq!(
            parameters(source),
            expected(&[("/a", &[]), ("/b", &[]), ("/c", &[])])
        );
    }

    #[test]
    fn an_alias_import_and_a_module_import_give_the_dependency() {
        let alias = "from fastapi import APIRouter, Depends as D, Security as S\n\nrouter = APIRouter()\n\n@router.get(\"/a\", dependencies=[D(verify_key)])\nasync def a(user: User = S(verify_scope)):\n    return 1\n";
        assert_eq!(
            guards(alias),
            expected(&[("/a", &["verify_key", "verify_scope"])])
        );
        let module = "import fastapi\nimport fastapi as fa\nfrom fastapi.params import Depends\n\napp = fastapi.FastAPI(dependencies=[fa.Depends(count_request)])\n\n@app.get(\"/a\", dependencies=[fastapi.Depends(verify_key)])\nasync def a(user: User = Depends(verify_scope)):\n    return 1\n";
        assert_eq!(
            guards(module),
            expected(&[("/a", &["count_request", "verify_key", "verify_scope"])])
        );
    }

    #[test]
    fn two_imports_of_the_same_name_of_the_same_module_are_one_import() {
        let source = "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from fastapi import APIRouter, Depends\nelse:\n    try:\n        from fastapi import APIRouter, Depends\n    except ImportError:\n        raise\n\nrouter = APIRouter(dependencies=[Depends(count_request)])\n\n@router.get(\"/a\", dependencies=[Depends(verify_key)])\nasync def a():\n    return 1\n";
        assert_eq!(
            guards(source),
            expected(&[("/a", &["count_request", "verify_key"])])
        );
        let different = "from fastapi import APIRouter, Depends\nfrom mylib import Depends\n\nrouter = APIRouter()\n\n@router.get(\"/a\", dependencies=[Depends(verify_key)])\nasync def a():\n    return 1\n";
        assert_eq!(guards(different), expected(&[("/a", &[])]));
    }

    #[test]
    fn a_name_that_is_not_the_import_of_fastapi_is_not_a_dependency() {
        let local = "from fastapi import APIRouter\n\nrouter = APIRouter()\n\ndef Depends(function):\n    return function\n\n@router.get(\"/a\", dependencies=[Depends(verify_key)])\nasync def a(user: User = Depends(verify_key)):\n    return 1\n";
        assert_eq!(guards(local), expected(&[("/a", &[])]));
        assert_eq!(parameters(local), expected(&[("/a", &["user"])]));
        let shadowed = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter()\n\ndef Depends(function):\n    return function\n\n@router.get(\"/a\", dependencies=[Depends(verify_key)])\nasync def a():\n    return 1\n";
        assert_eq!(guards(shadowed), expected(&[("/a", &[])]));
        let other_module = "from fastapi import APIRouter\nfrom mylib import Depends\nimport mylib\n\nrouter = APIRouter()\n\n@router.get(\"/a\", dependencies=[Depends(verify_key), mylib.Depends(verify_key)])\nasync def a():\n    return 1\n";
        assert_eq!(guards(other_module), expected(&[("/a", &[])]));
        let local_scope = "from fastapi import APIRouter, Depends\n\ndef make(Depends):\n    router = APIRouter()\n\n    @router.get(\"/a\", dependencies=[Depends(verify_key)])\n    async def a():\n        return 1\n\n    return router\n";
        assert_eq!(guards(local_scope), expected(&[("/a", &[])]));
    }

    #[test]
    fn a_dependency_in_a_string_or_a_comment_is_not_a_guard() {
        let source = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter()\n\n# dependencies=[Depends(verify_key)]\n@router.get(\"/a\", description=\"dependencies=[Depends(verify_key)]\")\nasync def a(\n    note: str = \"Depends(verify_key)\",  # = Depends(verify_key)\n):\n    \"\"\"dependencies=[Depends(verify_key)]\"\"\"\n    return Depends(verify_key)\n\n@router.get(\"/b\", dependencies=[\"Depends(verify_key)\"])\nasync def b():\n    return 2\n";
        assert_eq!(guards(source), expected(&[("/a", &[]), ("/b", &[])]));
        assert_eq!(
            parameters(source),
            expected(&[("/a", &["note"]), ("/b", &[])])
        );
    }

    #[test]
    fn the_request_parameters_of_a_handler_follow_the_path_parameters() {
        let source = "from typing import Optional\nfrom fastapi import APIRouter, BackgroundTasks, Body, Cookie, File, Form, Header, Path, Query, Request, UploadFile\nfrom fastapi.responses import JSONResponse\nfrom starlette.responses import Response\nfrom starlette.websockets import WebSocket\nimport fastapi\n\nrouter = APIRouter()\n\n@router.put(\"/items/{item_id}/parts/{part}\")\nasync def update(\n    request: Request,\n    part: str,\n    bare,\n    response: Response,\n    tasks: BackgroundTasks,\n    reply: JSONResponse,\n    socket: WebSocket,\n    raw: fastapi.Request,\n    quoted: \"Request\",\n    item_id: int = Path(gt=0),\n    body: Item = Body(...),\n    limit: int = Query(10),\n    agent: Optional[str] = Header(None),\n    session: str = Cookie(None),\n    name: str = Form(...),\n    upload: UploadFile = File(...),\n    flag: bool = False,\n    untyped=None,\n    *args,\n    **kwargs,\n):\n    return 1\n";
        assert_eq!(
            parameters(source),
            expected(&[(
                "/items/{item_id}/parts/{part}",
                &[
                    "item_id", "part", "bare", "body", "limit", "agent", "session", "name",
                    "upload", "flag", "untyped"
                ]
            )])
        );
    }

    #[test]
    fn a_framework_object_needs_the_import_of_the_framework() {
        let source = "from fastapi import APIRouter\nfrom models import Response\n\nrouter = APIRouter()\n\nclass Request:\n    pass\n\n@router.post(\"/a\")\nasync def a(request: Request, response: Response, tasks: BackgroundTasks):\n    return 1\n";
        assert_eq!(
            parameters(source),
            expected(&[("/a", &["request", "response", "tasks"])])
        );
    }

    #[test]
    fn a_flask_handler_has_only_its_path_parameters() {
        let source = "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/users/<int:user_id>\")\ndef show(user_id, extra=None):\n    return user_id\n";
        assert_eq!(
            parameters(source),
            expected(&[("/users/<int:user_id>", &["user_id"])])
        );
    }

    #[test]
    fn only_a_decorator_below_the_route_decorator_is_a_guard() {
        let flask = "from flask import Flask\nfrom flask_login import login_required\n\napp = Flask(__name__)\n\n@login_required\n@app.route(\"/a\")\ndef a():\n    return 1\n\n@app.route(\"/b\")\n@login_required\ndef b():\n    return 2\n\n@app.route(\"/outer\")\n@login_required\n@app.route(\"/inner\")\ndef c():\n    return 3\n";
        assert_eq!(
            guards(flask),
            expected(&[
                ("/a", &[]),
                ("/b", &["login_required"]),
                ("/outer", &["login_required"]),
                ("/inner", &[]),
            ])
        );
        let fastapi = "from fastapi import FastAPI, Depends\n\napp = FastAPI()\n\n@login_required\n@app.get(\"/a\", dependencies=[Depends(verify_key)])\n@jwt_required()\nasync def a():\n    return 1\n";
        assert_eq!(
            guards(fastapi),
            expected(&[("/a", &["jwt_required", "verify_key"])])
        );
    }

    #[test]
    fn a_django_view_has_the_guards_of_its_decorators() {
        let urls = "from django.urls import path\nfrom . import views\n\nurlpatterns = [\n    path(\"items/<int:pk>/\", views.detail),\n    path(\"open/\", views.open_view),\n]\n";
        let views = "from django.contrib.auth.decorators import login_required, permission_required\nfrom django.views.decorators.http import require_POST\n\n@login_required\n@require_POST\n@permission_required(\"items.change\")\ndef detail(request, pk):\n    return pk\n\n# @login_required\ndef open_view(request):\n    \"\"\"@login_required\"\"\"\n    return 1\n";
        let found = entries(Language::Python, &[("urls.py", urls), ("views.py", views)]);
        let rows: Vec<(String, Vec<String>)> = found
            .into_iter()
            .map(|entry| (entry.route.unwrap().path, entry.guards))
            .collect();
        assert_eq!(
            rows,
            expected(&[
                (
                    "/items/<int:pk>/",
                    &["login_required", "permission_required"]
                ),
                ("/open/", &[]),
            ])
        );
    }

    fn a_chain_of_included_routers_at(count: usize) -> std::time::Duration {
        let mut source = String::from("from fastapi import APIRouter, Depends\n\n");
        for number in 0..count {
            source.push_str(&format!(
                "r{number} = APIRouter(dependencies=[Depends(d{number})])\n\n@r{number}.get(\"/p{number}\")\nasync def h{number}(user: User = Depends(u{number})):\n    return 1\n\n"
            ));
        }
        for number in 1..count {
            source.push_str(&format!("r{number}.include_router(r{})\n", number - 1));
        }
        let (found, elapsed) = entries_lean(Language::Python, &[("main.py", &source)]);
        assert_eq!(found.len(), count);
        for (number, entry) in found.iter().enumerate() {
            assert_eq!(entry.guards.len(), count - number + 1, "{number}");
            assert_eq!(entry.guards[0], format!("d{}", count - 1));
            assert_eq!(entry.guards[count - number], format!("u{number}"));
        }
        elapsed
    }

    #[test]
    fn a_chain_of_included_routers_gives_each_guard() {
        a_chain_of_included_routers_at(20);
    }

    #[test]
    fn a_qualified_django_view_is_the_function_of_the_imported_module() {
        let urls = "from django.contrib.auth.decorators import login_required\nfrom django.urls import path\nfrom . import views\n\n@login_required\ndef detail(request):\n    return 1\n\nurlpatterns = [\n    path(\"public/\", views.detail),\n    path(\"local/\", detail),\n]\n";
        let views = "def detail(request):\n    return 2\n";
        let found = entries(Language::Python, &[("urls.py", urls), ("views.py", views)]);
        let rows: Vec<(String, Option<String>, Vec<String>)> = found
            .into_iter()
            .map(|entry| (entry.route.unwrap().path, entry.symbol, entry.guards))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "/public/".to_string(),
                    Some("views.py#detail".to_string()),
                    vec![]
                ),
                (
                    "/local/".to_string(),
                    Some("urls.py#detail".to_string()),
                    vec!["login_required".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn a_comprehension_target_does_not_bind_a_name_of_the_module() {
        let source = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter(dependencies=[Depends(verify_key)])\nrouters = [router for router in (router,)]\nnames = {Depends: 1 for Depends in ()}\n\n@router.get(\"/a\", dependencies=[Depends(verify_admin)])\nasync def a():\n    return 1\n";
        assert_eq!(
            guards(source),
            expected(&[("/a", &["verify_key", "verify_admin"])])
        );
        let walrus = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter(dependencies=[Depends(verify_key)])\nrouters = [(router := other) for other in ()]\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(guards(walrus), expected(&[("/a", &[])]));
    }

    #[test]
    fn an_import_in_another_scope_does_not_hide_the_import_of_the_module() {
        let source = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter()\n\ndef helper():\n    from mylib import Depends\n    return Depends\n\n@router.get(\"/a\", dependencies=[Depends(verify_key)])\nasync def a(user: User = Depends(verify_scope)):\n    return 1\n\ndef make():\n    from mylib import Depends\n\n    @router.get(\"/b\", dependencies=[Depends(verify_key)])\n    async def b():\n        return 2\n";
        assert_eq!(
            guards(source),
            expected(&[("/a", &["verify_key", "verify_scope"]), ("/b", &[])])
        );
        assert_eq!(parameters(source), expected(&[("/a", &[]), ("/b", &[])]));
    }

    #[test]
    fn an_include_router_call_needs_a_receiver_that_is_a_router() {
        let source = "from fastapi import APIRouter, Depends\n\nrouter = APIRouter()\nregistry = Registry()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\nregistry.include_router(router, dependencies=[Depends(verify_key)])\n";
        assert_eq!(guards(source), expected(&[("/a", &[])]));
    }

    #[test]
    fn an_annotated_framework_object_is_not_a_request_parameter() {
        let source = "from typing import Annotated\nfrom fastapi import APIRouter, Query, Request\n\nrouter = APIRouter()\nRawRequest = Annotated[Request, \"metadata\"]\n\n@router.get(\"/a\")\nasync def a(\n    request: Annotated[Request, \"metadata\"],\n    raw: RawRequest,\n    item: Annotated[Item, \"metadata\"],\n    limit: Annotated[int, Query()],\n):\n    return 1\n";
        assert_eq!(parameters(source), expected(&[("/a", &["item", "limit"])]));
    }

    /// A chain of routers, each with one dependency and each included in the next one. With
    /// `with_route`, only the innermost router has a route, so the result is one route with one
    /// guard for each router.
    fn a_chain_with_one_route_at(count: usize, with_route: bool) -> std::time::Duration {
        let mut source = String::from("from fastapi import APIRouter, Depends\n\n");
        for number in 0..count {
            source.push_str(&format!(
                "r{number} = APIRouter(dependencies=[Depends(d{number})])\n"
            ));
        }
        if with_route {
            source.push_str("\n@r0.get(\"/p\")\nasync def h():\n    return 1\n\n");
        }
        for number in 1..count {
            source.push_str(&format!("r{number}.include_router(r{})\n", number - 1));
        }
        let (found, elapsed) = entries_lean(Language::Python, &[("main.py", &source)]);
        assert_eq!(found.len(), usize::from(with_route));
        if let Some(entry) = found.first() {
            assert_eq!(entry.guards.len(), count);
            assert_eq!(entry.guards[0], format!("d{}", count - 1));
            assert_eq!(entry.guards[count - 1], "d0");
        }
        elapsed
    }

    #[test]
    fn a_chain_with_one_route_gives_the_guard_of_each_router() {
        a_chain_with_one_route_at(30, true);
        a_chain_with_one_route_at(30, false);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_chain_of_included_routers_resolves_in_linear_time() {
        assert_scales_linearly("one route", 20_000, |count| {
            a_chain_with_one_route_at(count, true)
        });
        assert_scales_linearly("no route", 20_000, |count| {
            a_chain_with_one_route_at(count, false)
        });
    }

    fn many_routes_of_one_router_at(count: usize) -> std::time::Duration {
        let mut source = String::from(
            "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI(dependencies=[Depends(count_request)])\nrouter = APIRouter(dependencies=[Depends(verify_key)])\n\n",
        );
        for number in 0..count {
            source.push_str(&format!(
                "@router.get(\"/p{number}\", dependencies=[Depends(d{number})])\nasync def h{number}(body: Item, user: User = Depends(u{number})):\n    return 1\n\n"
            ));
        }
        source.push_str("app.include_router(router)\n");
        let (found, elapsed) = entries_lean(Language::Python, &[("main.py", &source)]);
        assert_eq!(found.len(), count);
        assert!(found.iter().all(|entry| entry.guards.len() == 4));
        elapsed
    }

    #[test]
    fn many_routes_of_one_router_give_each_guard() {
        many_routes_of_one_router_at(20);
    }

    fn deeply_nested_functions_at(depth: usize) -> std::time::Duration {
        let mut source =
            String::from("from fastapi import APIRouter, Depends\n\nrouter = APIRouter()\n\n");
        for level in 0..depth {
            let indent = "    ".repeat(level);
            source.push_str(&format!(
                "{indent}@router.get(\"/p{level}\", dependencies=[Depends(verify_key)])\n{indent}async def h{level}():\n"
            ));
        }
        source.push_str(&format!("{}return 1\n", "    ".repeat(depth)));
        let (found, elapsed) = entries_lean(Language::Python, &[("main.py", &source)]);
        assert_eq!(found.len(), depth);
        assert!(found.iter().all(|entry| entry.guards == ["verify_key"]));
        elapsed
    }

    #[test]
    fn deeply_nested_functions_resolve_the_names_of_the_module() {
        deeply_nested_functions_at(40);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_fastapi_dependencies_resolve_in_linear_time() {
        assert_scales_linearly("many routes", 10_000, many_routes_of_one_router_at);
    }
    /// The path of each route of the source, with `complete` or the reason.
    fn states(source: &str) -> Vec<(String, String)> {
        routes(source)
            .into_iter()
            .map(|entry| {
                assert_eq!(entry.guards_complete, entry.guards_unknown_reason.is_none());
                (
                    entry.route.unwrap().path,
                    entry
                        .guards_unknown_reason
                        .map_or("complete", |reason| reason.name())
                        .to_string(),
                )
            })
            .collect()
    }

    fn expected_states(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(path, state)| (path.to_string(), state.to_string()))
            .collect()
    }

    #[test]
    fn the_guards_of_an_application_that_omen_reads_in_full_are_complete() {
        let source = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI(dependencies=[Depends(count_request)])\nrouter = APIRouter(dependencies=[Depends(verify_key)])\n\n@app.get(\"/a\")\nasync def a(user=Depends(current_user)):\n    return 1\n\n@router.get(\"/b\", dependencies=[Depends(verify_admin)])\nasync def b():\n    return 2\n\napp.include_router(router)\n";
        assert_eq!(
            states(source),
            expected_states(&[("/a", "complete"), ("/b", "complete")])
        );
    }

    #[test]
    fn a_dependency_that_omen_cannot_read_makes_the_guards_not_complete() {
        for (decorator, parameters) in [
            ("dependencies=DEPENDENCIES", ""),
            ("dependencies=[Depends(make_guard(\"admin\"))]", ""),
            ("dependencies=[Depends(verify_key), GUARD]", ""),
            ("dependencies=[*GUARDS]", ""),
            ("", "user=Depends(guards[0])"),
            ("", "user=Depends(lambda: 1)"),
        ] {
            let separator = if decorator.is_empty() { "" } else { ", " };
            let source = format!(
                "from fastapi import Depends, FastAPI\n\napp = FastAPI()\n\n@app.get(\"/a\"{separator}{decorator})\nasync def a({parameters}):\n    return 1\n\n@app.get(\"/b\")\nasync def b():\n    return 2\n"
            );
            assert_eq!(
                states(&source),
                expected_states(&[("/a", "unreadable_dependency"), ("/b", "complete")]),
                "{decorator} {parameters}"
            );
        }
        let router = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter(dependencies=GUARDS)\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(router)\n";
        assert_eq!(
            states(router),
            expected_states(&[("/a", "unreadable_dependency")])
        );
        let include = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(router, dependencies=[Depends(pick())])\n";
        assert_eq!(
            states(include),
            expected_states(&[("/a", "unreadable_dependency")])
        );
    }

    #[test]
    fn a_router_that_omen_cannot_follow_makes_the_guards_not_complete() {
        let twice = "from fastapi import APIRouter\n\nrouter = APIRouter()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(
            states(twice),
            expected_states(&[("/a", "unresolved_router")])
        );
        let elsewhere = "from fastapi import APIRouter\n\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(
            states(elsewhere),
            expected_states(&[("/a", "included_elsewhere")])
        );
        let repeated = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(router)\napp.include_router(router, dependencies=[Depends(verify_key)])\n";
        assert_eq!(
            states(repeated),
            expected_states(&[("/a", "unreadable_include")])
        );
        let conditional = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\nif ENABLED:\n    app.include_router(router, dependencies=[Depends(verify_key)])\n";
        assert_eq!(
            states(conditional),
            expected_states(&[("/a", "unreadable_include")])
        );
        let late = "from fastapi import APIRouter, Depends, FastAPI\n\napp = FastAPI()\nrouter = APIRouter()\n\napp.include_router(router, dependencies=[Depends(verify_key)])\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(
            states(late),
            expected_states(&[("/a", "unreadable_include")])
        );
        let parent = "from fastapi import APIRouter, Depends\nfrom .main import app\n\nrouter = APIRouter()\n\n@router.get(\"/a\")\nasync def a():\n    return 1\n\napp.include_router(router)\n";
        assert_eq!(
            states(parent),
            expected_states(&[("/a", "imported_router")])
        );
        let chain = "from fastapi import APIRouter, Depends\n\nouter = APIRouter()\ninner = APIRouter()\n\n@inner.get(\"/a\")\nasync def a():\n    return 1\n\nouter.include_router(inner)\n";
        assert_eq!(
            states(chain),
            expected_states(&[("/a", "included_elsewhere")])
        );
    }

    #[test]
    fn a_wildcard_import_or_a_middleware_makes_the_guards_not_complete() {
        let wildcard = "from fastapi import FastAPI\nfrom .guards import *\n\napp = FastAPI()\n\n@app.get(\"/a\")\nasync def a():\n    return 1\n";
        assert_eq!(
            states(wildcard),
            expected_states(&[("/a", "wildcard_import")])
        );
        for middleware in [
            "app.add_middleware(AuthMiddleware)",
            "@app.middleware(\"http\")\nasync def check(request, call_next):\n    return await call_next(request)",
        ] {
            let source = format!(
                "from fastapi import FastAPI\n\napp = FastAPI()\n\n{middleware}\n\n@app.get(\"/a\")\nasync def a():\n    return 1\n"
            );
            assert_eq!(
                states(&source),
                expected_states(&[("/a", "layer_not_read")]),
                "{middleware}"
            );
        }
    }

    #[test]
    fn a_middleware_argument_of_the_application_makes_the_guards_not_complete() {
        let route = "\n\n@app.get(\"/a\")\nasync def a():\n    return 1\n";
        for (application, state) in [
            (
                "app = FastAPI(middleware=[Middleware(AuthMiddleware)])",
                "layer_not_read",
            ),
            (
                "app = fastapi.FastAPI(title=\"x\", middleware=MIDDLEWARE)",
                "layer_not_read",
            ),
            ("app = FastAPI(middleware=[])", "complete"),
            ("app = FastAPI(middleware=None)", "complete"),
            ("app = FastAPI(title=\"x\")", "complete"),
        ] {
            let source =
                format!("import fastapi\nfrom fastapi import FastAPI\n\n{application}{route}");
            assert_eq!(
                states(&source),
                expected_states(&[("/a", state)]),
                "{application}"
            );
        }
    }
}
