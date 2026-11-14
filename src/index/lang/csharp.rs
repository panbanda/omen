//! C# symbols.

use tree_sitter::Node;

use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const USING_DIRECTIVE: &str = "using_directive";
const FILE_SCOPED_NAMESPACE_DECLARATION: &str = "file_scoped_namespace_declaration";
const STATIC_KEYWORD: &str = "static";

use crate::index::calls::{Call, CallSyntax, Callee, LocalName};
use crate::index::model::{Receiver, ReferenceKind};

const VARIABLE_DECLARATION: &str = "variable_declaration";
const FIELD_DECLARATION: &str = "field_declaration";
const PROPERTY_DECLARATION: &str = "property_declaration";
const FOREACH_STATEMENT: &str = "foreach_statement";
const CONSTRUCTOR_INITIALIZER: &str = "constructor_initializer";
const IDENTIFIER: &str = "identifier";

const CALL_SYNTAX: CallSyntax = CallSyntax {
    calls: &[("invocation_expression", "function", "arguments")],
    constructions: &[("object_creation_expression", "type", "arguments")],
    members: &[("member_access_expression", "expression", "name")],
    paths: &[],
    names: &[IDENTIFIER],
    self_words: &["this"],
    super_words: &["base"],
    locals: &[],
    parameters: &[("parameter", "name", "type")],
    return_type_field: "returns",
    literals: &[
        ("integer_literal", "int"),
        ("real_literal", "double"),
        ("string_literal", "string"),
        ("character_literal", "char"),
        ("boolean_literal", "bool"),
    ],
};

pub struct CSharp;

const LAMBDA_EXPRESSION: &str = "lambda_expression";
const ANONYMOUS_METHOD_EXPRESSION: &str = "anonymous_method_expression";
const RECORD_DECLARATION: &str = "record_declaration";
const DESTRUCTOR_DECLARATION: &str = "destructor_declaration";
const NAMESPACE_DECLARATION: &str = "namespace_declaration";
const VARIABLE_DECLARATOR: &str = "variable_declarator";
const ATTRIBUTE_LIST: &str = "attribute_list";
const MODIFIER: &str = "modifier";
const BASE_LIST: &str = "base_list";
const GENERIC_NAME: &str = "generic_name";
const STRUCT_KEYWORD: &str = "struct";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "class_declaration",
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        RECORD_DECLARATION,
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "struct_declaration",
        SymbolKind::Struct,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "interface_declaration",
        SymbolKind::Interface,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "enum_declaration",
        SymbolKind::Enum,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "method_declaration",
        SymbolKind::Method,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "constructor_declaration",
        SymbolKind::Constructor,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        DESTRUCTOR_DECLARATION,
        SymbolKind::Method,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "local_function_statement",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(LAMBDA_EXPRESSION, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(
        ANONYMOUS_METHOD_EXPRESSION,
        SymbolKind::Function,
        NameRule::Hook,
    ),
    // Each declarator of a field declaration is one field. `refine` rejects a
    // declarator of a local variable.
    DefinitionRule::new(
        VARIABLE_DECLARATOR,
        SymbolKind::Field,
        NameRule::Field("name"),
    ),
    // A property has a declared type, as a field has.
    DefinitionRule::new(
        PROPERTY_DECLARATION,
        SymbolKind::Field,
        NameRule::Field("name"),
    ),
];

fn is_function_value(node: Node<'_>) -> bool {
    [LAMBDA_EXPRESSION, ANONYMOUS_METHOD_EXPRESSION].contains(&node.kind())
}

fn has_modifier(node: Node<'_>, keywords: &[&str], context: &Context<'_, '_>) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == MODIFIER && keywords.contains(&context.text(child).as_str()));
    found
}

