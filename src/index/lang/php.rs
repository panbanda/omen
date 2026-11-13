//! PHP symbols.

use tree_sitter::Node;

use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

pub struct Php;

const METHOD_DECLARATION: &str = "method_declaration";
const ANONYMOUS_FUNCTION: &str = "anonymous_function";
const ARROW_FUNCTION: &str = "arrow_function";
const ASSIGNMENT_EXPRESSION: &str = "assignment_expression";
const VARIABLE_NAME: &str = "variable_name";
const NAME: &str = "name";
const QUALIFIED_NAME: &str = "qualified_name";
const VISIBILITY_MODIFIER: &str = "visibility_modifier";
const NAMESPACE_DEFINITION: &str = "namespace_definition";
const BASE_CLAUSE: &str = "base_clause";
const CLASS_INTERFACE_CLAUSE: &str = "class_interface_clause";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "function_definition",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        METHOD_DECLARATION,
        SymbolKind::Method,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "class_declaration",
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "interface_declaration",
        SymbolKind::Interface,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "trait_declaration",
        SymbolKind::Trait,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "enum_declaration",
        SymbolKind::Enum,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(ANONYMOUS_FUNCTION, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(ARROW_FUNCTION, SymbolKind::Function, NameRule::Hook),
];

fn is_closure(node: Node<'_>) -> bool {
    [ANONYMOUS_FUNCTION, ARROW_FUNCTION].contains(&node.kind())
}

/// The variable to which an assignment binds a closure.
fn closure_variable(closure: Node<'_>) -> Option<Node<'_>> {
    let assignment = closure
        .parent()
        .filter(|parent| parent.kind() == ASSIGNMENT_EXPRESSION)?;
    if assignment.child_by_field_name("right") != Some(closure) {
        return None;
    }
    assignment
        .child_by_field_name("left")
        .filter(|left| left.kind() == VARIABLE_NAME)
}

impl LanguageSpec for Php {
    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            ASSIGNMENT_EXPRESSION,
            VARIABLE_NAME,
            NAME,
            QUALIFIED_NAME,
            VISIBILITY_MODIFIER,
            NAMESPACE_DEFINITION,
            BASE_CLAUSE,
            CLASS_INTERFACE_CLAUSE,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    /// A closure has a name only when an assignment binds it to one variable.
    /// The name does not include the `$` of the variable.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        _context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        let variable = closure_variable(node)?;
        let mut cursor = variable.walk();
        let name = variable
            .named_children(&mut cursor)
            .find(|child| child.kind() == NAME);
        name
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        let node = draft.node;
        if is_closure(node) {
            if let Some(variable) = closure_variable(node) {
                draft.span = Span::between(&variable, &node);
            }
            return true;
        }
        if node.kind() == METHOD_DECLARATION && draft.name == "__construct" {
            draft.kind = SymbolKind::Constructor;
        }
        if draft.kind.is_type() {
            let mut cursor = node.walk();
            for clause in node.children(&mut cursor) {
                if ![BASE_CLAUSE, CLASS_INTERFACE_CLAUSE].contains(&clause.kind()) {
                    continue;
                }
                let mut cursor = clause.walk();
                for base in clause.named_children(&mut cursor) {
                    if [NAME, QUALIFIED_NAME].contains(&base.kind()) {
                        draft.supertypes.push(context.text(base));
                    }
                }
            }
        }
        true
    }

    /// A method is public unless it is `private` or `protected`. PHP makes a
    /// function that a function defines a global function.
    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let node = draft.node;
        if is_closure(node) {
            return Exported::NotExported;
        }
        if node.kind() != METHOD_DECLARATION {
            return Exported::Exported;
        }
        let mut cursor = node.walk();
        let restricted = node
            .children(&mut cursor)
            .any(|child| child.kind() == VISIBILITY_MODIFIER && context.text(child) != "public");
        if restricted {
            return Exported::NotExported;
        }
        context
            .scopes
            .iter()
            .rev()
            .find(|scope| scope.role == ScopeRole::Type)
            .map_or(Exported::Exported, |scope| scope.exported.clone())
    }

    /// A namespace with a body is a part of the key. It is not a symbol.
    fn scope<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        if node.kind() != NAMESPACE_DEFINITION {
            return None;
        }
        node.child_by_field_name("body")?;
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
    fn test_functions_and_bound_closures() {
        let source = "<?php\nnamespace Shop;\n\nfunction outer(): int\n{\n    function inner(): int\n    {\n        return 5;\n    }\n    return inner();\n}\n\nfunction closure(): int\n{\n    $add = function (int $n): int {\n        return $n + 1;\n    };\n    $double = fn(int $n): int => $n * 2;\n    array_map(function (int $n): int { return $n; }, [1]);\n    return $add(2) + $double(2);\n}\n";
        assert_eq!(
            rows(Language::Php, source),
            [
                "Function outer - 4-11 exported",
                "Function inner outer 6-9 exported",
                "Function closure - 13-21 exported",
                "Function add closure 15-17 not_exported",
                "Function double closure 18-18 not_exported",
            ]
        );
    }

    #[test]
    fn test_types_and_members() {
        let source = "<?php\ninterface Shape\n{\n    public function area(): float;\n}\n\ntrait Describes\n{\n    public function describe(): string\n    {\n        return '';\n    }\n}\n\nenum Unit: string\n{\n    case Metre = 'm';\n\n    public function label(): string\n    {\n        return '';\n    }\n}\n\nabstract class Base extends \\Shop\\Root implements Shape, Other\n{\n    use Describes;\n\n    public function __construct(protected string $name)\n    {\n    }\n\n    private static function build(): self\n    {\n    }\n\n    protected function radius(): float\n    {\n    }\n\n    function plain()\n    {\n    }\n}\n";
        assert_eq!(
            rows(Language::Php, source),
            [
                "Interface Shape - 2-5 exported",
                "Method area Shape 4-4 exported",
                "Trait Describes - 7-13 exported",
                "Method describe Describes 9-12 exported",
                "Enum Unit - 15-23 exported",
                "Method label Unit 19-22 exported",
                "Class Base - 25-44 exported",
                "Constructor __construct Base 29-31 exported",
                "Method build Base 33-35 not_exported",
                "Method radius Base 37-39 not_exported",
                "Method plain Base 41-43 exported",
            ]
        );
        assert_eq!(
            symbols(Language::Php, source)[6].supertypes,
            ["\\Shop\\Root", "Shape", "Other"]
        );
    }

    #[test]
    fn test_namespace_with_a_body_is_a_part_of_the_key() {
        let source = "<?php\nnamespace Shop\\Util {\n    function helper() {}\n    class Helpers {\n        public static function helper() {}\n    }\n}\n";
        assert_eq!(
            rows(Language::Php, source),
            [
                "Function helper - 3-3 exported",
                "Class Helpers - 4-6 exported",
                "Method helper Helpers 5-5 exported",
            ]
        );
        assert_eq!(
            keys(Language::Php, source),
            [
                "file#Shop\\Util.helper",
                "file#Shop\\Util.Helpers",
                "file#Shop\\Util.Helpers.helper"
            ]
        );
    }
}
