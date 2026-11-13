//! C++ symbols.

use tree_sitter::Node;

use super::c;
use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, Symbol, SymbolKind};

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

/// The last name of `a::b`.
fn last_segment(qualified: &str) -> &str {
    qualified.rsplit("::").next().unwrap_or(qualified)
}

/// Gives the visibility that all the declarations agree on.
fn declared_visibility(name: &str, declarations: &[&Exported]) -> Exported {
    match declarations.split_first() {
        None => Exported::Unknown(format!("the index has no declaration of `{name}`")),
        Some((first, rest)) if rest.iter().all(|other| other == first) => (*first).clone(),
        Some(_) => Exported::Unknown(format!(
            "the declarations of `{name}` do not have the same visibility"
        )),
    }
}

impl LanguageSpec for Cpp {
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
    fn link(&self, symbols: &mut [&mut Symbol]) {
        let mut updates = Vec::new();
        for (position, symbol) in symbols.iter().enumerate() {
            let Some(qualifier) = &symbol.receiver_type else {
                continue;
            };
            if !matches!(symbol.exported, Exported::Unknown(_)) {
                continue;
            }
            let owner = last_segment(qualifier);
            let owner_has_kind = |kinds: &[SymbolKind]| {
                symbols.iter().any(|candidate| {
                    kinds.contains(&candidate.kind) && last_segment(&candidate.name) == owner
                })
            };
            let is_type = owner_has_kind(&[SymbolKind::Class, SymbolKind::Struct]);
            let is_namespace = owner_has_kind(&[SymbolKind::Module]);
            if is_type == is_namespace {
                continue;
            }
            let declarations: Vec<&Exported> = symbols
                .iter()
                .enumerate()
                .filter(|(other, candidate)| {
                    *other != position
                        && candidate.receiver_type.is_none()
                        && candidate.container.as_deref() == Some(owner)
                        && candidate.name == symbol.name
                        && (candidate.kind == SymbolKind::Function) == is_namespace
                })
                .map(|(_, candidate)| &candidate.exported)
                .collect();
            let name = format!("{qualifier}::{}", symbol.name);
            updates.push((
                position,
                is_namespace,
                declared_visibility(&name, &declarations),
            ));
        }
        for (position, is_namespace, exported) in updates {
            let symbol = &mut symbols[position];
            symbol.exported = exported;
            if is_namespace {
                symbol.kind = SymbolKind::Function;
                symbol.receiver_type = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, linked_rows, rows, rows_in, symbols};

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

    #[test]
    fn test_link_gives_the_access_of_the_class_declaration() {
        let header = "class Circle {\npublic:\n    explicit Circle(double radius);\n    double area() const;\n    double area(double factor) const;\nprotected:\n    double scaled(double factor) const;\n};\n";
        let source = "Circle::Circle(double radius) {\n}\ndouble Circle::area() const {\n    return 1;\n}\ndouble Circle::area(double factor) const {\n    return 2;\n}\ndouble Circle::scaled(double factor) const {\n    return 3;\n}\n";
        let rows = linked_rows(&[
            (Language::Cpp, "shapes.hpp", header),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(
            rows[5..],
            [
                "Constructor Circle Circle 1-2 exported",
                "Method area Circle 3-5 exported",
                "Method area Circle 6-8 exported",
                "Method scaled Circle 9-11 not_exported",
            ]
        );
    }

    #[test]
    fn test_link_makes_a_namespace_member_a_function() {
        let header = "namespace util {\nint helper();\nstatic int quiet();\n}\nclass Util {\npublic:\n    static int helper();\n};\n";
        let source = "int util::helper() {\n    return 1;\n}\nint util::quiet() {\n    return 2;\n}\nint Util::helper() {\n    return 5;\n}\nint missing::helper() {\n    return 6;\n}\n";
        let rows = linked_rows(&[
            (Language::Cpp, "util.hpp", header),
            (Language::Cpp, "util.cpp", source),
        ]);
        assert_eq!(
            rows[5..],
            [
                "Function helper util 1-3 exported",
                "Function quiet util 4-6 not_exported",
                "Method helper Util 7-9 exported",
                "Method helper missing 10-12 unknown",
            ]
        );
    }

    #[test]
    fn test_link_keeps_unknown_when_the_declarations_do_not_agree() {
        let header = "class Circle {\npublic:\n    double area() const;\nprivate:\n    double area(double factor) const;\n};\n";
        let source = "double Circle::area() const {\n    return 1;\n}\n";
        let rows = linked_rows(&[
            (Language::Cpp, "shapes.hpp", header),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(rows[3..], ["Method area Circle 1-3 unknown"]);
    }
}
