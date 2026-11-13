//! Rust symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const USE_DECLARATION: &str = "use_declaration";
const USE_AS_CLAUSE: &str = "use_as_clause";
const USE_WILDCARD: &str = "use_wildcard";
const SCOPED_USE_LIST: &str = "scoped_use_list";
const USE_LIST: &str = "use_list";
const SELF: &str = "self";

pub struct Rust;

const IMPL_ITEM: &str = "impl_item";
const TRAIT_ITEM: &str = "trait_item";
const MOD_ITEM: &str = "mod_item";
const CLOSURE_EXPRESSION: &str = "closure_expression";
const LET_DECLARATION: &str = "let_declaration";
const IDENTIFIER: &str = "identifier";
const TYPE_IDENTIFIER: &str = "type_identifier";
const GENERIC_TYPE: &str = "generic_type";
const SCOPED_TYPE_IDENTIFIER: &str = "scoped_type_identifier";
const REFERENCE_TYPE: &str = "reference_type";
const VISIBILITY_MODIFIER: &str = "visibility_modifier";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::function("function_item", NameRule::Field("name")),
    DefinitionRule::function("function_signature_item", NameRule::Field("name")),
    DefinitionRule::new("struct_item", SymbolKind::Struct, NameRule::Field("name")),
    DefinitionRule::new("union_item", SymbolKind::Struct, NameRule::Field("name")),
    DefinitionRule::new("enum_item", SymbolKind::Enum, NameRule::Field("name")),
    DefinitionRule::new(TRAIT_ITEM, SymbolKind::Trait, NameRule::Field("name")),
    DefinitionRule::new("type_item", SymbolKind::TypeAlias, NameRule::Field("name")),
    DefinitionRule::new(MOD_ITEM, SymbolKind::Module, NameRule::Field("name")),
    DefinitionRule::new(CLOSURE_EXPRESSION, SymbolKind::Function, NameRule::Hook),
];

/// The name of a type without its path, its generic arguments, and its reference.
fn type_name(mut node: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
    loop {
        node = match node.kind() {
            TYPE_IDENTIFIER => return Some(context.text(node)),
            GENERIC_TYPE | REFERENCE_TYPE => node.child_by_field_name("type")?,
            SCOPED_TYPE_IDENTIFIER => node.child_by_field_name("name")?,
            _ => return find_descendant(node, &[TYPE_IDENTIFIER]).map(|name| context.text(name)),
        };
    }
}

/// `pub` exports an item. `pub(crate)` and the other restricted forms do not.
fn visibility(node: Node<'_>, context: &Context<'_, '_>) -> Exported {
    let mut cursor = node.walk();
    let public = node
        .children(&mut cursor)
        .any(|child| child.kind() == VISIBILITY_MODIFIER && context.text(child) == "pub");
    if public {
        Exported::Exported
    } else {
        Exported::NotExported
    }
}

/// A method of a trait implementation has the visibility of the trait. This
/// function finds the trait only when the trait is adjacent to the `impl` block.
fn trait_visibility(implementation: Node<'_>, context: &Context<'_, '_>) -> Exported {
    let Some(reference) = implementation.child_by_field_name("trait") else {
        return Exported::NotExported;
    };
    let name = match reference.kind() {
        TYPE_IDENTIFIER => Some(reference),
        GENERIC_TYPE => reference
            .child_by_field_name("type")
            .filter(|base| base.kind() == TYPE_IDENTIFIER),
        _ => None,
    }
    .map(|name| context.text(name));
    let definition = name.as_ref().and_then(|name| {
        let parent = implementation.parent()?;
        let mut cursor = parent.walk();
        let definition = parent.children(&mut cursor).find(|item| {
            item.kind() == TRAIT_ITEM
                && item
                    .child_by_field_name("name")
                    .is_some_and(|candidate| &context.text(candidate) == name)
        });
        definition
    });
    match definition {
        Some(definition) => visibility(definition, context),
        None => Exported::Unknown(format!(
            "the trait `{}` is not in this scope of the file",
            context.text(reference)
        )),
    }
}

