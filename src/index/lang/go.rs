//! Go symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const IMPORT_SPEC: &str = "import_spec";
const BLANK_IDENTIFIER: &str = "blank_identifier";
const DOT: &str = "dot";

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
];

impl LanguageSpec for Go {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![IMPORT_SPEC]
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
                "8 Module example.com/shop/shapes shapes->shapes ambiguous(shapes/more.go,shapes/shapes.go)",
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
}
