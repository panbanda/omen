//! Go symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const IMPORT_SPEC: &str = "import_spec";
const PACKAGE_CLAUSE: &str = "package_clause";
const BLANK_IDENTIFIER: &str = "blank_identifier";
const DOT: &str = "dot";

use crate::index::calls::{CallSyntax, LocalName};

const RANGE_CLAUSE: &str = "range_clause";
const COMPOSITE_LITERAL: &str = "composite_literal";
const UNARY_EXPRESSION: &str = "unary_expression";

const CALL_SYNTAX: CallSyntax = CallSyntax {
    calls: &[("call_expression", "function", "arguments")],
    constructions: &[],
    members: &[("selector_expression", "operand", "field")],
    paths: &[],
    names: &[IDENTIFIER],
    self_words: &[],
    super_words: &[],
    locals: &[(VAR_SPEC, "name", "type", "value")],
    parameters: &[
        ("parameter_declaration", "name", "type"),
        ("variadic_parameter_declaration", "name", "type"),
    ],
    return_type_field: "result",
    literals: &[],
};

pub struct Go;

const FUNCTION_LITERAL: &str = "func_literal";
const METHOD_DECLARATION: &str = "method_declaration";
const TYPE_SPEC: &str = "type_spec";
const STRUCT_TYPE: &str = "struct_type";
const INTERFACE_TYPE: &str = "interface_type";
const EXPRESSION_LIST: &str = "expression_list";
const SHORT_VAR_DECLARATION: &str = "short_var_declaration";
const VAR_SPEC: &str = "var_spec";
const IDENTIFIER: &str = "identifier";
const TYPE_IDENTIFIER: &str = "type_identifier";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "function_declaration",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        METHOD_DECLARATION,
        SymbolKind::Method,
        NameRule::Field("name"),
    ),
    DefinitionRule::new("method_elem", SymbolKind::Method, NameRule::Field("name")),
    DefinitionRule::new(TYPE_SPEC, SymbolKind::TypeAlias, NameRule::Field("name")),
    DefinitionRule::new("type_alias", SymbolKind::TypeAlias, NameRule::Field("name")),
    DefinitionRule::new(FUNCTION_LITERAL, SymbolKind::Function, NameRule::Hook),
    // A declaration with two or more names gives the first name only.
    DefinitionRule::new(
        "field_declaration",
        SymbolKind::Field,
        NameRule::Field("name"),
    ),
];

