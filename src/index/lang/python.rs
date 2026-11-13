//! Python symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

pub struct Python;

const FUNCTION_DEFINITION: &str = "function_definition";
const CLASS_DEFINITION: &str = "class_definition";
const LAMBDA: &str = "lambda";
const ASSIGNMENT: &str = "assignment";
const AUGMENTED_ASSIGNMENT: &str = "augmented_assignment";
const TYPE_ALIAS_STATEMENT: &str = "type_alias_statement";
const EXPRESSION_STATEMENT: &str = "expression_statement";
const IDENTIFIER: &str = "identifier";
const KEYWORD_ARGUMENT: &str = "keyword_argument";
const LIST: &str = "list";
const TUPLE: &str = "tuple";
const STRING: &str = "string";
const STRING_CONTENT: &str = "string_content";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::function(FUNCTION_DEFINITION, NameRule::Field("name")),
    DefinitionRule::new(CLASS_DEFINITION, SymbolKind::Class, NameRule::Field("name")),
    DefinitionRule::new(LAMBDA, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(ASSIGNMENT, SymbolKind::TypeAlias, NameRule::Hook),
    DefinitionRule::new(TYPE_ALIAS_STATEMENT, SymbolKind::TypeAlias, NameRule::Hook),
];

/// The names that a literal `__all__` lists.
enum PublicNames {
    /// The module does not assign `__all__`.
    NotDeclared,
    Listed(Vec<String>),
    /// The module calculates `__all__`.
    NotLiteral,
}

fn public_names(module: Node<'_>, context: &Context<'_, '_>) -> PublicNames {
    let mut result = PublicNames::NotDeclared;
    let mut cursor = module.walk();
    for statement in module.children(&mut cursor) {
        if statement.kind() != EXPRESSION_STATEMENT {
            continue;
        }
        let Some(assignment) = statement.named_child(0) else {
            continue;
        };
        let targets_all = assignment
            .child_by_field_name("left")
            .is_some_and(|left| left.kind() == IDENTIFIER && context.text(left) == "__all__");
        if !targets_all {
            continue;
        }
        let value = assignment
            .child_by_field_name("right")
            .filter(|value| [LIST, TUPLE].contains(&value.kind()));
        let (ASSIGNMENT, PublicNames::NotDeclared, Some(value)) =
            (assignment.kind(), &result, value)
        else {
            return PublicNames::NotLiteral;
        };
        let mut names = Vec::new();
        let mut cursor = value.walk();
        for item in value.named_children(&mut cursor) {
            let content = (item.kind() == STRING)
                .then(|| find_descendant(item, &[STRING_CONTENT]))
                .flatten();
            match content {
                Some(content) => names.push(context.text(content)),
                None => return PublicNames::NotLiteral,
            }
        }
        result = PublicNames::Listed(names);
    }
    result
}