impl LanguageSpec for CSharp {
    fn block_kinds(&self) -> Vec<&'static str> {
        vec!["block"]
    }

    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn call_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().call_kinds();
        kinds.push(CONSTRUCTOR_INITIALIZER);
        kinds
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().local_kinds();
        kinds.extend([VARIABLE_DECLARATION, FOREACH_STATEMENT]);
        kinds
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![
            VARIABLE_DECLARATION,
            CONSTRUCTOR_INITIALIZER,
            FOREACH_STATEMENT,
            FIELD_DECLARATION,
        ]
    }

    /// `: base(...)` after a constructor calls the constructor of the parent
    /// class. `: this(...)` calls a different constructor of the same class.
    fn call<'tree>(&self, node: Node<'tree>, context: &Context<'_, 'tree>) -> Option<Call<'tree>> {
        if node.kind() != CONSTRUCTOR_INITIALIZER {
            return self.call_syntax().call(node, context);
        }
        let mut cursor = node.walk();
        let word = node
            .children(&mut cursor)
            .find(|child| ["base", "this"].contains(&context.text(*child).as_str()))?;
        let receiver = if context.text(word) == "this" {
            Receiver::SelfValue
        } else {
            Receiver::Super
        };
        let mut cursor = node.walk();
        let arguments = node
            .named_children(&mut cursor)
            .last()
            .map_or(0, |arguments| arguments.named_child_count());
        Some(Call {
            callee: Callee {
                name_node: word,
                receiver: Some(receiver),
                qualifier: None,
            },
            kind: ReferenceKind::Call,
            arguments: vec![crate::index::calls::Argument::Other; arguments],
        })
    }

    /// The type of a variable is on the declaration. The value of a
    /// declarator is its last child and has no field name. A field is a local
    /// name of its class.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        if node.kind() == FOREACH_STATEMENT {
            // `foreach (Type name in values)` binds an element of the values.
            // With `var`, the declared type of the values gives the type.
            let field = |field| node.child_by_field_name(field);
            let name = field("left").filter(|name| name.kind() == IDENTIFIER);
            if let (Some(name), Some(values)) = (name, field("right")) {
                out.push(LocalName {
                    name: context.text(name),
                    type_name: field("type").map(|declared| context.text(declared)),
                    value_start: Some(values.start_byte()),
                    is_parameter: false,
                    element: true,
                    result_index: None,
                    visible: None,
                });
            }
            return;
        }
        if node.kind() != VARIABLE_DECLARATION {
            self.call_syntax().locals(node, context, out);
            return;
        }
        let type_name = node
            .child_by_field_name("type")
            .map(|type_name| context.text(type_name));
        let mut cursor = node.walk();
        for declarator in node.named_children(&mut cursor) {
            if declarator.kind() != VARIABLE_DECLARATOR {
                continue;
            }
            let Some(name) = declarator.child_by_field_name("name") else {
                continue;
            };
            let value = declarator
                .named_child(declarator.named_child_count().saturating_sub(1) as u32)
                .filter(|value| *value != name);
            out.push(LocalName {
                name: context.text(name),
                type_name: type_name.clone(),
                value_start: value.map(|value| value.start_byte()),
                is_parameter: false,
                element: false,
                result_index: None,
                visible: None,
            });
        }
    }

    fn import_kinds(&self) -> Vec<&'static str> {
        vec![USING_DIRECTIVE]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![STATIC_KEYWORD]
    }

    /// `using A.B;` and `using static A.B;` make names available without a
    /// qualifier, thus they are wildcards. `using C = A.B;` binds one name.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let alias = node.child_by_field_name("name");
        let mut cursor = node.walk();
        let path = node
            .named_children(&mut cursor)
            .find(|child| Some(*child) != alias);
        let Some(path) = path else {
            return;
        };
        let path: String = context.text(path).split_whitespace().collect();
        match alias {
            Some(alias) => {
                let name = path.rsplit('.').next().unwrap_or(&path).to_string();
                out.push(context.import(
                    node,
                    path,
                    ImportKind::Named,
                    Some(name),
                    Some(context.text(alias)),
                ));
            }
            None => out.push(context.import(node, path, ImportKind::Wildcard, None, None)),
        }
    }

    fn module_kinds(&self) -> Vec<&'static str> {
        vec![NAMESPACE_DECLARATION, FILE_SCOPED_NAMESPACE_DECLARATION]
    }

    /// The full name of a namespace includes the namespaces that contain it.
    fn module_name(&self, node: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
        let mut parts = context.path();
        parts.push(context.text(node.child_by_field_name("name")?));
        Some(parts.join("."))
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            NAMESPACE_DECLARATION,
            VARIABLE_DECLARATOR,
            ATTRIBUTE_LIST,
            MODIFIER,
            BASE_LIST,
            GENERIC_NAME,
            STRUCT_KEYWORD,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    /// A lambda or an anonymous method has a name only when a declaration
    /// binds it to one variable.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        node.parent()
            .filter(|parent| parent.kind() == VARIABLE_DECLARATOR)?
            .child_by_field_name("name")
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        let node = draft.node;
        if node.kind() == VARIABLE_DECLARATOR {
            let declaration = context
                .parent(node)
                .filter(|parent| parent.kind() == VARIABLE_DECLARATION);
            let field = declaration
                .and_then(|declaration| declaration.parent())
                .filter(|field| field.kind() == FIELD_DECLARATION);
            let (Some(declaration), Some(field)) = (declaration, field) else {
                return false;
            };
            // A field whose value is a function value is a function.
            let mut cursor = node.walk();
            if node.named_children(&mut cursor).any(is_function_value) {
                return false;
            }
            draft.return_type = declaration
                .child_by_field_name("type")
                .map(|declared| context.text(declared));
            let mut cursor = field.walk();
            let start = field
                .children(&mut cursor)
                .find(|child| child.kind() != ATTRIBUTE_LIST)
                .unwrap_or(field);
            draft.span = Span::between(&start, &field);
            return true;
        }
        if is_function_value(node) {
            draft.span = Span::between(&draft.name_node, &node);
            return true;
        }
        // The range of a declaration does not include its attributes.
        let mut cursor = node.walk();
        let start = node
            .children(&mut cursor)
            .find(|child| child.kind() != ATTRIBUTE_LIST);
        if let Some(start) = start {
            draft.span = Span::between(&start, &node);
        }
        match node.kind() {
            DESTRUCTOR_DECLARATION => draft.name = format!("~{}", draft.name),
            RECORD_DECLARATION => {
                let mut cursor = node.walk();
                if node
                    .children(&mut cursor)
                    .any(|child| child.kind() == STRUCT_KEYWORD)
                {
                    draft.kind = SymbolKind::Struct;
                }
            }
            _ => {}
        }
        if draft.kind.is_type() {
            let mut cursor = node.walk();
            let bases = node
                .children(&mut cursor)
                .find(|child| child.kind() == BASE_LIST);
            if let Some(bases) = bases {
                let mut cursor = bases.walk();
                for base in bases.named_children(&mut cursor) {
                    let name = match base.kind() {
                        GENERIC_NAME => base.named_child(0).unwrap_or(base),
                        _ => base,
                    };
                    draft.supertypes.push(context.text(name));
                }
            }
        }
        true
    }

    /// `public` exports a declaration when each type that contains it is exported.
    /// A member of an interface is public without the keyword.
    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        if context.in_function() {
            return Exported::NotExported;
        }
        let holder = if is_function_value(draft.node) {
            // The modifiers of a field are on the declaration that contains the declarator.
            let mut holder = draft.node.parent();
            for _ in 0..2 {
                holder = holder.and_then(|node| node.parent());
            }
            holder
        } else if draft.node.kind() == VARIABLE_DECLARATOR {
            // The declarator of a field is in a variable declaration in the field declaration.
            draft
                .node
                .parent()
                .and_then(|declaration| declaration.parent())
        } else {
            Some(draft.node)
        };
        let Some(holder) = holder else {
            return Exported::NotExported;
        };
        let container = context
            .scopes
            .iter()
            .rev()
            .find(|scope| scope.role == ScopeRole::Type);
        let in_interface = container.is_some_and(|scope| scope.kind == Some(SymbolKind::Interface));
        let public = if in_interface {
            !has_modifier(holder, &["private", "protected", "internal"], context)
        } else {
            has_modifier(holder, &["public"], context)
        };
        match container {
            _ if !public => Exported::NotExported,
            Some(scope) => scope.exported.clone(),
            None => Exported::Exported,
        }
    }

    /// A namespace with a body is a part of the key. It is not a symbol.
    fn scope_kinds(&self) -> Vec<&'static str> {
        vec![NAMESPACE_DECLARATION]
    }

    fn scope<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        if node.kind() != NAMESPACE_DECLARATION {
            return None;
        }
        let name = context.text(node.child_by_field_name("name")?);
        let mut path = context.path();
        path.push(name.clone());
        Some(Scope {
            node,
            name,
            role: ScopeRole::Namespace,
            kind: None,
            exported: Exported::Exported,
            path,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_types() {
        let source = "namespace Shop.Shapes;\n\npublic interface IShape\n{\n    double Area();\n    private void Helper() {}\n}\n\n[Obsolete]\npublic class Circle : Base<int>, IShape\n{\n}\n\ninternal class Ring\n{\n}\n\nclass Plain\n{\n}\n\npublic enum Unit\n{\n    Metre\n}\n\npublic struct Point\n{\n}\n\npublic record Size(double Width)\n{\n}\n\npublic record struct Pair(int X);\n";
        assert_eq!(
            rows(Language::CSharp, source),
            [
                "Interface IShape - 3-7 exported",
                "Method Area IShape 5-5 exported",
                "Method Helper IShape 6-6 not_exported",
                "Class Circle - 10-12 exported",
                "Class Ring - 14-16 not_exported",
                "Class Plain - 18-20 not_exported",
                "Enum Unit - 22-25 exported",
                "Struct Point - 27-29 exported",
                "Class Size - 31-33 exported",
                "Struct Pair - 35-35 exported",
            ]
        );
        assert_eq!(
            symbols(Language::CSharp, source)[3].supertypes,
            ["Base", "IShape"]
        );
    }

    #[test]
    fn test_members_and_visibility() {
        let source = "public class Circle\n{\n    public Circle(double radius)\n    {\n    }\n\n    ~Circle()\n    {\n    }\n\n    public double Area()\n    {\n        return 1;\n    }\n\n    private double Scaled() { return 1; }\n\n    protected virtual string Name() { return \"\"; }\n\n    [Obsolete]\n    internal double Diameter()\n    {\n        return 2;\n    }\n\n    static int Local() { return 4; }\n\n    private sealed class Counter\n    {\n        public int Next() { return 1; }\n    }\n}\n\ninternal class Ring\n{\n    public Ring() {}\n}\n";
        assert_eq!(
            rows(Language::CSharp, source),
            [
                "Class Circle - 1-32 exported",
                "Constructor Circle Circle 3-5 exported",
                "Method ~Circle Circle 7-9 not_exported",
                "Method Area Circle 11-14 exported",
                "Method Scaled Circle 16-16 not_exported",
                "Method Name Circle 18-18 not_exported",
                "Method Diameter Circle 21-24 not_exported",
                "Method Local Circle 26-26 not_exported",
                "Class Counter Circle 28-31 not_exported",
                "Method Next Counter 30-30 not_exported",
                "Class Ring - 34-37 not_exported",
                "Constructor Ring Ring 36-36 not_exported",
            ]
        );
    }

    #[test]
    fn test_local_functions_and_bound_lambdas() {
        let source = "public class Program\n{\n    public static Func<int> Task = () => 1;\n\n    static int Run()\n    {\n        int Inner()\n        {\n            return 5;\n        }\n        Func<int, int> add = n => n + 1;\n        var numbers = list.Select(n => n);\n        Func<int> old = delegate { return 1; };\n        return add(Inner());\n    }\n}\n";
        assert_eq!(
            rows(Language::CSharp, source),
            [
                "Class Program - 1-16 exported",
                "Function Task Program 3-3 exported",
                "Method Run Program 5-15 not_exported",
                "Function Inner Run 7-10 not_exported",
                "Function add Run 11-11 not_exported",
                "Function old Run 13-13 not_exported",
            ]
        );
    }

    #[test]
    fn test_namespace_with_a_body_is_a_part_of_the_key() {
        let source = "namespace Shop.Util\n{\n    public static class Helpers\n    {\n        public static int Twice(int value) { return 1; }\n        public static double Twice(double value) { return 2; }\n    }\n}\n";
        assert_eq!(
            rows(Language::CSharp, source),
            [
                "Class Helpers - 3-7 exported",
                "Method Twice Helpers 5-5 exported",
                "Method Twice Helpers 6-6 exported",
            ]
        );
        assert_eq!(
            keys(Language::CSharp, source),
            [
                "file#Shop.Util.Helpers",
                "file#Shop.Util.Helpers.Twice",
                "file#Shop.Util.Helpers.Twice~2"
            ]
        );
    }

    #[test]
    fn test_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "Program.cs",
                "global using System.Collections.Generic;\nusing System;\nusing Shop.Shapes;\nusing Tools = Shop.Util.Helpers;\nusing static Shop.Other.Others;\nusing Shop.Util;\nusing Shop.Util.Deep;\nusing Gone = Shop.Util.Missing;\n\nnamespace Shop;\n\npublic class Program {}\n",
            ),
            ("Shapes/Shapes.cs", "namespace Shop.Shapes;\n\npublic class Circle {}\n"),
            ("Shapes/More.cs", "namespace Shop.Shapes;\n\npublic class Square {}\n"),
            ("Other/Others.cs", "namespace Shop.Other;\n\npublic static class Others {}\n"),
            (
                "Util/Helpers.cs",
                "namespace Shop.Util\n{\n    public static class Helpers {}\n\n    namespace Deep\n    {\n        public class Inner {}\n    }\n}\n",
            ),
        ];
        assert_eq!(
            rows(&files, "Program.cs"),
            [
                "1 Wildcard System.Collections.Generic -->- external",
                "2 Wildcard System -->- external",
                "3 Wildcard Shop.Shapes -->- package(Shapes/More.cs,Shapes/Shapes.cs)",
                "4 Named Shop.Util.Helpers Helpers->Tools Util/Helpers.cs [Util/Helpers.cs#Shop.Util.Helpers]",
                "5 Wildcard Shop.Other.Others -->- Other/Others.cs",
                "6 Wildcard Shop.Util -->- Util/Helpers.cs",
                "7 Wildcard Shop.Util.Deep -->- Util/Helpers.cs",
                "8 Named Shop.Util.Missing Missing->Gone unresolved",
            ]
        );
    }

    #[test]
    fn test_type_in_two_files_is_ambiguous_and_namespace_is_a_package() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "Program.cs",
                "using Circle = Shop.Shapes.Circle;\nusing Shop.Shapes;\nusing static Shop.Shapes.Circle;\n",
            ),
            ("a/Circle.cs", "namespace Shop.Shapes;\n\npublic class Circle {}\n"),
            ("b/Circle.cs", "namespace Shop.Shapes;\n\npublic class Circle {}\n"),
        ];
        assert_eq!(
            rows(&files, "Program.cs"),
            [
                "1 Named Shop.Shapes.Circle Circle->Circle ambiguous(a/Circle.cs,b/Circle.cs)",
                "2 Wildcard Shop.Shapes -->- package(a/Circle.cs,b/Circle.cs)",
                "3 Wildcard Shop.Shapes.Circle -->- ambiguous(a/Circle.cs,b/Circle.cs)",
            ]
        );
    }

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "Program.cs",
            "using System;\nusing Shop.Shapes;\nusing Tools = Shop.Util.Helpers;\nusing static Shop.Other.Others;\n\nnamespace Shop;\n\npublic class Program\n{\n    private Counter counter = new Counter();\n\n    static int Local()\n    {\n        return 4;\n    }\n\n    static int Run()\n    {\n        int Inner()\n        {\n            return 5;\n        }\n        int total = Local() + Tools.Helper() + OnlyHere() + Inner();\n        Func<int, int> add = n => n + Local();\n        return total + add(1);\n    }\n\n    static double Measure(Circle size, IShape shape)\n    {\n        var made = new Circle(1.0);\n        return size.Area() + shape.Area() + made.Area() + Math.Max(1, 2);\n    }\n\n    static int Shadow()\n    {\n        Func<int> Helper = () => 9;\n        return Helper() + Shop.Util.Helpers.Helper();\n    }\n\n    int Tally()\n    {\n        return counter.Next() + Tools.Twice(2) + Tools.Twice(2.0) + Tools.Twice(Local()) + Missing();\n    }\n\n    private sealed class Counter\n    {\n        public int Next()\n        {\n            return 1;\n        }\n    }\n}\n",
        ),
        (
            "Other/Others.cs",
            "namespace Shop.Other;\n\npublic static class Others\n{\n    public static int OnlyHere()\n    {\n        return 3;\n    }\n}\n",
        ),
        (
            "Shapes/Shapes.cs",
            "namespace Shop.Shapes;\n\npublic interface IShape\n{\n    double Area();\n}\n\npublic class Circle : IShape\n{\n    public Circle(double radius)\n    {\n    }\n\n    public double Area()\n    {\n        return this.Scaled() + Edge();\n    }\n\n    private double Scaled()\n    {\n        return 1;\n    }\n\n    protected virtual string Name()\n    {\n        return \"\";\n    }\n\n    internal double Edge()\n    {\n        return 1;\n    }\n}\n\ninternal class Ring : Circle\n{\n    public Ring(double radius) : base(radius)\n    {\n    }\n\n    protected override string Name()\n    {\n        return base.Name();\n    }\n}\n",
        ),
        (
            "Util/Helpers.cs",
            "namespace Shop.Util;\n\npublic static class Helpers\n{\n    public static int Helper()\n    {\n        return 1;\n    }\n\n    public static int Twice(int value)\n    {\n        return value * 2;\n    }\n\n    public static double Twice(double value)\n    {\n        return value * 2;\n    }\n}\n",
        ),
    ];

    #[test]
    fn test_calls_in_the_program_class() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "Program.cs"),
            [
                "10 Program>Counter UniqueCandidate QualifiedPath [Program.cs#Program.Counter]",
                "23 Program.Run>Local UniqueCandidate EnclosingType [Program.cs#Program.Local]",
                "23 Program.Run>Tools.Helper UniqueCandidate QualifiedPath [Util/Helpers.cs#Helpers.Helper]",
                "23 Program.Run>OnlyHere UniqueCandidate ImportedModule [Other/Others.cs#Others.OnlyHere]",
                "23 Program.Run>Inner UniqueCandidate LexicalScope [Program.cs#Program.Run.Inner]",
                "24 Program.Run.add>Local UniqueCandidate EnclosingType [Program.cs#Program.Local]",
                "25 Program.Run>add UniqueCandidate LexicalScope [Program.cs#Program.Run.add]",
                "30 Program.Measure>Circle UniqueCandidate QualifiedPath [Shapes/Shapes.cs#Circle.Circle]",
                "31 Program.Measure>size.Area UniqueCandidate ReceiverDeclaredType [Shapes/Shapes.cs#Circle.Area]",
                "31 Program.Measure>shape.Area Ambiguous NameOnly [Shapes/Shapes.cs#Circle.Area, Shapes/Shapes.cs#IShape.Area]",
                "31 Program.Measure>made.Area UniqueCandidate ReceiverDeclaredType [Shapes/Shapes.cs#Circle.Area]",
                "31 Program.Measure>Math.Max Unresolved - []",
                "37 Program.Shadow>Helper UniqueCandidate LexicalScope [Program.cs#Program.Shadow.Helper]",
                "37 Program.Shadow>Shop.Util.Helpers::Helper UniqueCandidate QualifiedPath [Util/Helpers.cs#Helpers.Helper]",
                "42 Program.Tally>counter.Next UniqueCandidate ReceiverDeclaredType [Program.cs#Program.Counter.Next]",
                "42 Program.Tally>Tools.Twice UniqueCandidate QualifiedPath [Util/Helpers.cs#Helpers.Twice]",
                "42 Program.Tally>Tools.Twice UniqueCandidate QualifiedPath [Util/Helpers.cs#Helpers.Twice~2]",
                "42 Program.Tally>Tools.Twice Ambiguous QualifiedPath [Util/Helpers.cs#Helpers.Twice, Util/Helpers.cs#Helpers.Twice~2]",
                "42 Program.Tally>Local UniqueCandidate EnclosingType [Program.cs#Program.Local]",
                "42 Program.Tally>Missing Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_with_this_and_base() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "Shapes/Shapes.cs"),
            [
                "16 Circle.Area>self.Scaled UniqueCandidate EnclosingType [Shapes/Shapes.cs#Circle.Scaled]",
                "16 Circle.Area>Edge UniqueCandidate EnclosingType [Shapes/Shapes.cs#Circle.Edge]",
                "37 Ring.Ring>super.base UniqueCandidate EnclosingType [Shapes/Shapes.cs#Circle.Circle]",
                "43 Ring.Name>super.Name UniqueCandidate EnclosingType [Shapes/Shapes.cs#Circle.Name]",
            ]
        );
    }
    const FIELD_SOURCE: &str = r#"using System.Collections.Generic;

