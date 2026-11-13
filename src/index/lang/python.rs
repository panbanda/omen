//! Python symbols.

use tree_sitter::Node;

use crate::index::extract::{
    find_descendant, Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind};

const IMPORT_STATEMENT: &str = "import_statement";
const IMPORT_FROM_STATEMENT: &str = "import_from_statement";
const ALIASED_IMPORT: &str = "aliased_import";
const WILDCARD_IMPORT: &str = "wildcard_import";

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
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![IMPORT_STATEMENT, IMPORT_FROM_STATEMENT]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![ALIASED_IMPORT, WILDCARD_IMPORT]
    }

    /// `import a.b` binds the name `a`. `import a.b as c` binds `c` to the
    /// module `a.b`. `from a import b` binds one name of the module `a`.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        // The name in the module, and the alias if the import has one.
        let parts = |name: Node<'_>| -> (String, Option<String>) {
            if name.kind() != ALIASED_IMPORT {
                return (context.text(name), None);
            }
            (
                name.child_by_field_name("name")
                    .map(|name| context.text(name))
                    .unwrap_or_default(),
                name.child_by_field_name("alias")
                    .map(|alias| context.text(alias)),
            )
        };
        let mut cursor = node.walk();
        let names: Vec<Node<'_>> = node.children_by_field_name("name", &mut cursor).collect();
        if node.kind() == IMPORT_STATEMENT {
            for name in names {
                let (module, alias) = parts(name);
                let (imported, local) = match alias {
                    Some(alias) => (module.clone(), alias),
                    None => {
                        let top = module.split('.').next().unwrap_or("").to_string();
                        (top.clone(), top)
                    }
                };
                out.push(context.import(
                    node,
                    module,
                    ImportKind::Module,
                    Some(imported),
                    Some(local),
                ));
            }
            return;
        }
        let Some(module) = node.child_by_field_name("module_name") else {
            return;
        };
        let module: String = context.text(module).split_whitespace().collect();
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|child| child.kind() == WILDCARD_IMPORT)
        {
            out.push(context.import(node, module.clone(), ImportKind::Wildcard, None, None));
        }
        for name in names {
            let (imported, alias) = parts(name);
            let local = alias.unwrap_or_else(|| imported.clone());
            out.push(context.import(
                node,
                module.clone(),
                ImportKind::Named,
                Some(imported),
                Some(local),
            ));
        }
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
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

    const IMPORT_FILES: &[(&str, &str)] = &[
        ("app/__init__.py", "from .shapes import describe\nfrom . import util\n"),
        (
            "app/main.py",
            "import math\nimport app.other as alt\nimport app.util, os\nfrom app import describe\nfrom app.other import *\nfrom app.shapes import (Circle, describe as tell)\nfrom app import util\nfrom app.missing import gone\nfrom ..outside import far\n",
        ),
        ("app/other.py", "def only_here():\n    pass\n"),
        (
            "app/shapes.py",
            "from .util import twice\nfrom . import other\nfrom .. import top\n\nclass Circle:\n    pass\n\ndef describe():\n    pass\n",
        ),
        ("app/util.py", "def twice():\n    pass\n"),
        ("top.py", "x = 1\n"),
    ];

    #[test]
    fn test_absolute_imports() {
        use crate::index::imports::testing::rows;
        assert_eq!(
            rows(IMPORT_FILES, "app/main.py"),
            [
                "1 Module math math->math external",
                "2 Module app.other app.other->alt app/other.py",
                "3 Module app.util app->app app/util.py",
                "3 Module os os->os external",
                "4 Named app describe->describe app/__init__.py [app/shapes.py#describe]",
                "5 Wildcard app.other -->- app/other.py",
                "6 Named app.shapes Circle->Circle app/shapes.py [app/shapes.py#Circle]",
                "6 Named app.shapes describe->tell app/shapes.py [app/shapes.py#describe]",
                "7 Named app util->util app/util.py",
                "8 Named app.missing gone->gone external",
                "9 Named ..outside far->far unresolved",
            ]
        );
    }

    #[test]
    fn test_relative_imports() {
        use crate::index::imports::testing::rows;
        assert_eq!(
            rows(IMPORT_FILES, "app/shapes.py"),
            [
                "1 Named .util twice->twice app/util.py [app/util.py#twice]",
                "2 Named . other->other app/other.py",
                "3 Named .. top->top top.py",
            ]
        );
    }

    #[test]
    fn test_two_modules_with_the_same_dotted_name_are_ambiguous() {
        use crate::index::imports::testing::rows;
        let files = [
            ("one/pkg/mod.py", ""),
            ("two/pkg/mod.py", ""),
            ("main.py", "import pkg.mod\n"),
        ];
        assert_eq!(
            rows(&files, "main.py"),
            ["1 Module pkg.mod pkg->pkg ambiguous(one/pkg/mod.py,two/pkg/mod.py)"]
        );
    }
}