impl LanguageSpec for Python {
    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> &'static [&'static str] {
        &[
            AUGMENTED_ASSIGNMENT,
            EXPRESSION_STATEMENT,
            IDENTIFIER,
            KEYWORD_ARGUMENT,
            LIST,
            TUPLE,
            STRING,
            STRING_CONTENT,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        match node.kind() {
            // A lambda has a name only when an assignment binds it to one variable.
            LAMBDA => {
                let assignment = node.parent().filter(|parent| parent.kind() == ASSIGNMENT)?;
                if assignment.child_by_field_name("right") != Some(node) {
                    return None;
                }
                assignment
                    .child_by_field_name("left")
                    .filter(|left| left.kind() == IDENTIFIER)
            }
            // `Name: TypeAlias = ...` is the type alias syntax before Python 3.12.
            ASSIGNMENT => {
                let annotation = context.text(node.child_by_field_name("type")?);
                if annotation != "TypeAlias" && !annotation.ends_with(".TypeAlias") {
                    return None;
                }
                node.child_by_field_name("left")
                    .filter(|left| left.kind() == IDENTIFIER)
            }
            TYPE_ALIAS_STATEMENT => {
                find_descendant(node.child_by_field_name("left")?, &[IDENTIFIER])
            }
            _ => None,
        }
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        match draft.node.kind() {
            FUNCTION_DEFINITION => {
                if draft.kind == SymbolKind::Method && draft.name == "__init__" {
                    draft.kind = SymbolKind::Constructor;
                }
            }
            CLASS_DEFINITION => {
                if let Some(bases) = draft.node.child_by_field_name("superclasses") {
                    let mut cursor = bases.walk();
                    draft.supertypes = bases
                        .named_children(&mut cursor)
                        .filter(|base| base.kind() != KEYWORD_ARGUMENT)
                        .map(|base| context.text(base))
                        .collect();
                }
            }
            LAMBDA => draft.span = Span::between(&draft.name_node, &draft.node),
            _ => {}
        }
        true
    }

    /// Python has no visibility syntax. A name that starts with `_` is private
    /// by convention, and a literal `__all__` lists the public names of a module.
    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let dunder = draft.name.starts_with("__") && draft.name.ends_with("__");
        if context.in_function() || (draft.name.starts_with('_') && !dunder) {
            return Exported::NotExported;
        }
        if let Some(scope) = context
            .scopes
            .iter()
            .rev()
            .find(|scope| scope.role == ScopeRole::Type)
        {
            return scope.exported.clone();
        }
        let mut module = draft.node;
        while let Some(parent) = module.parent() {
            module = parent;
        }
        match public_names(module, context) {
            PublicNames::NotDeclared => Exported::Exported,
            PublicNames::Listed(names) if names.contains(&draft.name) => Exported::Exported,
            PublicNames::Listed(_) => Exported::NotExported,
            PublicNames::NotLiteral => {
                Exported::Unknown("the module calculates `__all__`".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_functions_and_visibility() {
        let source = "def run():\n    def inner():\n        return 5\n    add = lambda n: n + 1\n    list(map(lambda n: n, []))\n    return inner() + add(1)\n\ndef _hidden():\n    return 7\n\nhandler = lambda: 1\n";
        assert_eq!(
            rows(Language::Python, source),
            [
                "Function run - 1-6 exported",
                "Function inner run 2-3 not_exported",
                "Function add run 4-4 not_exported",
                "Function _hidden - 8-9 not_exported",
                "Function handler - 11-11 exported",
            ]
        );
    }

    #[test]
    fn test_classes_and_methods() {
        let source = "class Circle(Shape, metaclass=Meta):\n    def __init__(self):\n        self.r = 1\n\n    def area(self):\n        def helper():\n            return 1\n        return helper()\n\n    def _scaled(self):\n        return 2\n\n    @staticmethod\n    def unit():\n        return Circle()\n\n    class Inner:\n        def go(self):\n            pass\n\nclass _Hidden:\n    def visible(self):\n        pass\n";
        assert_eq!(
            rows(Language::Python, source),
            [
                "Class Circle - 1-19 exported",
                "Constructor __init__ Circle 2-3 exported",
                "Method area Circle 5-8 exported",
                "Function helper area 6-7 not_exported",
                "Method _scaled Circle 10-11 not_exported",
                "Method unit Circle 14-15 exported",
                "Class Inner Circle 17-19 exported",
                "Method go Inner 18-19 exported",
                "Class _Hidden - 21-23 not_exported",
                "Method visible _Hidden 22-23 not_exported",
            ]
        );
        assert_eq!(symbols(Language::Python, source)[0].supertypes, ["Shape"]);
        assert_eq!(keys(Language::Python, source)[7], "file#Circle.Inner.go");
    }

    #[test]
    fn test_type_aliases() {
        let source = "from typing import TypeAlias\nimport typing\n\nScale: TypeAlias = float\nRatio: typing.TypeAlias = float\ntype Pair = tuple[int, int]\ncount: int = 3\nname = 'x'\n";
        assert_eq!(
            rows(Language::Python, source),
            [
                "TypeAlias Scale - 4-4 exported",
                "TypeAlias Ratio - 5-5 exported",
                "TypeAlias Pair - 6-6 exported",
            ]
        );
    }

    #[test]
    fn test_literal_all_lists_the_public_names() {
        let source = "__all__ = ['area', \"Circle\"]\n\ndef area():\n    pass\n\ndef other():\n    pass\n\nclass Circle:\n    def go(self):\n        pass\n";
        assert_eq!(
            rows(Language::Python, source),
            [
                "Function area - 3-4 exported",
                "Function other - 6-7 not_exported",
                "Class Circle - 9-11 exported",
                "Method go Circle 10-11 exported",
            ]
        );
    }

    #[test]
    fn test_calculated_all_gives_unknown_visibility() {
        let source = "__all__ = ['area']\n__all__ += extra()\n\ndef area():\n    pass\n\ndef _private():\n    pass\n";
        assert_eq!(
            rows(Language::Python, source),
            [
                "Function area - 4-5 unknown",
                "Function _private - 7-8 not_exported",
            ]
        );
    }
}