class Item
{
    public string Name() { return ""; }
    public Item GetOut() { return this; }
}

public class Node
{
    private Node parent;
    public Item item, spare;
    private readonly List<Item> items = new List<Item>();
    private IEnumerable<Item> more;
    private Item[] array;
    public Item Lead { get; set; }
    internal Node Next { get; }
    private object loose;
    System.Action task = () => {};

    string Run(List<Item> given)
    {
        parent.item.Name();
        this.parent.item.Name();
        this.spare.GetOut().Name();
        Lead.Name();
        this.Next.Lead.Name();
        foreach (Item each in items) { each.Name(); }
        foreach (var each in more) { each.Name(); }
        foreach (var each in this.array) { each.Name(); }
        foreach (var each in given) { each.Name(); }
        items.Name();
        this.items.Name();
        loose.Name();
        this.missing.Name();
        parent.missing.Name();
        return "";
    }
}
"#;

    #[test]
    fn test_fields_and_properties_have_the_declared_type() {
        use crate::index::extract::testing::fields;
        assert_eq!(
            fields(Language::CSharp, FIELD_SOURCE),
            [
                "Field parent Node 11-11 not_exported",
                "Field item Node 12-12 exported",
                "Field spare Node 12-12 exported",
                "Field items Node 13-13 not_exported",
                "Field more Node 14-14 not_exported",
                "Field array Node 15-15 not_exported",
                "Field Lead Node 16-16 exported",
                "Field Next Node 17-17 not_exported",
                "Field loose Node 18-18 not_exported",
            ]
        );
        // A field whose value is a lambda stays a function, and a field does
        // not change a key.
        let keys = keys(Language::CSharp, FIELD_SOURCE);
        assert!(keys.contains(&"file#Node.task".to_string()), "{keys:?}");
        assert!(!keys.iter().any(|key| key.contains('~')), "{keys:?}");
    }

    #[test]
    fn test_field_chains_and_loop_variables_give_the_receiver_type() {
        use crate::index::resolve::testing::rows;
        let found: Vec<String> = rows(&[("Node.cs", FIELD_SOURCE)], "Node.cs")
            .into_iter()
            .filter(|row| row.contains("Name ") || row.contains(".GetOut "))
            .map(|row| row.replace("Node.Run>", ""))
            .collect();
        let name = "UniqueCandidate ReceiverDeclaredType [Node.cs#Item.Name]";
        assert_eq!(
            found,
            [
                format!("23 parent.item::Name {name}"),
                format!("24 (..).Name {name}"),
                format!("25 (..).Name {name}"),
                "25 (..).GetOut UniqueCandidate ReceiverDeclaredType [Node.cs#Item.GetOut]"
                    .to_string(),
                format!("26 Lead.Name {name}"),
                format!("27 (..).Name {name}"),
                format!("28 each.Name {name}"),
                format!("29 each.Name {name}"),
                format!("30 each.Name {name}"),
                format!("31 each.Name {name}"),
                // A collection is not an element.
                "32 items.Name Unresolved - []".to_string(),
                "33 (..).Name Unresolved - []".to_string(),
                "34 loose.Name Unresolved - []".to_string(),
                // A hop without a declared type gives candidates and no edge.
                "35 (..).Name Ambiguous NameOnly [Node.cs#Item.Name]".to_string(),
                "36 parent.missing::Name Ambiguous NameOnly [Node.cs#Item.Name]".to_string(),
            ]
        );
    }
}
