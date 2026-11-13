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
}