impl LanguageSpec for Rust {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![USE_DECLARATION, MOD_ITEM]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![USE_AS_CLAUSE, USE_WILDCARD, SCOPED_USE_LIST, USE_LIST, SELF]
    }

    /// One binding for each leaf of a `use` tree. The path of a binding is the
    /// full path of the leaf. `mod name;` gives a module declaration.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        if node.kind() == MOD_ITEM {
            if let (None, Some(name)) = (
                node.child_by_field_name("body"),
                node.child_by_field_name("name"),
            ) {
                let name = context.text(name);
                out.push(context.import(
                    node,
                    name.clone(),
                    ImportKind::ModuleDeclaration,
                    None,
                    Some(name),
                ));
            }
            return;
        }
        let mut cursor = node.walk();
        let exported = node
            .children(&mut cursor)
            .any(|child| child.kind() == VISIBILITY_MODIFIER);
        if let Some(argument) = node.child_by_field_name("argument") {
            use_tree(node, argument, "", exported, context, out);
        }
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            IMPL_ITEM,
            LET_DECLARATION,
            IDENTIFIER,
            TYPE_IDENTIFIER,
            GENERIC_TYPE,
            SCOPED_TYPE_IDENTIFIER,
            REFERENCE_TYPE,
            VISIBILITY_MODIFIER,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["line_comment", "block_comment"]
    }

    /// A closure has a name only when a `let` declaration binds it to one variable.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        let declaration = node
            .parent()
            .filter(|parent| parent.kind() == LET_DECLARATION)?;
        if declaration.child_by_field_name("value") != Some(node) {
            return None;
        }
        declaration
            .child_by_field_name("pattern")
            .filter(|pattern| pattern.kind() == IDENTIFIER)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        match draft.node.kind() {
            // `mod name;` refers to a different file. It does not define a module here.
            MOD_ITEM => return draft.body.is_some(),
            CLOSURE_EXPRESSION => draft.span = Span::between(&draft.name_node, &draft.node),
            _ => {}
        }
        if let Some(scope) = context.scopes.last() {
            if scope.node.kind() == IMPL_ITEM {
                draft.receiver_type = Some(scope.name.clone());
            }
        }
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        if context.in_function() {
            return Exported::NotExported;
        }
        match context.scopes.last() {
            Some(scope) if scope.node.kind() == TRAIT_ITEM => scope.exported.clone(),
            Some(scope)
                if scope.node.kind() == IMPL_ITEM
                    && scope.node.child_by_field_name("trait").is_some() =>
            {
                trait_visibility(scope.node, context)
            }
            _ => visibility(draft.node, context),
        }
    }

    fn scope<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        if node.kind() != IMPL_ITEM {
            return None;
        }
        let name = type_name(node.child_by_field_name("type")?, context)?;
        let mut path = context.path();
        path.push(name.clone());
        Some(Scope {
            node,
            name,
            role: ScopeRole::Type,
            kind: None,
            exported: Exported::Unknown("an `impl` block has no visibility".to_string()),
            path,
        })
    }
}

fn joined(prefix: &str, path: &str) -> String {
    let path: String = path.split_whitespace().collect();
    match (prefix.is_empty(), path.is_empty()) {
        (true, _) => path,
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}::{path}"),
    }
}

fn last_segment(path: &str) -> String {
    path.rsplit("::").next().unwrap_or(path).to_string()
}