impl LanguageSpec for Go {
    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![COMPOSITE_LITERAL, UNARY_EXPRESSION, RANGE_CLAUSE]
    }

    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().local_kinds();
        kinds.extend([SHORT_VAR_DECLARATION, RANGE_CLAUSE]);
        kinds
    }

    /// `a, b := x, y` binds each name on the left to the value at the same position.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        if node.kind() == RANGE_CLAUSE {
            // `for _, element := range values` binds the second name to an
            // element. A slice has no methods, thus the type of the values
            // without `[]` is the type of the element.
            let element = node
                .child_by_field_name("left")
                .filter(|left| left.named_child_count() == 2)
                .and_then(|left| left.named_child(1))
                .filter(|element| element.kind() == IDENTIFIER);
            if let (Some(element), Some(values)) = (element, node.child_by_field_name("right")) {
                out.push(LocalName {
                    name: context.text(element),
                    type_name: None,
                    value_start: Some(values.start_byte()),
                    is_parameter: false,
                    element: true,
                    result_index: None,
                });
            }
            return;
        }
        if node.kind() != SHORT_VAR_DECLARATION {
            let first = out.len();
            self.call_syntax().locals(node, context, out);
            // `name ...Type` is a slice of the type.
            if node.kind() == "variadic_parameter_declaration" {
                for local in &mut out[first..] {
                    local.type_name = local.type_name.take().map(|name| format!("...{name}"));
                }
            }
            // `var name = Type{...}` has the type of the literal.
            let literal = node
                .child_by_field_name("value")
                .and_then(|values| values.named_child(0))
                .and_then(|value| literal_type(value, context));
            if let (Some(literal), Some(local)) = (literal, out.get_mut(first)) {
                local.type_name.get_or_insert(literal);
            }
            return;
        }
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        for position in 0..left.named_child_count() {
            let Some(name) = left.named_child(position as u32) else {
                continue;
            };
            if name.kind() != IDENTIFIER {
                continue;
            }
            let paired = left.named_child_count() == right.named_child_count();
            let value = paired.then(|| right.named_child(position as u32)).flatten();
            // `a, b := f()` gives each name one result of the call.
            let call = (!paired && right.named_child_count() == 1)
                .then(|| right.named_child(0))
                .flatten();
            out.push(LocalName {
                name: context.text(name),
                type_name: value.and_then(|value| literal_type(value, context)),
                value_start: value.or(call).map(|value| value.start_byte()),
                is_parameter: false,
                element: false,
                result_index: call.map(|_| position),
            });
        }
    }

    fn import_kinds(&self) -> Vec<&'static str> {
        vec![IMPORT_SPEC]
    }

    fn module_kinds(&self) -> Vec<&'static str> {
        vec![PACKAGE_CLAUSE]
    }

    /// The name in the package clause. A caller uses this name, which can be
    /// different from the last segment of the import path.
    fn module_name(&self, node: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
        node.named_child(0).map(|name| context.text(name))
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![BLANK_IDENTIFIER, DOT]
    }

    /// One binding for each import specification. The path has no quotation marks.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let Some(path) = node.child_by_field_name("path") else {
            return;
        };
        let path = context.text(path);
        let path = path.trim_matches(|quote| quote == '"' || quote == '`');
        let package = path.rsplit('/').next().unwrap_or(path).to_string();
        let name = node.child_by_field_name("name");
        let (kind, imported, local) = match name.map(|name| name.kind()) {
            Some(BLANK_IDENTIFIER) => (ImportKind::Load, None, None),
            Some(DOT) => (ImportKind::Wildcard, None, None),
            Some(_) => (
                ImportKind::Module,
                Some(package),
                name.map(|name| context.text(name)),
            ),
            None => (ImportKind::Module, Some(package.clone()), Some(package)),
        };
        out.push(context.import(node, path, kind, imported, local));
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            STRUCT_TYPE,
            INTERFACE_TYPE,
            EXPRESSION_LIST,
            SHORT_VAR_DECLARATION,
            VAR_SPEC,
            IDENTIFIER,
            TYPE_IDENTIFIER,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    /// A function literal has a name only when a declaration binds it to one variable.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        let values = node
            .parent()
            .filter(|list| list.kind() == EXPRESSION_LIST)?;
        let mut cursor = values.walk();
        let position = values
            .named_children(&mut cursor)
            .position(|value| value == node)?;
        let declaration = values.parent()?;
        let name = match declaration.kind() {
            SHORT_VAR_DECLARATION => declaration
                .child_by_field_name("left")?
                .named_child(position as u32),
            VAR_SPEC => {
                let mut cursor = declaration.walk();
                let name = declaration
                    .children_by_field_name("name", &mut cursor)
                    .nth(position);
                name
            }
            _ => None,
        }?;
        (name.kind() == IDENTIFIER && context.text(name) != "_").then_some(name)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        match draft.node.kind() {
            METHOD_DECLARATION => {
                let receiver = draft
                    .node
                    .child_by_field_name("receiver")
                    .and_then(|receiver| find_descendant(receiver, &[TYPE_IDENTIFIER]))
                    .map(|name| context.text(name));
                if let Some(receiver) = receiver {
                    draft.container = Some(receiver.clone());
                    draft.container_path = vec![receiver.clone()];
                    draft.receiver_type = Some(receiver);
                }
            }
            TYPE_SPEC => {
                let definition = draft.node.child_by_field_name("type");
                match definition.map(|definition| definition.kind()) {
                    Some(STRUCT_TYPE) => draft.kind = SymbolKind::Struct,
                    Some(INTERFACE_TYPE) => draft.kind = SymbolKind::Interface,
                    _ => {}
                }
            }
            FUNCTION_LITERAL => draft.span = Span::between(&draft.name_node, &draft.node),
            _ => {}
        }
        true
    }

    /// Go exports a package-level name that starts with an upper-case letter.
    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let upper_case = draft.name.chars().next().is_some_and(char::is_uppercase);
        if upper_case && !context.in_function() {
            Exported::Exported
        } else {
            Exported::NotExported
        }
    }
}

