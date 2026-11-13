//! Java symbols.

use tree_sitter::Node;

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const IMPORT_DECLARATION: &str = "import_declaration";
const PACKAGE_DECLARATION: &str = "package_declaration";
const SCOPED_IDENTIFIER: &str = "scoped_identifier";
const IDENTIFIER: &str = "identifier";
const ASTERISK: &str = "asterisk";

pub struct Java;

const LAMBDA_EXPRESSION: &str = "lambda_expression";
const VARIABLE_DECLARATOR: &str = "variable_declarator";
const MODIFIERS: &str = "modifiers";
const MARKER_ANNOTATION: &str = "marker_annotation";
const ANNOTATION: &str = "annotation";
const TYPE_IDENTIFIER: &str = "type_identifier";
const SCOPED_TYPE_IDENTIFIER: &str = "scoped_type_identifier";
const TYPE_ARGUMENTS: &str = "type_arguments";
const SUPERCLASS: &str = "superclass";
const SUPER_INTERFACES: &str = "super_interfaces";
const EXTENDS_INTERFACES: &str = "extends_interfaces";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "class_declaration",
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "record_declaration",
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "interface_declaration",
        SymbolKind::Interface,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "annotation_type_declaration",
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
        "compact_constructor_declaration",
        SymbolKind::Constructor,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(LAMBDA_EXPRESSION, SymbolKind::Function, NameRule::Hook),
];

fn modifiers(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let modifiers = node
        .children(&mut cursor)
        .find(|child| child.kind() == MODIFIERS);
    modifiers
}

fn has_modifier(node: Node<'_>, keyword: &str, context: &Context<'_, '_>) -> bool {
    modifiers(node).is_some_and(|modifiers| {
        let mut cursor = modifiers.walk();
        let found = modifiers
            .children(&mut cursor)
            .any(|modifier| context.text(modifier) == keyword);
        found
    })
}

/// The first token of a declaration that is not an annotation.
fn start_after_annotations(node: Node<'_>) -> Option<Node<'_>> {
    let modifiers = modifiers(node)?;
    let is_annotation = |child: &Node<'_>| [MARKER_ANNOTATION, ANNOTATION].contains(&child.kind());
    let mut cursor = modifiers.walk();
    let keyword = modifiers
        .children(&mut cursor)
        .find(|child| !is_annotation(child));
    keyword.or_else(|| modifiers.next_sibling())
}

fn collect_type_names(node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            TYPE_ARGUMENTS => {}
            TYPE_IDENTIFIER | SCOPED_TYPE_IDENTIFIER => out.push(context.text(child)),
            _ => collect_type_names(child, context, out),
        }
    }
}

