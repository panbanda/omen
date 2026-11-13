//! C++ symbols.

use tree_sitter::Node;

use super::c;
use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, Linked, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::ImportBinding;

pub struct Cpp;

const CLASS_SPECIFIER: &str = "class_specifier";
const NAMESPACE_DEFINITION: &str = "namespace_definition";
const LAMBDA_EXPRESSION: &str = "lambda_expression";
const INIT_DECLARATOR: &str = "init_declarator";
const QUALIFIED_IDENTIFIER: &str = "qualified_identifier";
const FIELD_IDENTIFIER: &str = "field_identifier";
const DESTRUCTOR_NAME: &str = "destructor_name";
const OPERATOR_NAME: &str = "operator_name";
const TEMPLATE_TYPE: &str = "template_type";
const TEMPLATE_ARGUMENT_LIST: &str = "template_argument_list";
const ACCESS_SPECIFIER: &str = "access_specifier";
const FRIEND_DECLARATION: &str = "friend_declaration";
const BASE_CLASS_CLAUSE: &str = "base_class_clause";
const FIELD_DECLARATION_LIST: &str = "field_declaration_list";
const PARAMETER_KINDS: &[&str] = &[
    "parameter_declaration",
    "optional_parameter_declaration",
    "variadic_parameter_declaration",
];

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(CLASS_SPECIFIER, SymbolKind::Class, NameRule::Field("name")),
    DefinitionRule::new(
        NAMESPACE_DEFINITION,
        SymbolKind::Module,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "alias_declaration",
        SymbolKind::TypeAlias,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(LAMBDA_EXPRESSION, SymbolKind::Function, NameRule::Hook),
];

const FUNCTION_NAME_KINDS: &[&str] = &[
    c::IDENTIFIER,
    FIELD_IDENTIFIER,
    DESTRUCTOR_NAME,
    OPERATOR_NAME,
    QUALIFIED_IDENTIFIER,
];

fn is_function(node: Node<'_>) -> bool {
    [c::FUNCTION_DEFINITION, c::FUNCTION_DECLARATOR].contains(&node.kind())
}

/// The scope names of a qualified name, and the node of its last name.
/// `a::B::run` gives `["a", "B"]` and the node of `run`.
fn split_qualified_name<'tree>(
    mut name: Node<'tree>,
    context: &Context<'_, 'tree>,
) -> (Vec<String>, Node<'tree>) {
    let mut scopes = Vec::new();
    while name.kind() == QUALIFIED_IDENTIFIER {
        if let Some(scope) = name.child_by_field_name("scope") {
            let scope = match scope.kind() {
                TEMPLATE_TYPE => scope.child_by_field_name("name").unwrap_or(scope),
                _ => scope,
            };
            scopes.push(context.text(scope));
        }
        match name.child_by_field_name("name") {
            Some(inner) => name = inner,
            None => break,
        }
    }
    (scopes, name)
}

/// The child of a class body that contains `node`.
fn class_member<'tree>(node: Node<'tree>, class: Node<'tree>) -> Option<Node<'tree>> {
    let body = class.child_by_field_name("body")?;
    let mut member = node;
    while member.parent()? != body {
        member = member.parent()?;
    }
    Some(member)
}

/// The members of a `class` are private until an access specifier. The
/// members of a `struct` or a `union` are public.
fn is_public_member(member: Node<'_>, class: Node<'_>, context: &Context<'_, '_>) -> bool {
    let mut public = class.kind() != CLASS_SPECIFIER;
    let Some(body) = class.child_by_field_name("body") else {
        return public;
    };
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child == member {
            break;
        }
        if child.kind() == ACCESS_SPECIFIER {
            public = context.text(child).starts_with("public");
        }
    }
    public
}

fn exported_if(condition: bool) -> Exported {
    if condition {
        Exported::Exported
    } else {
        Exported::NotExported
    }
}

fn collect_base_names(node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            ACCESS_SPECIFIER | TEMPLATE_ARGUMENT_LIST => {}
            c::TYPE_IDENTIFIER => out.push(context.text(child)),
            QUALIFIED_IDENTIFIER => {
                let (mut names, last) = split_qualified_name(child, context);
                let last = match last.kind() {
                    TEMPLATE_TYPE => last.child_by_field_name("name").unwrap_or(last),
                    _ => last,
                };
                names.push(context.text(last));
                out.push(names.join("::"));
            }
            _ => collect_base_names(child, context, out),
        }
    }
}

