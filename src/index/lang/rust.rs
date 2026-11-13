//! Rust symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

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
    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> &'static [&'static str] {
        &[
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
}