/// The type that a composite literal names: `Type{...}` or `&Type{...}`.
fn literal_type(value: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
    let literal = match value.kind() {
        UNARY_EXPRESSION => value.child_by_field_name("operand")?,
        _ => value,
    };
    (literal.kind() == COMPOSITE_LITERAL)
        .then(|| literal.child_by_field_name("type"))
        .flatten()
        .map(|type_name| context.text(type_name))
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_functions_and_visibility() {
        let source = "package a\n\nfunc Public() {}\n\nfunc private() int {\n\treturn 1\n}\n";
        assert_eq!(
            rows(Language::Go, source),
            [
                "Function Public - 3-3 exported",
                "Function private - 5-7 not_exported",
            ]
        );
    }

    #[test]
    fn test_types() {
        let source = "package a\n\ntype Shape interface {\n\tArea() float64\n\tname() string\n}\n\ntype Circle struct {\n\tRadius float64\n}\n\ntype Scale = float64\n\ntype celsius float64\n\ntype (\n\tA struct{}\n\tb interface{}\n)\n";
        assert_eq!(
            rows(Language::Go, source),
            [
                "Interface Shape - 3-6 exported",
                "Method Area Shape 4-4 exported",
                "Method name Shape 5-5 not_exported",
                "Struct Circle - 8-10 exported",
                "TypeAlias Scale - 12-12 exported",
                "TypeAlias celsius - 14-14 not_exported",
                "Struct A - 17-17 exported",
                "Interface b - 18-18 not_exported",
            ]
        );
    }

    #[test]
    fn test_receiver_methods() {
        let source = "package a\n\nfunc (c *Circle) Area() float64 {\n\treturn 1\n}\n\nfunc (s Square) side() float64 { return 1 }\n\nfunc (l *List[T]) Push(v T) {}\n";
        assert_eq!(
            rows(Language::Go, source),
            [
                "Method Area Circle 3-5 exported",
                "Method side Square 7-7 not_exported",
                "Method Push List 9-9 exported",
            ]
        );
        let receivers: Vec<Option<String>> = symbols(Language::Go, source)
            .into_iter()
            .map(|symbol| symbol.receiver_type)
            .collect();
        assert_eq!(
            receivers,
            [
                Some("Circle".to_string()),
                Some("Square".to_string()),
                Some("List".to_string())
            ]
        );
        assert_eq!(
            keys(Language::Go, source),
            ["file#Circle.Area", "file#Square.side", "file#List.Push"]
        );
    }

    #[test]
    fn test_bound_function_literals() {
        let source = "package a\n\nfunc run() {\n\tadd := func(n int) int { return n + 1 }\n\tvar sub = func(n int) int {\n\t\treturn n - 1\n\t}\n\tuse(func() {})\n\tOne, two := func() {}, func() {}\n}\n\nvar Handler = func() {}\n";
        assert_eq!(
            rows(Language::Go, source),
            [
                "Function run - 3-10 not_exported",
                "Function add run 4-4 not_exported",
                "Function sub run 5-7 not_exported",
                "Function One run 9-9 not_exported",
                "Function two run 9-9 not_exported",
                "Function Handler - 12-12 exported",
            ]
        );
    }

    #[test]
    fn test_function_literal_in_method_has_method_as_container() {
        let source =
            "package a\n\nfunc (c *Circle) Area() {\n\thelper := func() {}\n\thelper()\n}\n";
        assert_eq!(
            rows(Language::Go, source),
            [
                "Method Area Circle 3-6 exported",
                "Function helper Area 4-4 not_exported",
            ]
        );
        assert_eq!(
            keys(Language::Go, source),
            ["file#Circle.Area", "file#Circle.Area.helper"]
        );
    }

    #[test]
    fn test_duplicate_descriptors_get_an_ordinal() {
        let source = "package a\n\nfunc init() {}\n\nfunc init() {}\n";
        assert_eq!(keys(Language::Go, source), ["file#init", "file#init~2"]);
    }

    #[test]
    fn test_signature_and_doc() {
        let source = "package a\n\n// Area gives the area.\n// It is exact.\nfunc Area(r float64) float64 {\n\treturn r\n}\n";
        let symbols = symbols(Language::Go, source);
        assert_eq!(symbols[0].signature, "func Area(r float64) float64");
        assert_eq!(
            symbols[0].doc.as_deref(),
            Some("// Area gives the area.\n// It is exact.")
        );
        assert_eq!(symbols[0].name_range.start_column, 5);
        assert!(symbols[0].body_range.is_some());
        assert_eq!(symbols[0].scip_symbol, None);
    }

    #[test]
    fn test_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            ("go.mod", "module example.com/shop\n\ngo 1.21\n"),
            (
                "main.go",
                "package main\n\nimport \"math\"\n\nimport (\n\t_ \"embed\"\n\t. \"example.com/shop/other\"\n\t\"example.com/shop/shapes\"\n\ttools \"example.com/shop/util\"\n\t\"example.com/shop/missing\"\n\t\"example.com/shop\"\n)\n",
            ),
            ("other/other.go", "package other\n"),
            ("shapes/shapes.go", "package shapes\n"),
            ("shapes/more.go", "package shapes\n"),
            ("util/util.go", "package util\n"),
            ("util/deep/deep.go", "package deep\n"),
        ];
        assert_eq!(
            rows(&files, "main.go"),
            [
                "3 Module math math->math external",
                "6 Load embed -->- external",
                "7 Wildcard example.com/shop/other -->- other/other.go",
                "8 Module example.com/shop/shapes shapes->shapes package(shapes/more.go,shapes/shapes.go)",
                "9 Module example.com/shop/util util->tools util/util.go",
                "10 Module example.com/shop/missing missing->missing unresolved",
                "11 Module example.com/shop shop->shop main.go",
            ]
        );
    }

    #[test]
    fn test_import_without_a_go_mod_file_is_unresolved() {
        use crate::index::imports::testing::rows;
        let files = [("main.go", "package main\n\nimport \"a/b\"\n")];
        assert_eq!(rows(&files, "main.go"), ["3 Module a/b b->b unresolved"]);
    }

    #[test]
    fn test_two_modules_with_one_module_path_are_ambiguous() {
        use crate::index::imports::testing::rows;
        let files = [
            ("app/go.mod", "module example.com/app\n"),
            (
                "app/main.go",
                "package main\n\nimport (\n\t\"example.com/lib/pkg\"\n\t\"example.com/solo/x\"\n\t\"example.com/app/inner\"\n)\n",
            ),
            ("app/inner/a.go", "package inner\n"),
            ("app/inner/b.go", "package inner\n"),
            ("one/go.mod", "module example.com/lib\n"),
            ("one/pkg/a.go", "package pkg\n"),
            ("two/go.mod", "module example.com/lib\n"),
            ("two/pkg/b.go", "package pkg\n"),
            ("third/go.mod", "module example.com/solo\n"),
            ("third/x/x.go", "package x\n"),
        ];
        assert_eq!(
            rows(&files, "app/main.go"),
            [
                "4 Module example.com/lib/pkg pkg->pkg ambiguous(one/pkg/a.go,two/pkg/b.go)",
                "5 Module example.com/solo/x x->x third/x/x.go",
                "6 Module example.com/app/inner inner->inner package(app/inner/a.go,app/inner/b.go)",
            ]
        );
    }

    const CALL_FILES: &[(&str, &str)] = &[
        ("go.mod", "module example.com/shop\n"),
        (
            "main.go",
            "package main\n\nimport (\n\t\"strings\"\n\n\t. \"example.com/shop/other\"\n\t\"example.com/shop/shapes\"\n\ttools \"example.com/shop/util\"\n)\n\nfunc local() int { return 4 }\n\nfunc run() int {\n\ttotal := local() + tools.Helper() + OnlyHere()\n\tcircle := shapes.NewCircle(1)\n\t_ = circle.Area()\n\tnumbers := apply(func(n int) int { return local() })\n\tadd := func(n int) int { return n + 1 }\n\treturn total + add(len(numbers))\n}\n\nfunc apply(f func(int) int) int {\n\treturn f(1)\n}\n\nfunc measure(size shapes.Square, shape shapes.Shape) float64 {\n\treturn size.Area() + shape.Area()\n}\n\nfunc shadow() int {\n\tHelper := func() int { return 9 }\n\treturn Helper()\n}\n\nfunc external(unknown Thing) string {\n\tvar b strings.Builder\n\tb.WriteString(\"x\")\n\tunknown.Area()\n\treturn strings.ToUpper(b.String()) + tools.Missing()\n}\n",
        ),
        (
            "other/other.go",
            "package other\n\nfunc Helper() int { return 2 }\n\nfunc OnlyHere() int { return 3 }\n\nfunc WriteString() {}\n",
        ),
        (
            "shapes/shapes.go",
            "package shapes\n\ntype Shape interface {\n\tArea() float64\n}\n\ntype Circle struct{}\n\ntype Square struct{}\n\nfunc NewCircle(radius float64) *Circle {\n\treturn &Circle{}\n}\n\nfunc (c *Circle) Area() float64 {\n\treturn c.scaled(1) + hidden()\n}\n\nfunc (c *Circle) scaled(factor float64) float64 {\n\treturn factor\n}\n\nfunc (s Square) Area() float64 {\n\treturn 1\n}\n",
        ),
        ("shapes/more.go", "package shapes\n\nfunc hidden() float64 { return 7 }\n"),
        ("util/util.go", "package util\n\nfunc Helper() int { return 1 }\n"),
        ("tools.py", "def local():\n    pass\n"),
    ];

    #[test]
    fn test_calls_with_names_and_imports() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.go")[..9],
            [
                "14 run>local UniqueCandidate SameFile [main.go#local]",
                "14 run>tools.Helper UniqueCandidate ImportedModule [util/util.go#Helper]",
                "14 run>OnlyHere UniqueCandidate ImportedModule [other/other.go#OnlyHere]",
                "15 run>shapes.NewCircle UniqueCandidate ImportedModule [shapes/shapes.go#NewCircle]",
                "16 run>circle.Area UniqueCandidate ReceiverDeclaredType [shapes/shapes.go#Circle.Area]",
                "17 run>apply UniqueCandidate SameFile [main.go#apply]",
                "17 run>local UniqueCandidate SameFile [main.go#local]",
                "19 run>add UniqueCandidate LexicalScope [main.go#run.add]",
                "19 run>len Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_that_do_not_give_an_edge() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.go")[9..],
            [
                "23 apply>f Unresolved - []",
                "27 measure>size.Area UniqueCandidate ReceiverDeclaredType [shapes/shapes.go#Square.Area]",
                "27 measure>shape.Area Ambiguous NameOnly [shapes/shapes.go#Circle.Area, shapes/shapes.go#Shape.Area, shapes/shapes.go#Square.Area]",
                "32 shadow>Helper UniqueCandidate LexicalScope [main.go#shadow.Helper]",
                "37 external>b.WriteString Unresolved - []",
                "38 external>unknown.Area Unresolved - []",
                "39 external>strings.ToUpper External - []",
                "39 external>b.String Unresolved - []",
                "39 external>tools.Missing Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_in_methods() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "shapes/shapes.go"),
            [
                "16 Circle.Area>c.scaled UniqueCandidate ReceiverDeclaredType [shapes/shapes.go#Circle.scaled]",
                "16 Circle.Area>hidden UniqueCandidate SamePackage [shapes/more.go#hidden]",
            ]
        );
    }

    #[test]
    fn test_call_in_a_function_literal_without_a_name_is_via_closure() {
        use crate::index::imports::testing::index;
        let files = [(
            "main.go",
            "package main\n\nfunc local() int { return 4 }\n\nfunc run() int {\n\tdirect := local()\n\tapply(func() int { return local() })\n\tnamed := func() int { return local() }\n\treturn direct + named()\n}\n",
        )];
        let facts = index(&files);
        let flags: Vec<(u32, &str, Option<&str>, bool)> = facts[0]
            .references
            .iter()
            .map(|reference| {
                (
                    reference.name_range.start_line,
                    reference.name.as_str(),
                    reference.enclosing.as_deref(),
                    reference.via_closure,
                )
            })
            .collect();
        assert_eq!(
            flags,
            [
                (6, "local", Some("main.go#run"), false),
                (7, "apply", Some("main.go#run"), false),
                (7, "local", Some("main.go#run"), true),
                (8, "local", Some("main.go#run.named"), false),
                (9, "named", Some("main.go#run"), false),
            ]
        );
    }

    #[test]
    fn test_receiver_types_from_composite_literals_and_call_results() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "main.go",
            "package main\n\ntype FlagSet struct{}\n\nfunc (f *FlagSet) Lookup(name string) int { return 1 }\n\ntype Command struct{}\n\nfunc (c *Command) AddCommand(child *Command) {}\n\nfunc (c *Command) Flags() *FlagSet { return &FlagSet{} }\n\nfunc (c *Command) Root() *Command { return c }\n\nfunc run(unknown Other) {\n\troot := &Command{}\n\tchild := Command{}\n\troot.AddCommand(&child)\n\tchild.Flags().Lookup(\"x\")\n\troot.Root().Root().Flags()\n\tunknown.Make().Flags()\n}\n",
        )];
        assert_eq!(
            rows(&files, "main.go"),
            [
                "18 run>root.AddCommand UniqueCandidate ReceiverDeclaredType [main.go#Command.AddCommand]",
                "19 run>(..).Lookup UniqueCandidate ReceiverDeclaredType [main.go#FlagSet.Lookup]",
                "19 run>child.Flags UniqueCandidate ReceiverDeclaredType [main.go#Command.Flags]",
                "20 run>(..).Flags UniqueCandidate ReceiverDeclaredType [main.go#Command.Flags]",
                "20 run>(..).Root UniqueCandidate ReceiverDeclaredType [main.go#Command.Root]",
                "20 run>root.Root UniqueCandidate ReceiverDeclaredType [main.go#Command.Root]",
                "21 run>(..).Flags Ambiguous NameOnly [main.go#Command.Flags]",
                "21 run>unknown.Make Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_receiver_types_from_a_range_and_a_package_variable() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "main.go",
            "package main\n\ntype Command struct{}\n\nfunc (c *Command) Name() string { return \"\" }\n\nfunc (c *Command) Commands() []*Command { return nil }\n\nvar rootCmd = &Command{}\n\nfunc run(list []*Command) {\n\tfor _, child := range rootCmd.Commands() {\n\t\tchild.Name()\n\t}\n\tfor index := range rootCmd.Commands() {\n\t\tindex.Name()\n\t}\n\tfor _, given := range list {\n\t\tgiven.Name()\n\t}\n}\n",
        )];
        assert_eq!(
            rows(&files, "main.go"),
            [
                "12 run>rootCmd.Commands UniqueCandidate ReceiverDeclaredType [main.go#Command.Commands]",
                "13 run>child.Name UniqueCandidate ReceiverDeclaredType [main.go#Command.Name]",
                "15 run>rootCmd.Commands UniqueCandidate ReceiverDeclaredType [main.go#Command.Commands]",
                "16 run>index.Name Unresolved - []",
                "19 run>given.Name UniqueCandidate ReceiverDeclaredType [main.go#Command.Name]",
            ]
        );
    }

    #[test]
    fn test_variadic_parameter_is_a_slice() {
        use crate::index::resolve::testing::rows;
        let source = "package shop\n\ntype Item struct{}\n\nfunc (i *Item) Name() string {\n\treturn \"\"\n}\n\nfunc add(items ...*Item) {\n\tfor _, item := range items {\n\t\titem.Name()\n\t}\n\titems.Name()\n}\n";
        assert_eq!(
            rows(&[("a.go", source)], "a.go"),
            [
                "11 add>item.Name UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]",
                "13 add>items.Name Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_fields_and_results_give_the_type_of_a_receiver() {
        use crate::index::resolve::testing::rows;
        let source = "package shop\n\ntype Item struct {\n\tname string\n}\n\nfunc (i *Item) Name() string {\n\treturn i.name\n}\n\ntype Cart struct {\n\tparent *Cart\n\tfirst  Item\n\titems  []*Item\n\tcount  int\n}\n\nfunc (c *Cart) Root() *Cart {\n\treturn c\n}\n\nfunc (c *Cart) Find(name string) (*Item, []string, error) {\n\treturn nil, nil, nil\n}\n\nfunc (c *Cart) Run() {\n\tc.parent.Root()\n\tc.parent.parent.Root()\n\tc.first.Name()\n\tfor _, item := range c.items {\n\t\titem.Name()\n\t}\n\tfound, _, err := c.Find(\"a\")\n\tfound.Name()\n\terr.Error()\n\tc.items.Name()\n\tc.count.Name()\n\tc.missing.Name()\n\tlocal := c.parent\n\tlocal.Root()\n}\n";
        let files = [("a.go", source)];
        assert_eq!(
            rows(&files, "a.go"),
            [
                "27 Cart.Run>c.parent::Root UniqueCandidate ReceiverDeclaredType [a.go#Cart.Root]",
                "28 Cart.Run>c.parent.parent::Root UniqueCandidate ReceiverDeclaredType [a.go#Cart.Root]",
                "29 Cart.Run>c.first::Name UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]",
                "31 Cart.Run>item.Name UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]",
                "33 Cart.Run>c.Find UniqueCandidate ReceiverDeclaredType [a.go#Cart.Find]",
                "34 Cart.Run>found.Name UniqueCandidate ReceiverDeclaredType [a.go#Item.Name]",
                "35 Cart.Run>err.Error Unresolved - []",
                "36 Cart.Run>c.items::Name Unresolved - []",
                "37 Cart.Run>c.count::Name Unresolved - []",
                "38 Cart.Run>c.missing::Name Ambiguous NameOnly [a.go#Item.Name]",
                "40 Cart.Run>local.Root UniqueCandidate ReceiverDeclaredType [a.go#Cart.Root]",
            ]
        );
        let fields: Vec<String> = crate::index::extract::testing::fields(Language::Go, source)
            .into_iter()
            .filter(|row| row.starts_with("Field"))
            .collect();
        assert_eq!(
            fields,
            [
                "Field name Item 4-4 not_exported",
                "Field parent Cart 12-12 not_exported",
                "Field first Cart 13-13 not_exported",
                "Field items Cart 14-14 not_exported",
                "Field count Cart 15-15 not_exported",
            ]
        );
    }
}
