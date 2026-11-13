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

use crate::index::calls::{CallSyntax, LocalName};

const LOCAL_VARIABLE_DECLARATION: &str = "local_variable_declaration";
const FIELD_DECLARATION: &str = "field_declaration";
const ENHANCED_FOR_STATEMENT: &str = "enhanced_for_statement";

/// A method call has its object and its name in the call node. Thus the call
/// node is also the callee expression.
const CALL_SYNTAX: CallSyntax = CallSyntax {
    calls: &[
        ("method_invocation", "", "arguments"),
        (
            "explicit_constructor_invocation",
            "constructor",
            "arguments",
        ),
    ],
    constructions: &[("object_creation_expression", "type", "arguments")],
    members: &[
        ("method_invocation", "object", "name"),
        ("field_access", "object", "field"),
    ],
    paths: &[],
    names: &[IDENTIFIER],
    self_words: &["this"],
    super_words: &["super"],
    locals: &[],
    parameters: &[
        ("formal_parameter", "name", "type"),
        ("spread_parameter", "name", "type"),
    ],
    return_type_field: "type",
    literals: &[
        ("decimal_integer_literal", "int"),
        ("hex_integer_literal", "int"),
        ("decimal_floating_point_literal", "double"),
        ("string_literal", "String"),
        ("character_literal", "char"),
        ("true", "boolean"),
        ("false", "boolean"),
    ],
};

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
    // Each declarator of a field declaration is one field. `refine` rejects a
    // declarator of a local variable.
    DefinitionRule::new(
        VARIABLE_DECLARATOR,
        SymbolKind::Field,
        NameRule::Field("name"),
    ),
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
    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().local_kinds();
        kinds.extend([
            LOCAL_VARIABLE_DECLARATION,
            FIELD_DECLARATION,
            ENHANCED_FOR_STATEMENT,
        ]);
        kinds
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![
            LOCAL_VARIABLE_DECLARATION,
            FIELD_DECLARATION,
            ENHANCED_FOR_STATEMENT,
        ]
    }

    /// The type of a variable is on the declaration, and each declarator has
    /// a name and a value. A field is a local name of its class.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        if node.kind() == ENHANCED_FOR_STATEMENT {
            // `for (Type name : values)` binds an element of the values. With
            // `var`, the declared type of the values gives the type.
            let field = |field| node.child_by_field_name(field);
            if let (Some(name), Some(values)) = (field("name"), field("value")) {
                out.push(LocalName {
                    name: context.text(name),
                    type_name: field("type").map(|declared| context.text(declared)),
                    value_start: Some(values.start_byte()),
                    is_parameter: false,
                    element: true,
                    result_index: None,
                });
            }
            return;
        }
        if ![LOCAL_VARIABLE_DECLARATION, FIELD_DECLARATION].contains(&node.kind()) {
            self.call_syntax().locals(node, context, out);
            return;
        }
        let type_name = node
            .child_by_field_name("type")
            .map(|type_name| context.text(type_name));
        let mut cursor = node.walk();
        for declarator in node.children_by_field_name("declarator", &mut cursor) {
            let Some(name) = declarator.child_by_field_name("name") else {
                continue;
            };
            out.push(LocalName {
                name: context.text(name),
                type_name: type_name.clone(),
                value_start: declarator
                    .child_by_field_name("value")
                    .map(|value| value.start_byte()),
                is_parameter: false,
                element: false,
                result_index: None,
            });
        }
    }

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
        if draft.kind == SymbolKind::Field {
            let declaration = context
                .parent(node)
                .filter(|parent| parent.kind() == FIELD_DECLARATION);
            let Some(declaration) = declaration else {
                return false;
            };
            // A field whose value is a lambda is a function.
            let is_lambda = node
                .child_by_field_name("value")
                .is_some_and(|value| value.kind() == LAMBDA_EXPRESSION);
            if is_lambda {
                return false;
            }
            draft.return_type = declaration
                .child_by_field_name("type")
                .map(|declared| context.text(declared));
            let start = start_after_annotations(declaration).unwrap_or(declaration);
            draft.span = Span::between(&start, &declaration);
            return true;
        }
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
        } else if draft.kind == SymbolKind::Field {
            context.parent(draft.node)
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

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "Main.java",
            "import java.util.List;\nimport java.util.function.*;\n\nimport shop.other.*;\nimport shop.shapes.Circle;\nimport shop.shapes.Shape;\nimport shop.util.Util;\n\nimport static shop.util.Util.helper;\n\npublic class Main {\n    private Counter counter = new Counter();\n\n    static int local() {\n        return 4;\n    }\n\n    static int run() {\n        int total = local() + helper() + Other.onlyHere();\n        List<Integer> numbers = List.of(1).stream().map(n -> local()).toList();\n        IntUnaryOperator add = n -> n + 1;\n        return total + add.applyAsInt(numbers.size());\n    }\n\n    static double measure(Circle size, Shape shape) {\n        var made = new Circle(1.0);\n        return size.area() + shape.area() + made.area() + Util.helper();\n    }\n\n    static int shadow() {\n        IntSupplier helper = () -> 9;\n        return helper();\n    }\n\n    int tally() {\n        return counter.next() + this.counter.next() + Math.max(1, 2) + missing();\n    }\n\n    private static class Counter {\n        int next() {\n            return 1;\n        }\n    }\n}\n",
        ),
        (
            "shop/other/Other.java",
            "package shop.other;\n\npublic class Other {\n    public static int onlyHere() {\n        return 3;\n    }\n}\n",
        ),
        (
            "shop/shapes/Circle.java",
            "package shop.shapes;\n\npublic class Circle implements Shape {\n    public Circle(double radius) {\n    }\n\n    public double area() {\n        return this.scaled(1) + edge();\n    }\n\n    private double scaled(double factor) {\n        return factor;\n    }\n\n    protected String name() {\n        return \"circle\";\n    }\n\n    double edge() {\n        return 1;\n    }\n}\n\nclass Ring extends Circle {\n    Ring(double radius) {\n        super(radius);\n    }\n\n    protected String name() {\n        return \"ring of \" + super.name() + edge();\n    }\n}\n",
        ),
        (
            "shop/shapes/Shape.java",
            "package shop.shapes;\n\npublic interface Shape {\n    double area();\n}\n",
        ),
        (
            "shop/util/Util.java",
            "package shop.util;\n\npublic class Util {\n    public static int helper() {\n        return 1;\n    }\n}\n",
        ),
    ];

    #[test]
    fn test_calls_in_the_main_class() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "Main.java"),
            [
                "12 Main>Counter UniqueCandidate QualifiedPath [Main.java#Main.Counter]",
                "19 Main.run>local UniqueCandidate EnclosingType [Main.java#Main.local]",
                "19 Main.run>helper UniqueCandidate ImportedName [shop/util/Util.java#Util.helper]",
                "19 Main.run>Other.onlyHere UniqueCandidate QualifiedPath [shop/other/Other.java#Other.onlyHere]",
                "20 Main.run>(..).toList Unresolved - []",
                "20 Main.run>(..).map Unresolved - []",
                "20 Main.run>(..).stream Unresolved - []",
                "20 Main.run>List.of External - []",
                "20 Main.run>local UniqueCandidate EnclosingType [Main.java#Main.local]",
                "22 Main.run>add.applyAsInt UniqueCandidate LexicalScope [Main.java#Main.run.add]",
                "22 Main.run>numbers.size Unresolved - []",
                "26 Main.measure>Circle UniqueCandidate QualifiedPath [shop/shapes/Circle.java#Circle.Circle]",
                "27 Main.measure>size.area UniqueCandidate ReceiverDeclaredType [shop/shapes/Circle.java#Circle.area]",
                "27 Main.measure>shape.area Ambiguous NameOnly [shop/shapes/Circle.java#Circle.area, shop/shapes/Shape.java#Shape.area]",
                "27 Main.measure>made.area UniqueCandidate ReceiverDeclaredType [shop/shapes/Circle.java#Circle.area]",
                "27 Main.measure>Util.helper UniqueCandidate QualifiedPath [shop/util/Util.java#Util.helper]",
                "32 Main.shadow>helper UniqueCandidate ImportedName [shop/util/Util.java#Util.helper]",
                "36 Main.tally>counter.next UniqueCandidate ReceiverDeclaredType [Main.java#Main.Counter.next]",
                "36 Main.tally>(..).next UniqueCandidate ReceiverDeclaredType [Main.java#Main.Counter.next]",
                "36 Main.tally>Math.max Unresolved - []",
                "36 Main.tally>missing Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_with_this_and_super() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "shop/shapes/Circle.java"),
            [
                "8 Circle.area>self.scaled UniqueCandidate EnclosingType [shop/shapes/Circle.java#Circle.scaled]",
                "8 Circle.area>edge UniqueCandidate EnclosingType [shop/shapes/Circle.java#Circle.edge]",
                "26 Ring.Ring>super.super UniqueCandidate EnclosingType [shop/shapes/Circle.java#Circle.Circle]",
                "30 Ring.name>super.name UniqueCandidate EnclosingType [shop/shapes/Circle.java#Circle.name]",
                "30 Ring.name>edge UniqueCandidate EnclosingType [shop/shapes/Circle.java#Circle.edge]",
            ]
        );
    }

    const FIELD_SOURCE: &str = "import java.util.Collection;\nimport java.util.List;\nimport java.util.Set;\n\nclass Item {\n    String name() {\n        return \"\";\n    }\n\n    Item getOut() {\n        return this;\n    }\n}\n\npublic class Node {\n    private Node parent;\n    public Item item, spare;\n    private final List<Item> items = null;\n    private Collection<Item> more;\n    private Set<Item> set;\n    private Item[] array;\n    private Object loose;\n    Runnable task = () -> {};\n\n    String run(List<Item> given) {\n        parent.item.name();\n        this.parent.item.name();\n        this.spare.getOut().name();\n        parent.getOut();\n        for (Item each : items) {\n            each.name();\n        }\n        for (var each : more) {\n            each.name();\n        }\n        for (var each : this.set) {\n            each.name();\n        }\n        for (var each : array) {\n            each.name();\n        }\n        for (var each : given) {\n            each.name();\n        }\n        items.name();\n        this.items.name();\n        loose.name();\n        this.missing.name();\n        parent.missing.name();\n        return \"\";\n    }\n}\n";

    #[test]
    fn test_fields_have_the_declared_type() {
        use crate::index::extract::testing::fields;
        assert_eq!(
            fields(Language::Java, FIELD_SOURCE),
            [
                "Field parent Node 16-16 not_exported",
                "Field item Node 17-17 exported",
                "Field spare Node 17-17 exported",
                "Field items Node 18-18 not_exported",
                "Field more Node 19-19 not_exported",
                "Field set Node 20-20 not_exported",
                "Field array Node 21-21 not_exported",
                "Field loose Node 22-22 not_exported",
            ]
        );
        // A field whose value is a lambda stays a function, and a field does
        // not change a key.
        let keys = keys(Language::Java, FIELD_SOURCE);
        assert!(keys.contains(&"file#Node.task".to_string()), "{keys:?}");
        assert!(!keys.iter().any(|key| key.contains('~')), "{keys:?}");
    }

    #[test]
    fn test_field_chains_and_loop_variables_give_the_receiver_type() {
        use crate::index::resolve::testing::rows;
        let found: Vec<String> = rows(&[("Node.java", FIELD_SOURCE)], "Node.java")
            .into_iter()
            .filter(|row| row.contains("name ") || row.contains(".getOut "))
            .collect();
        let name = "UniqueCandidate ReceiverDeclaredType [Node.java#Item.name]";
        assert_eq!(
            found,
            [
                format!("26 Node.run>parent.item::name {name}"),
                format!("27 Node.run>(..).name {name}"),
                format!("28 Node.run>(..).name {name}"),
                "28 Node.run>(..).getOut UniqueCandidate ReceiverDeclaredType [Node.java#Item.getOut]"
                    .to_string(),
                // `Node` has no `getOut`. The declared type gives no method.
                "29 Node.run>parent.getOut Unresolved - []".to_string(),
                format!("31 Node.run>each.name {name}"),
                format!("34 Node.run>each.name {name}"),
                format!("37 Node.run>each.name {name}"),
                format!("40 Node.run>each.name {name}"),
                format!("43 Node.run>each.name {name}"),
                // A collection is not an element.
                "45 Node.run>items.name Unresolved - []".to_string(),
                "46 Node.run>(..).name Unresolved - []".to_string(),
                "47 Node.run>loose.name Unresolved - []".to_string(),
                // A hop without a declared type gives candidates and no edge.
                "48 Node.run>(..).name Ambiguous NameOnly [Node.java#Item.name]".to_string(),
                "49 Node.run>parent.missing::name Ambiguous NameOnly [Node.java#Item.name]".to_string(),
            ]
        );
    }

    #[test]
    fn test_overload_by_the_exact_type_of_an_argument() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "Util.java",
            "public class Util {\n    static int twice(int value) { return 1; }\n    static int twice(double value) { return 2; }\n    static int wide(long value) { return 3; }\n    static int wide(double value) { return 4; }\n    static int pair(int left, String right) { return 5; }\n    static int pair(int left, int right) { return 6; }\n\n    static int run(int given) {\n        double real = 2.0;\n        var inferred = 2.0;\n        return twice(2) + twice(2.0) + twice(real) + twice(given)\n            + wide(2) + twice(run(1)) + twice(inferred) + twice(2.0f)\n            + pair(run(1), \"a\") + pair(1, 2) + twice(1, 2);\n    }\n}\n",
        )];
        assert_eq!(
            rows(&files, "Util.java"),
            [
                "12 Util.run>twice UniqueCandidate ArgumentType [Util.java#Util.twice]",
                "12 Util.run>twice UniqueCandidate ArgumentType [Util.java#Util.twice~2]",
                "12 Util.run>twice UniqueCandidate ArgumentType [Util.java#Util.twice~2]",
                "12 Util.run>twice UniqueCandidate ArgumentType [Util.java#Util.twice]",
                "13 Util.run>wide Ambiguous EnclosingType [Util.java#Util.wide, Util.java#Util.wide~2]",
                "13 Util.run>twice Ambiguous EnclosingType [Util.java#Util.twice, Util.java#Util.twice~2]",
                "13 Util.run>run UniqueCandidate EnclosingType [Util.java#Util.run]",
                "13 Util.run>twice Ambiguous EnclosingType [Util.java#Util.twice, Util.java#Util.twice~2]",
                "13 Util.run>twice Ambiguous EnclosingType [Util.java#Util.twice, Util.java#Util.twice~2]",
                "14 Util.run>pair UniqueCandidate ArgumentType [Util.java#Util.pair]",
                "14 Util.run>run UniqueCandidate EnclosingType [Util.java#Util.run]",
                "14 Util.run>pair UniqueCandidate ArgumentType [Util.java#Util.pair~2]",
                "14 Util.run>twice Ambiguous EnclosingType [Util.java#Util.twice, Util.java#Util.twice~2]",
            ]
        );
    }
}