/// The kind of the scope that the name of a definition gives.
enum Owner {
    Missing,
    /// The index does not give one answer.
    NotOne,
    Type,
    Namespace,
}

/// Finds the scope that has this full path. Many files can open one
/// namespace, but a type has one definition.
fn owner(symbols: &[Linked<'_>], scope_path: &[String]) -> Owner {
    let Some((name, container_path)) = scope_path.split_last() else {
        return Owner::Missing;
    };
    let mut types = 0;
    let mut namespaces = 0;
    for candidate in symbols {
        if candidate.symbol.name != *name || candidate.facts.container_path != container_path {
            continue;
        }
        match candidate.symbol.kind {
            SymbolKind::Class | SymbolKind::Struct => types += 1,
            SymbolKind::Module => namespaces += 1,
            _ => {}
        }
    }
    match (types, namespaces) {
        (0, 0) => Owner::Missing,
        (1, 0) => Owner::Type,
        (0, _) => Owner::Namespace,
        _ => Owner::NotOne,
    }
}

/// Finds the kind and the visibility on which the declarations of the
/// definition at `position` agree. The error is the start of a reason.
fn declaration(
    symbols: &[Linked<'_>],
    position: usize,
    in_namespace: bool,
) -> Result<(SymbolKind, Exported), &'static str> {
    let definition = &symbols[position];
    let mut found: Option<(SymbolKind, &Exported)> = None;
    for (other, candidate) in symbols.iter().enumerate() {
        let callable = matches!(
            candidate.symbol.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        );
        if other == position
            || !callable
            || candidate.symbol.receiver_type.is_some()
            || candidate.symbol.name != definition.symbol.name
            || candidate.facts.container_path != definition.facts.container_path
            || candidate.facts.parameter_count != definition.facts.parameter_count
        {
            continue;
        }
        let fact = (candidate.symbol.kind, &candidate.symbol.exported);
        match found {
            None => found = Some(fact),
            Some(first) if first == fact => {}
            Some(_) => return Err("the declarations do not agree for"),
        }
    }
    let Some((kind, exported)) = found else {
        return Err("the index has no declaration of");
    };
    if (kind == SymbolKind::Function) != in_namespace {
        return Err("the declaration has an unexpected kind for");
    }
    Ok((kind, exported.clone()))
}

/// The number of parameters that a parameter list declares. `(void)` declares none.
fn parameter_count(parameters: Node<'_>, context: &Context<'_, '_>) -> usize {
    let mut cursor = parameters.walk();
    let declared: Vec<Node<'_>> = parameters
        .named_children(&mut cursor)
        .filter(|child| PARAMETER_KINDS.contains(&child.kind()))
        .collect();
    match declared.as_slice() {
        [only] if context.text(*only) == "void" => 0,
        _ => declared.len(),
    }
}

impl LanguageSpec for Cpp {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![c::PREPROC_INCLUDE]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![c::SYSTEM_LIB_STRING]
    }

    /// `using namespace` and a namespace alias are not imports of a file.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        c::include_binding(node, context, out);
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        let mut definitions = c::DEFINITIONS.to_vec();
        definitions.extend_from_slice(DEFINITIONS);
        definitions
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        let mut kinds = c::HOOK_KINDS.to_vec();
        kinds.extend_from_slice(&[
            c::REFERENCE_DECLARATOR,
            INIT_DECLARATOR,
            QUALIFIED_IDENTIFIER,
            FIELD_IDENTIFIER,
            DESTRUCTOR_NAME,
            OPERATOR_NAME,
            TEMPLATE_TYPE,
            TEMPLATE_ARGUMENT_LIST,
            ACCESS_SPECIFIER,
            FRIEND_DECLARATION,
            BASE_CLASS_CLAUSE,
            FIELD_DECLARATION_LIST,
        ]);
        kinds.extend_from_slice(PARAMETER_KINDS);
        kinds
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        if node.kind() == LAMBDA_EXPRESSION {
            // A lambda has a name only when a declaration binds it to one variable.
            let declarator = node
                .parent()
                .filter(|parent| parent.kind() == INIT_DECLARATOR)?;
            if declarator.child_by_field_name("value") != Some(node) {
                return None;
            }
            return declarator
                .child_by_field_name("declarator")
                .filter(|name| name.kind() == c::IDENTIFIER);
        }
        if is_function(node) {
            // A friend declaration names a function of a different scope.
            let friend = c::function_holder(node)
                .parent()
                .is_some_and(|parent| parent.kind() == FRIEND_DECLARATION);
            if friend {
                return None;
            }
        }
        c::name_node(node, FUNCTION_NAME_KINDS, context)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        if !c::refine(draft) {
            return false;
        }
        let node = draft.node;
        match node.kind() {
            // `class Name;` refers to a class. It does not define the class.
            CLASS_SPECIFIER if draft.body.is_none() => return false,
            LAMBDA_EXPRESSION => draft.span = Span::between(&draft.name_node, &node),
            // `namespace a::b` defines `b` in the scope `a`.
            NAMESPACE_DEFINITION => {
                let mut names: Vec<String> = draft
                    .name
                    .split("::")
                    .map(|name| name.trim().to_string())
                    .collect();
                if let Some(last) = names.pop() {
                    draft.name = last;
                    draft.container_path.extend(names);
                }
            }
            _ => {}
        }
        if draft.kind.is_type() {
            let mut cursor = node.walk();
            for clause in node.children(&mut cursor) {
                if clause.kind() == BASE_CLASS_CLAUSE {
                    collect_base_names(clause, context, &mut draft.supertypes);
                }
            }
        }
        if !is_function(node) {
            return true;
        }
        draft.parameter_count = draft
            .name_node
            .parent()
            .and_then(|declarator| declarator.child_by_field_name("parameters"))
            .map(|parameters| parameter_count(parameters, context));
        if draft.name_node.kind() == QUALIFIED_IDENTIFIER {
            // The definition is not in the body of its class or its namespace.
            // The kind is `Method` until `link` finds that the scope is a namespace.
            let (scopes, name) = split_qualified_name(draft.name_node, context);
            draft.name_node = name;
            draft.name = context.text(name);
            if let Some(owner) = scopes.last() {
                draft.kind = if &draft.name == owner {
                    SymbolKind::Constructor
                } else {
                    SymbolKind::Method
                };
                draft.container = Some(owner.clone());
                draft.receiver_type = Some(scopes.join("::"));
                draft.container_path.extend(scopes);
            }
        } else if let Some(scope) = context.container() {
            if scope.role == ScopeRole::Type && scope.name == draft.name {
                draft.kind = SymbolKind::Constructor;
            }
        }
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let anonymous_namespace = context.scopes.iter().any(|scope| {
            scope.role == ScopeRole::Namespace && scope.exported == Exported::NotExported
        });
        if context.in_function() || anonymous_namespace {
            return Exported::NotExported;
        }
        if draft.receiver_type.is_some() {
            return Exported::Unknown(
                "the declaration in the class or the namespace gives the visibility".to_string(),
            );
        }
        let node = draft.node;
        let holder = match node.kind() {
            LAMBDA_EXPRESSION => node
                .parent()
                .and_then(|declarator| declarator.parent())
                .unwrap_or(node),
            _ => c::function_holder(node),
        };
        match context.scopes.last() {
            Some(scope) if scope.role == ScopeRole::Type => {
                let public = class_member(holder, scope.node)
                    .is_some_and(|member| is_public_member(member, scope.node, context));
                if public {
                    scope.exported.clone()
                } else {
                    Exported::NotExported
                }
            }
            _ if is_function(node) || node.kind() == LAMBDA_EXPRESSION => {
                exported_if(!c::has_storage_class(holder, "static", context))
            }
            _ if node.kind() == NAMESPACE_DEFINITION => Exported::Exported,
            _ => exported_if(c::is_header(context.file)),
        }
    }

    /// A namespace without a name gives internal linkage to its members. It is
    /// not a symbol and not a container.
    fn scope<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        if node.kind() != NAMESPACE_DEFINITION || node.child_by_field_name("name").is_some() {
            return None;
        }
        Some(Scope {
            node,
            name: String::new(),
            role: ScopeRole::Namespace,
            kind: None,
            exported: Exported::NotExported,
            path: context.path(),
        })
    }

    /// A definition such as `int Scope::name() {}` does not tell if `Scope` is
    /// a class or a namespace, and it does not have the access of the member.
    /// The declaration in a different file gives these facts.
    ///
    /// A declaration agrees with a definition when the full scope path, the
    /// name, and the number of parameters are the same. If the index does not
    /// give one answer, the definition keeps its kind and its unknown visibility.
    fn link(&self, symbols: &mut [Linked<'_>]) {
        let mut updates = Vec::new();
        for (position, definition) in symbols.iter().enumerate() {
            let symbol = &definition.symbol;
            if symbol.receiver_type.is_none() || !matches!(symbol.exported, Exported::Unknown(_)) {
                continue;
            }
            let scope_path = &definition.facts.container_path;
            let name = format!("{}::{}", scope_path.join("::"), symbol.name);
            let outcome = match owner(symbols, scope_path) {
                Owner::Missing => Err(format!(
                    "the index has no scope `{}`",
                    scope_path.join("::")
                )),
                Owner::NotOne => Err(format!(
                    "more than one definition has the name `{}`",
                    scope_path.join("::")
                )),
                Owner::Type => declaration(symbols, position, false)
                    .map_err(|problem| format!("{problem} `{name}`")),
                Owner::Namespace => declaration(symbols, position, true)
                    .map_err(|problem| format!("{problem} `{name}`")),
            };
            updates.push((position, outcome));
        }
        for (position, outcome) in updates {
            let symbol = &mut *symbols[position].symbol;
            match outcome {
                Ok((kind, exported)) => {
                    symbol.kind = kind;
                    symbol.exported = exported;
                    if kind == SymbolKind::Function {
                        symbol.receiver_type = None;
                    }
                }
                Err(reason) => symbol.exported = Exported::Unknown(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, linked_files, rows, rows_in, symbols};

    #[test]
    fn test_free_functions_and_linkage() {
        let source = "namespace {\n\nint local() {\n    return 4;\n}\n\n}  // namespace\n\nstatic int twice_local(int value) {\n    return value * 2;\n}\n\nint run() {\n    auto add = [](int n) { return n + 1; };\n    Circle circle(1.0);\n    std::transform(a, b, [](int n) { return n; });\n    return add(1);\n}\n\ntemplate <typename T>\nT largest(T a, T b) {\n    return a;\n}\n\nstd::string describe();\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Function local - 3-5 not_exported",
                "Function twice_local - 9-11 not_exported",
                "Function run - 13-18 exported",
                "Function add run 14-14 not_exported",
                "Function largest - 21-23 exported",
                "Function describe - 25-25 exported",
            ]
        );
    }

    #[test]
    fn test_namespaces_are_modules_and_containers() {
        let source = "namespace other {\n\nint helper();\n\nint only_here() {\n    return 3;\n}\n\nnamespace inner {\nint deep();\n}\n\n}  // namespace other\nnamespace tools = other;\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Module other - 1-13 exported",
                "Function helper other 3-3 exported",
                "Function only_here other 5-7 exported",
                "Module inner other 9-11 exported",
                "Function deep inner 10-10 exported",
            ]
        );
        assert_eq!(keys(Language::Cpp, source)[4], "file#other.inner.deep");
    }

    #[test]
    fn test_class_members_and_access() {
        let source = "class Shape {\npublic:\n    virtual ~Shape() = default;\n    virtual double area() const = 0;\n};\n\nclass Circle : public Shape, private detail::Base<int> {\n    int hidden();\npublic:\n    explicit Circle(double radius);\n    ~Circle() override;\n    double radius() const {\n        return radius_;\n    }\n    explicit Circle(int side) : radius_(side) {\n    }\n    static int count();\n    int operator+(int x) const;\n    friend void peek(const Circle& circle);\n    class Inner {};\n\nprotected:\n    double scaled(Scale factor) const;\n\nprivate:\n    double radius_;\n    using Local = int;\n};\n\nstruct Point {\n    double x;\n    double norm() const;\n};\n\nclass Forward;\n";
        assert_eq!(
            rows_in(Language::Cpp, "shapes.hpp", source),
            [
                "Class Shape - 1-5 exported",
                "Method ~Shape Shape 3-3 exported",
                "Method area Shape 4-4 exported",
                "Class Circle - 7-28 exported",
                "Method hidden Circle 8-8 not_exported",
                "Constructor Circle Circle 10-10 exported",
                "Method ~Circle Circle 11-11 exported",
                "Method radius Circle 12-14 exported",
                "Constructor Circle Circle 15-16 exported",
                "Method count Circle 17-17 exported",
                "Method operator+ Circle 18-18 exported",
                "Class Inner Circle 20-20 exported",
                "Method scaled Circle 23-23 not_exported",
                "TypeAlias Local Circle 27-27 not_exported",
                "Struct Point - 30-33 exported",
                "Method norm Point 32-32 exported",
            ]
        );
        assert_eq!(
            symbols(Language::Cpp, source)[3].supertypes,
            ["Shape", "detail::Base"]
        );
        assert_eq!(
            rows_in(
                Language::Cpp,
                "shapes.cpp",
                "class Local {\npublic:\n    int go();\n};\n"
            ),
            [
                "Class Local - 1-4 not_exported",
                "Method go Local 3-3 not_exported",
            ]
        );
    }

    #[test]
    fn test_types() {
        let source = "using Scale = double;\nenum class Unit {\n    Metre\n};\ntypedef int Old;\ntemplate <typename T>\nclass Box {\n    T value;\n};\n";
        assert_eq!(
            rows_in(Language::Cpp, "types.hpp", source),
            [
                "TypeAlias Scale - 1-1 exported",
                "Enum Unit - 2-4 exported",
                "TypeAlias Old - 5-5 exported",
                "Class Box - 7-9 exported",
            ]
        );
    }

    #[test]
    fn test_out_of_line_definition_without_its_declaration() {
        let source = "Circle::Circle(double radius) : radius_(radius) {\n}\n\nCircle::~Circle() {\n}\n\ndouble Circle::area() const {\n    return 1;\n}\n\nint a::b::deep() {\n    return 1;\n}\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Constructor Circle Circle 1-2 unknown",
                "Method ~Circle Circle 4-5 unknown",
                "Method area Circle 7-9 unknown",
                "Method deep b 11-13 unknown",
            ]
        );
        let symbols = symbols(Language::Cpp, source);
        assert_eq!(symbols[2].receiver_type.as_deref(), Some("Circle"));
        assert_eq!(symbols[3].receiver_type.as_deref(), Some("a::b"));
        assert_eq!(symbols[3].key, "file#a.b.deep");
    }

    const CIRCLE_HEADER: &str = "class Circle {\npublic:\n    explicit Circle(double radius);\n    double area() const;\nprivate:\n    double area(double factor) const;\nprotected:\n    double scaled(double factor) const;\n};\n";
    const CIRCLE_SOURCE: &str = "Circle::Circle(double radius) {\n}\ndouble Circle::area() const {\n    return 1;\n}\ndouble Circle::area(double factor) const {\n    return 2;\n}\ndouble Circle::scaled(double factor) const {\n    return 3;\n}\n";

    #[test]
    fn test_link_gives_the_access_of_the_declaration_with_the_same_parameter_count() {
        let files = linked_files(&[
            (Language::Cpp, "shapes.hpp", CIRCLE_HEADER),
            (Language::Cpp, "shapes.cpp", CIRCLE_SOURCE),
        ]);
        assert_eq!(
            files["shapes.cpp"],
            [
                "Constructor Circle Circle 1-2 exported",
                "Method area Circle 3-5 exported",
                "Method area Circle 6-8 not_exported",
                "Method scaled Circle 9-11 not_exported",
            ]
        );
    }

    #[test]
    fn test_link_does_not_depend_on_the_file_order() {
        let header = (Language::Cpp, "shapes.hpp", CIRCLE_HEADER);
        let source = (Language::Cpp, "shapes.cpp", CIRCLE_SOURCE);
        let namespace = (
            Language::Cpp,
            "util.hpp",
            "namespace util {\nint helper();\n}\n",
        );
        let function = (
            Language::Cpp,
            "util.cpp",
            "int util::helper() {\n    return 1;\n}\n",
        );
        let ordered = linked_files(&[header, source, namespace, function]);
        assert_eq!(
            linked_files(&[function, namespace, source, header]),
            ordered
        );
        assert_eq!(
            linked_files(&[source, function, header, namespace]),
            ordered
        );
        assert_eq!(ordered["util.cpp"], ["Function helper util 1-3 exported"]);
    }

    #[test]
    fn test_link_makes_a_namespace_member_a_function() {
        let header = "namespace util {\nint helper();\nstatic int quiet();\n}\nclass Util {\npublic:\n    static int helper();\n};\n";
        let source = "int util::helper() {\n    return 1;\n}\nint util::quiet() {\n    return 2;\n}\nint Util::helper() {\n    return 5;\n}\nint missing::helper() {\n    return 6;\n}\nint util::absent() {\n    return 7;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "util.hpp", header),
            (Language::Cpp, "util.cpp", source),
        ]);
        assert_eq!(
            files["util.cpp"],
            [
                "Function helper util 1-3 exported",
                "Function quiet util 4-6 not_exported",
                "Method helper Util 7-9 exported",
                "Method helper missing 10-12 unknown",
                "Method absent util 13-15 unknown",
            ]
        );
    }

    #[test]
    fn test_link_uses_the_full_scope_path() {
        let header = "namespace a {\nclass B {\npublic:\n    void run();\n};\n}\nnamespace c {\nclass B {\n    void run();\n};\n}\nnamespace d::e {\nint deep();\n}\n";
        let source = "void a::B::run() {\n}\nvoid c::B::run() {\n}\nvoid B::run() {\n}\nnamespace a {\nvoid B::run() {\n}\n}\nint d::e::deep() {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "b.hpp", header),
            (Language::Cpp, "b.cpp", source),
        ]);
        assert_eq!(
            files["b.cpp"],
            [
                "Method run B 1-2 exported",
                "Method run B 3-4 not_exported",
                "Method run B 5-6 unknown",
                "Module a - 7-10 exported",
                "Method run B 8-9 exported",
                "Function deep e 11-13 exported",
            ]
        );
    }

    #[test]
    fn test_link_abstains_when_declarations_with_the_same_parameter_count_do_not_agree() {
        let header = "class Circle {\npublic:\n    double area(int factor) const;\nprivate:\n    double area(double factor) const;\n};\n";
        let source = "double Circle::area(double factor) const {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "shapes.hpp", header),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(files["shapes.cpp"], ["Method area Circle 1-3 unknown"]);
    }

    #[test]
    fn test_link_abstains_when_two_types_have_the_same_qualified_name() {
        let first = "class Circle {\npublic:\n    double area() const;\n};\n";
        let second = "class Circle {\npublic:\n    double area() const;\n};\n";
        let source = "double Circle::area() const {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "one.hpp", first),
            (Language::Cpp, "two.hpp", second),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(files["shapes.cpp"], ["Method area Circle 1-3 unknown"]);
    }

    #[test]
    fn test_link_abstains_when_a_type_and_a_namespace_have_the_same_qualified_name() {
        let namespace = "namespace shape {\nint area();\n}\n";
        let class = "class shape {\npublic:\n    int area();\n};\n";
        let source = "int shape::area() {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "namespace.hpp", namespace),
            (Language::Cpp, "class.hpp", class),
            (Language::Cpp, "shape.cpp", source),
        ]);
        assert_eq!(files["shape.cpp"], ["Method area shape 1-3 unknown"]);
    }

    #[test]
    fn test_includes() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "main.cpp",
                "#include <vector>\n#include \"shapes.hpp\"\n#include \"sub/util.hpp\"\n#include \"plain.h\"\nusing namespace other;\nnamespace tools = util;\n",
            ),
            ("shapes.hpp", ""),
            ("sub/util.hpp", ""),
            ("sub/util.cpp", "#include \"util.hpp\"\n"),
            ("include/plain.h", ""),
        ];
        assert_eq!(
            rows(&files, "main.cpp"),
            [
                "1 Load vector -->- external",
                "2 Load shapes.hpp -->- shapes.hpp",
                "3 Load sub/util.hpp -->- sub/util.hpp",
                "4 Load plain.h -->- include/plain.h",
            ]
        );
        assert_eq!(
            rows(&files, "sub/util.cpp"),
            ["1 Load util.hpp -->- sub/util.hpp"]
        );
    }
}
