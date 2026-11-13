//! PHP symbols.

use tree_sitter::Node;

use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind, ImportTarget};

const NAMESPACE_USE_DECLARATION: &str = "namespace_use_declaration";
const NAMESPACE_USE_CLAUSE: &str = "namespace_use_clause";
const NAMESPACE_USE_GROUP: &str = "namespace_use_group";
const NAMESPACE_NAME: &str = "namespace_name";
const LOAD_EXPRESSIONS: &[&str] = &[
    "require_expression",
    "require_once_expression",
    "include_expression",
    "include_once_expression",
];
const BINARY_EXPRESSION: &str = "binary_expression";
const PARENTHESIZED_EXPRESSION: &str = "parenthesized_expression";
const STRING_LITERALS: &[&str] = &["string", "encapsed_string"];
const STRING_CONTENT: &str = "string_content";

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
    fn import_kinds(&self) -> Vec<&'static str> {
        [&[NAMESPACE_USE_DECLARATION][..], LOAD_EXPRESSIONS].concat()
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        let mut kinds = vec![
            NAMESPACE_USE_CLAUSE,
            NAMESPACE_USE_GROUP,
            NAMESPACE_NAME,
            BINARY_EXPRESSION,
            PARENTHESIZED_EXPRESSION,
            STRING_CONTENT,
        ];
        kinds.extend_from_slice(STRING_LITERALS);
        kinds
    }

    /// One binding for each clause of a `use` declaration. The path of a
    /// grouped clause is the prefix of the group. `require` and `include`
    /// give a binding only when the path ends with a string literal.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        if node.kind() != NAMESPACE_USE_DECLARATION {
            out.extend(load_binding(node, context));
            return;
        }
        let mut cursor = node.walk();
        let prefix = node
            .children(&mut cursor)
            .find(|child| child.kind() == NAMESPACE_NAME)
            .map(|prefix| context.text(prefix));
        let clauses = node.child_by_field_name("body").unwrap_or(node);
        let mut cursor = clauses.walk();
        for clause in clauses.children(&mut cursor) {
            if clause.kind() != NAMESPACE_USE_CLAUSE {
                continue;
            }
            let alias = clause.child_by_field_name("alias");
            let mut cursor = clause.walk();
            let path = clause
                .named_children(&mut cursor)
                .find(|child| Some(*child) != alias);
            let Some(path) = path else {
                continue;
            };
            let path = context.text(path);
            let path = path.trim_start_matches('\\');
            let (qualifier, name) = match path.rsplit_once('\\') {
                Some((qualifier, name)) => (Some(qualifier), name),
                None => (None, path),
            };
            let module = match (&prefix, qualifier) {
                (Some(prefix), Some(qualifier)) => format!("{prefix}\\{qualifier}"),
                (Some(prefix), None) => prefix.clone(),
                (None, _) => path.to_string(),
            };
            let local = alias.map_or_else(|| name.to_string(), |alias| context.text(alias));
            out.push(context.import(
                node,
                module,
                ImportKind::Named,
                Some(name.to_string()),
                Some(local),
            ));
        }
    }

    fn module_kinds(&self) -> Vec<&'static str> {
        vec![NAMESPACE_DEFINITION]
    }

    fn module_name(&self, node: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
        node.child_by_field_name("name")
            .map(|name| context.text(name))
    }

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

/// The text of a string literal that has no interpolation.
fn literal(string: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
    if !STRING_LITERALS.contains(&string.kind()) {
        return None;
    }
    match string.named_child_count() {
        0 => Some(String::new()),
        1 => string
            .named_child(0)
            .filter(|content| content.kind() == STRING_CONTENT)
            .map(|content| context.text(content)),
        _ => None,
    }
}