impl LanguageSpec for Java {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![IMPORT_DECLARATION]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![SCOPED_IDENTIFIER, IDENTIFIER, ASTERISK]
    }

    /// An import of one type or one static member binds its last name. An
    /// on-demand import is a wildcard, and its path has no `.*`.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let Some(path) = qualified_name(node, context) else {
            return;
        };
        let mut cursor = node.walk();
        let on_demand = node
            .children(&mut cursor)
            .any(|child| child.kind() == ASTERISK);
        if on_demand {
            out.push(context.import(node, path, ImportKind::Wildcard, None, None));
        } else {
            let name = path.rsplit('.').next().unwrap_or(&path).to_string();
            out.push(context.import(
                node,
                path,
                ImportKind::Named,
                Some(name.clone()),
                Some(name),
            ));
        }
    }

    fn module_kinds(&self) -> Vec<&'static str> {
        vec![PACKAGE_DECLARATION]
    }

    fn module_name(&self, node: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
        qualified_name(node, context)
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            VARIABLE_DECLARATOR,
            MODIFIERS,
            MARKER_ANNOTATION,
            ANNOTATION,
            TYPE_IDENTIFIER,
            SCOPED_TYPE_IDENTIFIER,
            TYPE_ARGUMENTS,
            SUPERCLASS,
            SUPER_INTERFACES,
            EXTENDS_INTERFACES,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["line_comment", "block_comment"]
    }

    /// A lambda has a name only when a declaration binds it to one variable.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        let declarator = node
            .parent()
            .filter(|parent| parent.kind() == VARIABLE_DECLARATOR)?;
        if declarator.child_by_field_name("value") != Some(node) {
            return None;
        }
        declarator.child_by_field_name("name")
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        let node = draft.node;
        if node.kind() == LAMBDA_EXPRESSION {
            draft.span = Span::between(&draft.name_node, &node);
            return true;
        }
        // The range of a declaration does not include its annotations.
        if let Some(start) = start_after_annotations(node) {
            draft.span = Span::between(&start, &node);
        }
        if draft.kind.is_type() {
            let mut cursor = node.walk();
            for clause in node.children(&mut cursor) {
                if [SUPERCLASS, SUPER_INTERFACES, EXTENDS_INTERFACES].contains(&clause.kind()) {
                    collect_type_names(clause, context, &mut draft.supertypes);
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
        let holder = if draft.node.kind() == LAMBDA_EXPRESSION {
            // The modifiers of a field are on the declaration that contains the declarator.
            draft
                .node
                .parent()
                .and_then(|declarator| declarator.parent())
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
            !has_modifier(holder, "private", context)
        } else {
            has_modifier(holder, "public", context)
        };
        match container {
            _ if !public => Exported::NotExported,
            Some(scope) => scope.exported.clone(),
            None => Exported::Exported,
        }
    }
}

/// The dotted name in a package declaration or an import declaration.
fn qualified_name(declaration: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
    let mut cursor = declaration.walk();
    let name = declaration
        .named_children(&mut cursor)
        .find(|child| [SCOPED_IDENTIFIER, IDENTIFIER].contains(&child.kind()))?;
    Some(context.text(name).split_whitespace().collect())
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_types() {
        let source = "public class Circle extends Base<Double> implements Shape, java.io.Serializable {\n}\nclass Ring {\n}\npublic interface Shape extends Named {\n    double area();\n    private void helper() {}\n}\npublic enum Unit {\n    METRE\n}\npublic record Point(double x) {\n    public Point {\n    }\n}\npublic @interface Marker {\n}\n";
        assert_eq!(
            rows(Language::Java, source),
            [
                "Class Circle - 1-2 exported",
                "Class Ring - 3-4 not_exported",
                "Interface Shape - 5-8 exported",
                "Method area Shape 6-6 exported",
                "Method helper Shape 7-7 not_exported",
                "Enum Unit - 9-11 exported",
                "Class Point - 12-15 exported",
                "Constructor Point Point 13-14 exported",
                "Interface Marker - 16-17 exported",
            ]
        );
        let symbols = symbols(Language::Java, source);
        assert_eq!(
            symbols[0].supertypes,
            ["Base", "Shape", "java.io.Serializable"]
        );
        assert_eq!(symbols[2].supertypes, ["Named"]);
    }

    #[test]
    fn test_members_and_visibility() {
        let source = "public class Circle {\n    public Circle() {\n    }\n\n    @Override\n    public double area() {\n        return 1;\n    }\n\n    private double scaled() {\n        return 1;\n    }\n\n    protected String name() {\n        return \"\";\n    }\n\n    double diameter() {\n        return 2;\n    }\n\n    @Deprecated public static void old() {}\n\n    private static class Counter {\n        public int next() {\n            return 1;\n        }\n    }\n}\n\nclass Ring {\n    public Ring() {}\n    public void turn() {}\n}\n";
        assert_eq!(
            rows(Language::Java, source),
            [
                "Class Circle - 1-29 exported",
                "Constructor Circle Circle 2-3 exported",
                "Method area Circle 6-8 exported",
                "Method scaled Circle 10-12 not_exported",
                "Method name Circle 14-16 not_exported",
                "Method diameter Circle 18-20 not_exported",
                "Method old Circle 22-22 exported",
                "Class Counter Circle 24-28 not_exported",
                "Method next Counter 25-27 not_exported",
                "Class Ring - 31-34 not_exported",
                "Constructor Ring Ring 32-32 not_exported",
                "Method turn Ring 33-33 not_exported",
            ]
        );
        assert_eq!(keys(Language::Java, source)[8], "file#Circle.Counter.next");
    }

    #[test]
    fn test_overloads_get_an_ordinal() {
        let source = "public class Util {\n    public static int twice(int value) { return 1; }\n    public static int twice(double value) { return 2; }\n}\n";
        assert_eq!(
            keys(Language::Java, source),
            ["file#Util", "file#Util.twice", "file#Util.twice~2"]
        );
    }

    #[test]
    fn test_bound_lambdas() {
        let source = "public class Main {\n    public static Runnable task = () -> {};\n    static int run() {\n        IntUnaryOperator add = n -> n + 1;\n        list.stream().map(n -> n);\n        Runnable local = new Runnable() {\n            public void run() {}\n        };\n        return add.applyAsInt(1);\n    }\n}\n";
        assert_eq!(
            rows(Language::Java, source),
            [
                "Class Main - 1-11 exported",
                "Function task Main 2-2 exported",
                "Method run Main 3-10 not_exported",
                "Function add run 4-4 not_exported",
                "Method run run 7-7 not_exported",
            ]
        );
    }

    #[test]
    fn test_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "Main.java",
                "import java.util.List;\nimport java.util.function.*;\nimport shop.shapes.*;\nimport shop.shapes.Circle;\nimport static shop.util.Util.helper;\nimport static shop.util.Util.*;\nimport shop.shapes.Missing;\nimport shop.util.*;\n\npublic class Main {}\n",
            ),
            ("shop/shapes/Circle.java", "package shop.shapes;\n\npublic class Circle {}\n"),
            ("shop/shapes/Shape.java", "package shop.shapes;\n\npublic interface Shape {}\n"),
            (
                "shop/util/Util.java",
                "package shop.util;\n\npublic class Util {\n    public static int helper() { return 1; }\n}\n",
            ),
        ];
        assert_eq!(
            rows(&files, "Main.java"),
            [
                "1 Named java.util.List List->List external",
                "2 Wildcard java.util.function -->- external",
                "3 Wildcard shop.shapes -->- package(shop/shapes/Circle.java,shop/shapes/Shape.java)",
                "4 Named shop.shapes.Circle Circle->Circle shop/shapes/Circle.java [shop/shapes/Circle.java#Circle]",
                "5 Named shop.util.Util.helper helper->helper shop/util/Util.java",
                "6 Wildcard shop.util.Util -->- shop/util/Util.java",
                "7 Named shop.shapes.Missing Missing->Missing unresolved",
                "8 Wildcard shop.util -->- shop/util/Util.java",
            ]
        );
    }

    #[test]
    fn test_two_files_that_declare_one_type_are_ambiguous() {
        use crate::index::imports::testing::rows;
        let files = [
            ("Main.java", "import shop.Circle;\n\nclass Main {}\n"),
            ("a/Circle.java", "package shop;\n\nclass Circle {}\n"),
            ("b/Circle.java", "package shop;\n\nclass Circle {}\n"),
        ];
        assert_eq!(
            rows(&files, "Main.java"),
            ["1 Named shop.Circle Circle->Circle ambiguous(a/Circle.java,b/Circle.java)"]
        );
    }
}