/// Adds the bindings of one node of a `use` tree. `prefix` is the path of the lists that contain it.
fn use_tree(
    statement: Node<'_>,
    node: Node<'_>,
    prefix: &str,
    exported: bool,
    context: &Context<'_, '_>,
    out: &mut Vec<ImportBinding>,
) {
    let named = if exported {
        ImportKind::ReExportNamed
    } else {
        ImportKind::Named
    };
    match node.kind() {
        USE_LIST => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                use_tree(statement, child, prefix, exported, context, out);
            }
        }
        SCOPED_USE_LIST => {
            let path = node
                .child_by_field_name("path")
                .map(|path| context.text(path))
                .unwrap_or_default();
            if let Some(list) = node.child_by_field_name("list") {
                use_tree(
                    statement,
                    list,
                    &joined(prefix, &path),
                    exported,
                    context,
                    out,
                );
            }
        }
        USE_WILDCARD => {
            let path = node
                .named_child(0)
                .map(|path| context.text(path))
                .unwrap_or_default();
            let kind = if exported {
                ImportKind::ReExportWildcard
            } else {
                ImportKind::Wildcard
            };
            out.push(context.import(statement, joined(prefix, &path), kind, None, None));
        }
        USE_AS_CLAUSE => {
            let (Some(path), Some(alias)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias"),
            ) else {
                return;
            };
            let path = joined(prefix, &context.text(path));
            let name = last_segment(&path);
            out.push(context.import(
                statement,
                path,
                named,
                Some(name),
                Some(context.text(alias)),
            ));
        }
        // `use a::{self}` binds the module `a`.
        SELF if !prefix.is_empty() => {
            let name = last_segment(prefix);
            out.push(context.import(statement, prefix, named, Some(name.clone()), Some(name)));
        }
        _ => {
            let path = joined(prefix, &context.text(node));
            let name = last_segment(&path);
            out.push(context.import(statement, path, named, Some(name.clone()), Some(name)));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};
    use crate::index::model::Exported;

    #[test]
    fn test_functions_and_visibility() {
        let source = "pub fn open() {}\nfn closed() {}\npub(crate) fn inside() {}\npub(super) fn above() {}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Function open - 1-1 exported",
                "Function closed - 2-2 not_exported",
                "Function inside - 3-3 not_exported",
                "Function above - 4-4 not_exported",
            ]
        );
    }

    #[test]
    fn test_types() {
        let source = "pub struct Circle {\n    r: f64,\n}\nenum Unit {\n    Metre,\n}\npub type Scale = f64;\npub union Bits {\n    a: u8,\n}\npub trait Shape {\n    fn area(&self) -> f64;\n    fn name(&self) -> String {\n        String::new()\n    }\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Struct Circle - 1-3 exported",
                "Enum Unit - 4-6 not_exported",
                "TypeAlias Scale - 7-7 exported",
                "Struct Bits - 8-10 exported",
                "Trait Shape - 11-16 exported",
                "Method area Shape 12-12 exported",
                "Method name Shape 13-15 exported",
            ]
        );
    }

    #[test]
    fn test_methods_of_a_private_trait_are_not_exported() {
        let source = "trait Hidden {\n    fn area(&self) -> f64;\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Trait Hidden - 1-3 not_exported",
                "Method area Hidden 2-2 not_exported",
            ]
        );
    }

    #[test]
    fn test_inherent_impl_methods() {
        let source = "impl Circle {\n    pub fn new() -> Self {\n        Circle\n    }\n    fn scaled(&self) {}\n}\nimpl<T> List<T> {\n    pub fn push(&mut self) {}\n}\nimpl shapes::Square {\n    pub fn side(&self) {}\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Method new Circle 2-4 exported",
                "Method scaled Circle 5-5 not_exported",
                "Method push List 8-8 exported",
                "Method side Square 11-11 exported",
            ]
        );
        assert_eq!(
            symbols(Language::Rust, source)[0].receiver_type.as_deref(),
            Some("Circle")
        );
        assert_eq!(
            keys(Language::Rust, source),
            [
                "file#Circle.new",
                "file#Circle.scaled",
                "file#List.push",
                "file#Square.side"
            ]
        );
    }

    #[test]
    fn test_trait_impl_method_has_the_visibility_of_a_trait_in_the_same_scope() {
        let source = "pub trait Shape {\n    fn area(&self);\n}\ntrait Hidden {\n    fn peek(&self);\n}\nimpl Shape for Circle {\n    fn area(&self) {}\n}\nimpl Hidden for Circle {\n    fn peek(&self) {}\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Trait Shape - 1-3 exported",
                "Method area Shape 2-2 exported",
                "Trait Hidden - 4-6 not_exported",
                "Method peek Hidden 5-5 not_exported",
                "Method area Circle 8-8 exported",
                "Method peek Circle 11-11 not_exported",
            ]
        );
    }

    #[test]
    fn test_trait_impl_method_of_a_trait_from_a_different_file_has_unknown_visibility() {
        let source =
            "impl std::fmt::Display for Circle {\n    fn fmt(&self) {}\n}\nimpl Shape for Circle {\n    fn area(&self) {}\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Method fmt Circle 2-2 unknown",
                "Method area Circle 5-5 unknown",
            ]
        );
        assert!(matches!(
            symbols(Language::Rust, source)[0].exported,
            Exported::Unknown(_)
        ));
    }

    #[test]
    fn test_same_name_in_two_impl_blocks_gets_an_ordinal() {
        let source = "pub trait Shape {\n    fn area(&self);\n}\nimpl Circle {\n    pub fn area(&self) {}\n}\nimpl Shape for Circle {\n    fn area(&self) {}\n}\n";
        assert_eq!(
            keys(Language::Rust, source),
            [
                "file#Shape",
                "file#Shape.area",
                "file#Circle.area",
                "file#Circle.area~2"
            ]
        );
    }

    #[test]
    fn test_nested_functions_and_bound_closures() {
        let source = "fn run() {\n    fn inner() {}\n    let add = |n: u32| n + 1;\n    let typed: fn() = || {};\n    let (a, b) = (|| 1, || 2);\n    call(|| 3);\n}\nimpl Circle {\n    pub fn area(&self) {\n        let half = || 1;\n        fn helper() {}\n    }\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Function run - 1-7 not_exported",
                "Function inner run 2-2 not_exported",
                "Function add run 3-3 not_exported",
                "Function typed run 4-4 not_exported",
                "Method area Circle 9-12 exported",
                "Function half area 10-10 not_exported",
                "Function helper area 11-11 not_exported",
            ]
        );
    }

    #[test]
    fn test_modules() {
        let source =
            "mod outside;\npub mod shapes {\n    pub fn area() {}\n    fn hidden() {}\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            [
                "Module shapes - 2-5 exported",
                "Function area shapes 3-3 exported",
                "Function hidden shapes 4-4 not_exported",
            ]
        );
        assert_eq!(
            keys(Language::Rust, source),
            ["file#shapes", "file#shapes.area", "file#shapes.hidden"]
        );
    }

    #[test]
    fn test_foreign_function_declarations() {
        let source = "extern \"C\" {\n    pub fn abs(value: i32) -> i32;\n}\n";
        assert_eq!(
            rows(Language::Rust, source),
            ["Function abs - 2-2 exported"]
        );
    }

    const IMPORT_FILES: &[(&str, &str)] = &[
        (
            "src/main.rs",
            "mod other;\nmod shapes;\nmod util;\nmod missing;\nmod inline {}\n\nuse std::collections::HashMap;\nuse shapes::{Circle, Shape as Form, round::*, self};\nuse util::twice as double;\nuse other::*;\npub(crate) use shapes::describe;\npub use util::*;\nuse crate::shapes::round::Ring;\nuse self::util::helper;\nuse serde;\n",
        ),
        ("src/other.rs", "pub fn only_here() {}\n"),
        (
            "src/shapes/mod.rs",
            "pub mod round;\nuse super::util::twice;\nuse super::super::x;\npub struct Circle;\npub fn describe() {}\n",
        ),
        (
            "src/shapes/round.rs",
            "use super::Circle;\nuse crate::util::helper;\npub struct Ring;\n",
        ),
        ("src/util.rs", "pub fn twice() {}\npub fn helper() {}\n"),
    ];

    #[test]
    fn test_imports_of_the_crate_root() {
        use crate::index::imports::testing::rows;
        assert_eq!(
            rows(IMPORT_FILES, "src/main.rs"),
            [
                "1 ModuleDeclaration other -->other src/other.rs",
                "2 ModuleDeclaration shapes -->shapes src/shapes/mod.rs",
                "3 ModuleDeclaration util -->util src/util.rs",
                "4 ModuleDeclaration missing -->missing unresolved",
                "7 Named std::collections::HashMap HashMap->HashMap external",
                "8 Named shapes::Circle Circle->Circle src/shapes/mod.rs [src/shapes/mod.rs#Circle]",
                "8 Named shapes::Shape Shape->Form src/shapes/mod.rs",
                "8 Wildcard shapes::round -->- src/shapes/round.rs",
                "8 Named shapes shapes->shapes src/shapes/mod.rs",
                "9 Named util::twice twice->double src/util.rs [src/util.rs#twice]",
                "10 Wildcard other -->- src/other.rs",
                "11 ReExportNamed shapes::describe describe->describe src/shapes/mod.rs [src/shapes/mod.rs#describe]",
                "12 ReExportWildcard util -->- src/util.rs",
                "13 Named crate::shapes::round::Ring Ring->Ring src/shapes/round.rs [src/shapes/round.rs#Ring]",
                "14 Named self::util::helper helper->helper src/util.rs [src/util.rs#helper]",
                "15 Named serde serde->serde external",
            ]
        );
    }

    #[test]
    fn test_imports_of_nested_modules() {
        use crate::index::imports::testing::rows;
        assert_eq!(
            rows(IMPORT_FILES, "src/shapes/mod.rs"),
            [
                "1 ModuleDeclaration round -->round src/shapes/round.rs",
                "2 Named super::util::twice twice->twice src/util.rs [src/util.rs#twice]",
                "3 Named super::super::x x->x unresolved",
            ]
        );
        assert_eq!(
            rows(IMPORT_FILES, "src/shapes/round.rs"),
            [
                "1 Named super::Circle Circle->Circle src/shapes/mod.rs [src/shapes/mod.rs#Circle]",
                "2 Named crate::util::helper helper->helper src/util.rs [src/util.rs#helper]",
            ]
        );
    }

    #[test]
    fn test_named_import_follows_a_re_export() {
        use crate::index::imports::testing::rows;
        let files = [
            ("main.rs", "mod a;\nmod b;\nuse a::deep;\n"),
            ("a.rs", "pub use crate::b::deep;\npub use crate::b::*;\n"),
            ("b.rs", "pub use crate::a::*;\npub fn deep() {}\n"),
        ];
        assert_eq!(
            rows(&files, "main.rs")[2],
            "3 Named a::deep deep->deep a.rs [b.rs#deep]"
        );
    }
}