/// The binding of a `require` or an `include` expression.
///
/// A literal path and `__DIR__ . 'literal'` are relative to the file. With a
/// different prefix, the binding starts as external, and the module map then
/// looks only for a unique file that ends with the literal.
fn load_binding(node: Node<'_>, context: &Context<'_, '_>) -> Option<ImportBinding> {
    let mut argument = node.named_child(0)?;
    while argument.kind() == PARENTHESIZED_EXPRESSION {
        argument = argument.named_child(0)?;
    }
    let (path, relative_to_file) = if argument.kind() == BINARY_EXPRESSION {
        let left = context.text(argument.child_by_field_name("left")?);
        let left: String = left.split_whitespace().collect();
        let path = literal(argument.child_by_field_name("right")?, context)?;
        (
            path,
            ["__DIR__", "dirname(__FILE__)"].contains(&left.as_str()),
        )
    } else {
        (literal(argument, context)?, true)
    };
    let mut binding = context.import(node, path, ImportKind::Load, None, None);
    if !relative_to_file {
        binding.target_file = ImportTarget::External;
    }
    Some(binding)
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

    #[test]
    fn test_use_declarations() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "main.php",
                "<?php\nnamespace Shop;\n\nuse ArrayObject;\nuse Shop\\Other;\nuse Shop\\Shapes\\Circle;\nuse Shop\\Shapes\\{Shape, Square as Box, Sub\\Deep};\nuse Shop\\Util\\Helpers as Tools;\nuse function Shop\\Util\\helper;\nuse Shop\\Shapes\\Missing, \\Shop\\Util\\Helpers;\n",
            ),
            ("other.php", "<?php\nnamespace Shop\\Other;\n\nfunction only_here() {}\n"),
            (
                "shapes.php",
                "<?php\nnamespace Shop\\Shapes;\n\ninterface Shape {}\nclass Circle {}\nclass Square {}\n",
            ),
            ("deep.php", "<?php\nnamespace Shop\\Shapes\\Sub;\n\nclass Deep {}\n"),
            (
                "util.php",
                "<?php\nnamespace Shop\\Util;\n\nfunction helper() {}\nfinal class Helpers {}\n",
            ),
        ];
        assert_eq!(
            rows(&files, "main.php"),
            [
                "4 Named ArrayObject ArrayObject->ArrayObject external",
                "5 Named Shop\\Other Other->Other other.php",
                "6 Named Shop\\Shapes\\Circle Circle->Circle shapes.php [shapes.php#Circle]",
                "7 Named Shop\\Shapes Shape->Shape shapes.php [shapes.php#Shape]",
                "7 Named Shop\\Shapes Square->Box shapes.php [shapes.php#Square]",
                "7 Named Shop\\Shapes\\Sub Deep->Deep deep.php [deep.php#Deep]",
                "8 Named Shop\\Util\\Helpers Helpers->Tools util.php [util.php#Helpers]",
                "9 Named Shop\\Util\\helper helper->helper util.php [util.php#helper]",
                "10 Named Shop\\Shapes\\Missing Missing->Missing unresolved",
                "10 Named Shop\\Util\\Helpers Helpers->Helpers util.php [util.php#Helpers]",
            ]
        );
    }

    #[test]
    fn test_require_and_include() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "app/main.php",
                "<?php\nrequire_once __DIR__ . '/shapes.php';\ninclude 'util.php';\nrequire(\"lib/x.php\");\nrequire $base . '/lib/x.php';\ninclude_once $base . '/gone.php';\nrequire $name;\nrequire \"$dir/y.php\";\nrequire __DIR__ . '/missing.php';\n",
            ),
            ("app/shapes.php", "<?php\n"),
            ("app/util.php", "<?php\n"),
            ("app/lib/x.php", "<?php\n"),
        ];
        assert_eq!(
            rows(&files, "app/main.php"),
            [
                "2 Load /shapes.php -->- app/shapes.php",
                "3 Load util.php -->- app/util.php",
                "4 Load lib/x.php -->- app/lib/x.php",
                "5 Load /lib/x.php -->- app/lib/x.php",
                "6 Load /gone.php -->- external",
                "9 Load /missing.php -->- unresolved",
            ]
        );
    }
}
