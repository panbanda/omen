//! C symbols, and the declarator rules that C++ shares.

use tree_sitter::Node;

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind, ImportTarget};

pub(crate) const PREPROC_INCLUDE: &str = "preproc_include";
pub(crate) const SYSTEM_LIB_STRING: &str = "system_lib_string";

use crate::index::calls::{CallSyntax, LocalName};

pub(crate) const PARAMETER_DECLARATION: &str = "parameter_declaration";
const INIT_DECLARATOR: &str = "init_declarator";

pub(crate) const CALL_SYNTAX: CallSyntax = CallSyntax {
    calls: &[("call_expression", "function", "arguments")],
    constructions: &[],
    members: &[("field_expression", "argument", "field")],
    paths: &[],
    names: &[IDENTIFIER],
    self_words: &[],
    super_words: &[],
    locals: &[],
    parameters: &[],
    return_type_field: "type",
    literals: &[("number_literal", "number")],
};

pub struct C;

pub(crate) const FUNCTION_DEFINITION: &str = "function_definition";
pub(crate) const FUNCTION_DECLARATOR: &str = "function_declarator";
pub(crate) const DECLARATION: &str = "declaration";
pub(crate) const FIELD_DECLARATION: &str = "field_declaration";
pub(crate) const TYPE_DEFINITION: &str = "type_definition";
pub(crate) const STRUCT_SPECIFIER: &str = "struct_specifier";
pub(crate) const UNION_SPECIFIER: &str = "union_specifier";
pub(crate) const ENUM_SPECIFIER: &str = "enum_specifier";
pub(crate) const POINTER_DECLARATOR: &str = "pointer_declarator";
pub(crate) const PARENTHESIZED_DECLARATOR: &str = "parenthesized_declarator";
pub(crate) const ARRAY_DECLARATOR: &str = "array_declarator";
pub(crate) const ATTRIBUTED_DECLARATOR: &str = "attributed_declarator";
/// Only the C++ grammar has this kind.
pub(crate) const REFERENCE_DECLARATOR: &str = "reference_declarator";
pub(crate) const STORAGE_CLASS_SPECIFIER: &str = "storage_class_specifier";
pub(crate) const IDENTIFIER: &str = "identifier";
pub(crate) const TYPE_IDENTIFIER: &str = "type_identifier";

pub(crate) const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::function(FUNCTION_DEFINITION, NameRule::Hook),
    DefinitionRule::function(FUNCTION_DECLARATOR, NameRule::Hook),
    DefinitionRule::new(
        STRUCT_SPECIFIER,
        SymbolKind::Struct,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(UNION_SPECIFIER, SymbolKind::Struct, NameRule::Field("name")),
    DefinitionRule::new(ENUM_SPECIFIER, SymbolKind::Enum, NameRule::Field("name")),
    DefinitionRule::new(TYPE_DEFINITION, SymbolKind::TypeAlias, NameRule::Hook),
];

pub(crate) const HOOK_KINDS: &[&str] = &[
    DECLARATION,
    FIELD_DECLARATION,
    POINTER_DECLARATOR,
    PARENTHESIZED_DECLARATOR,
    ARRAY_DECLARATOR,
    ATTRIBUTED_DECLARATOR,
    STORAGE_CLASS_SPECIFIER,
    IDENTIFIER,
    TYPE_IDENTIFIER,
];

/// A declarator that contains one declarator and adds no name.
const WRAPPER_DECLARATORS: &[&str] = &[
    POINTER_DECLARATOR,
    PARENTHESIZED_DECLARATOR,
    ARRAY_DECLARATOR,
    ATTRIBUTED_DECLARATOR,
    REFERENCE_DECLARATOR,
];

fn inner_declarator(declarator: Node<'_>) -> Option<Node<'_>> {
    declarator
        .child_by_field_name("declarator")
        .or_else(|| declarator.named_child(0))
}

/// Finds the name of the function that a declarator declares.
///
/// `int (*table)(int)` declares a pointer, not a function, and gives `None`.
/// `int (*get(void))(int)` declares the function `get`.
pub(crate) fn function_name<'tree>(
    declarator: Node<'tree>,
    name_kinds: &[&str],
) -> Option<Node<'tree>> {
    match declarator.kind() {
        FUNCTION_DECLARATOR => {
            let inner = declarator.child_by_field_name("declarator")?;
            if name_kinds.contains(&inner.kind()) {
                Some(inner)
            } else {
                function_name(inner, name_kinds)
            }
        }
        kind if WRAPPER_DECLARATORS.contains(&kind) => {
            function_name(inner_declarator(declarator)?, name_kinds)
        }
        _ => None,
    }
}

/// The declaration that a function declarator belongs to, when the declarator
/// declares a function and does not define it.
pub(crate) fn prototype_holder(declarator: Node<'_>) -> Option<Node<'_>> {
    let mut current = declarator.parent()?;
    while WRAPPER_DECLARATORS.contains(&current.kind()) || current.kind() == FUNCTION_DECLARATOR {
        current = current.parent()?;
    }
    [DECLARATION, FIELD_DECLARATION]
        .contains(&current.kind())
        .then_some(current)
}

/// The name of the first declarator of a `typedef`.
pub(crate) fn typedef_name(definition: Node<'_>) -> Option<Node<'_>> {
    let mut declarator = definition.child_by_field_name("declarator")?;
    while declarator.kind() != TYPE_IDENTIFIER {
        let wrapper = WRAPPER_DECLARATORS.contains(&declarator.kind())
            || declarator.kind() == FUNCTION_DECLARATOR;
        if !wrapper {
            return None;
        }
        declarator = inner_declarator(declarator)?;
    }
    Some(declarator)
}

pub(crate) fn has_storage_class(node: Node<'_>, keyword: &str, context: &Context<'_, '_>) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == STORAGE_CLASS_SPECIFIER && context.text(child) == keyword);
    found
}

/// A header declares the interface of a translation unit.
pub(crate) fn is_header(file: &str) -> bool {
    let extension = file.rsplit('.').next().unwrap_or("");
    ["h", "hh", "hpp", "hxx", "h++"].contains(&extension)
}

/// The name of a function definition, a function prototype, or a `typedef`.
/// The result is `None` for a function declarator that is a part of a
/// definition, a parameter, or a pointer.
pub(crate) fn name_node<'tree>(
    node: Node<'tree>,
    name_kinds: &[&str],
    context: &Context<'_, 'tree>,
) -> Option<Node<'tree>> {
    match node.kind() {
        FUNCTION_DEFINITION => function_name(node.child_by_field_name("declarator")?, name_kinds),
        FUNCTION_DECLARATOR => {
            prototype_holder(node)?;
            // In a function body, the grammar reads `Type name(argument);` as a
            // prototype, but it is usually a variable with a constructor call.
            if context.in_function() {
                return None;
            }
            node.child_by_field_name("declarator")
                .filter(|inner| name_kinds.contains(&inner.kind()))
        }
        TYPE_DEFINITION => typedef_name(node),
        _ => None,
    }
}

/// The node that has the storage class and the range of a function symbol.
pub(crate) fn function_holder(node: Node<'_>) -> Node<'_> {
    if node.kind() == FUNCTION_DECLARATOR {
        prototype_holder(node).unwrap_or(node)
    } else {
        node
    }
}

/// Corrects the draft of a C or C++ symbol. If the result is `false`, the node
/// is not a definition.
pub(crate) fn refine(draft: &mut Draft<'_>) -> bool {
    match draft.node.kind() {
        // `struct Name` without a body refers to a type. It does not define the type.
        STRUCT_SPECIFIER | UNION_SPECIFIER | ENUM_SPECIFIER => draft.body.is_some(),
        FUNCTION_DECLARATOR => {
            draft.span = Span::of(&function_holder(draft.node));
            true
        }
        _ => true,
    }
}

fn exported_if(condition: bool) -> Exported {
    if condition {
        Exported::Exported
    } else {
        Exported::NotExported
    }
}

/// A function has external linkage unless it is `static`. A type is a part of
/// the interface only when a header defines it.
pub(crate) fn exported(draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
    if context.in_function() {
        return Exported::NotExported;
    }
    match draft.node.kind() {
        FUNCTION_DEFINITION | FUNCTION_DECLARATOR => exported_if(!has_storage_class(
            function_holder(draft.node),
            "static",
            context,
        )),
        _ => exported_if(is_header(context.file)),
    }
}

impl LanguageSpec for C {
    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        vec![DECLARATION, PARAMETER_DECLARATION]
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![PARAMETER_DECLARATION, INIT_DECLARATOR]
    }

    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        declared_names(node, context, out);
    }

    fn import_kinds(&self) -> Vec<&'static str> {
        vec![PREPROC_INCLUDE]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![SYSTEM_LIB_STRING]
    }

    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        include_binding(node, context, out);
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        HOOK_KINDS.to_vec()
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        name_node(node, &[IDENTIFIER], context)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, _context: &Context<'_, 'tree>) -> bool {
        refine(draft)
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        exported(draft, context)
    }
}

/// One binding for an `#include`. The path has no quotation marks and no
/// angle brackets. An include with angle brackets starts as external, and the
/// module map then looks only for a unique file that ends with the path.
pub(crate) fn include_binding(
    node: Node<'_>,
    context: &Context<'_, '_>,
    out: &mut Vec<ImportBinding>,
) {
    let Some(path) = node.child_by_field_name("path") else {
        return;
    };
    let text = context.text(path);
    let system = path.kind() == SYSTEM_LIB_STRING;
    let text = text.trim_matches(|mark| mark == '"' || mark == '<' || mark == '>');
    let mut binding = context.import(node, text, ImportKind::Load, None, None);
    if system {
        binding.target_file = ImportTarget::External;
    }
    out.push(binding);
}

/// Adds the names that a declaration or a parameter declares. The name is in
/// a chain of declarators, and the type is on the declaration.
///
/// A declaration of a function outside a function body is a prototype. It
/// is not a local name.
pub(crate) fn declared_names(node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
    let type_name = node
        .child_by_field_name("type")
        .map(|type_name| context.text(type_name));
    let mut cursor = node.walk();
    for declarator in node.children_by_field_name("declarator", &mut cursor) {
        let (mut current, value) = match declarator.kind() {
            INIT_DECLARATOR => (
                declarator.child_by_field_name("declarator"),
                declarator.child_by_field_name("value"),
            ),
            _ => (Some(declarator), None),
        };
        let mut function = false;
        while let Some(inner) = current.filter(|inner| inner.kind() != IDENTIFIER) {
            function |= inner.kind() == FUNCTION_DECLARATOR;
            current = inner_declarator(inner);
        }
        let Some(name) = current else {
            continue;
        };
        if function && !context.in_function() && node.kind() == DECLARATION {
            continue;
        }
        out.push(LocalName {
            name: context.text(name),
            type_name: type_name.clone(),
            value_start: value.map(|value| value.start_byte()),
            is_parameter: node.kind() != DECLARATION,
            element: false,
            result_index: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{rows, rows_in};

    #[test]
    fn test_function_definitions() {
        let source = "static int tag(void) {\n    return 1;\n}\n\nint main(void) {\n    return 0;\n}\n\nconst char *describe(void) {\n    return \"\";\n}\n\nextern int shared(void) { return 1; }\n";
        assert_eq!(
            rows(Language::C, source),
            [
                "Function tag - 1-3 not_exported",
                "Function main - 5-7 exported",
                "Function describe - 9-11 exported",
                "Function shared - 13-13 exported",
            ]
        );
    }

    #[test]
    fn test_prototypes_are_symbols() {
        let source = "double circle_area(const struct Circle *circle);\nconst char *describe(void);\nstatic int tag(void);\nint first(void), second(int value);\nint (*get(void))(int);\n";
        assert_eq!(
            rows(Language::C, source),
            [
                "Function circle_area - 1-1 exported",
                "Function describe - 2-2 exported",
                "Function tag - 3-3 not_exported",
                "Function first - 4-4 exported",
                "Function second - 4-4 exported",
                "Function get - 5-5 exported",
            ]
        );
    }

    #[test]
    fn test_function_pointers_and_parameters_are_not_functions() {
        let source = "int (*handler)(int);\nstruct Table {\n    int (*insert)(int);\n};\nint apply(int f(int), int (*g)(int)) {\n    int local(int);\n    return f(1) + g(2);\n}\n";
        assert_eq!(
            rows(Language::C, source),
            [
                "Struct Table - 2-4 not_exported",
                "Function apply - 5-8 exported",
            ]
        );
    }

    #[test]
    fn test_types_are_exported_only_from_a_header() {
        let source = "struct Circle {\n    double radius;\n};\nenum Unit {\n    METRE\n};\nunion Bits {\n    int a;\n};\ntypedef double Scale;\ntypedef struct {\n    int x;\n} Point;\ntypedef int (*Handler)(int);\nstruct Circle;\nstruct Circle *make(void);\n";
        let expected = |exported: &str| {
            vec![
                format!("Struct Circle - 1-3 {exported}"),
                format!("Enum Unit - 4-6 {exported}"),
                format!("Struct Bits - 7-9 {exported}"),
                format!("TypeAlias Scale - 10-10 {exported}"),
                format!("TypeAlias Point - 11-13 {exported}"),
                format!("TypeAlias Handler - 14-14 {exported}"),
                "Function make - 16-16 exported".to_string(),
            ]
        };
        assert_eq!(
            rows_in(Language::C, "shapes.h", source),
            expected("exported")
        );
        assert_eq!(
            rows_in(Language::C, "shapes.c", source),
            expected("not_exported")
        );
    }

    #[test]
    fn test_definitions_in_a_header_guard() {
        let source =
            "#ifndef A_H\n#define A_H\n#define SQUARE(x) ((x) * (x))\nint helper(void);\n#endif\n";
        assert_eq!(
            rows_in(Language::C, "a.h", source),
            ["Function helper - 4-4 exported"]
        );
    }

    #[test]
    fn test_includes() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "main.c",
                "#include <stdio.h>\n#include \"other.h\"\n#include \"sub/util.h\"\n#include \"deep.h\"\n#include \"twice.h\"\n#include \"nothing.h\"\n#include <sub/util.h>\n#include \"../outside.h\"\n",
            ),
            ("other.h", ""),
            ("util.h", ""),
            ("sub/util.h", ""),
            ("sub/util.c", "#include \"util.h\"\n"),
            ("x/deep.h", ""),
            ("a/twice.h", ""),
            ("b/twice.h", ""),
        ];
        assert_eq!(
            rows(&files, "main.c"),
            [
                "1 Load stdio.h -->- external",
                "2 Load other.h -->- other.h",
                "3 Load sub/util.h -->- sub/util.h",
                "4 Load deep.h -->- x/deep.h",
                "5 Load twice.h -->- ambiguous(a/twice.h,b/twice.h)",
                "6 Load nothing.h -->- unresolved",
                "7 Load sub/util.h -->- sub/util.h",
                "8 Load ../outside.h -->- unresolved",
            ]
        );
        assert_eq!(
            rows(&files, "sub/util.c"),
            ["1 Load util.h -->- sub/util.h"]
        );
    }

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "main.c",
            "#include <string.h>\n\n#include \"other.h\"\n#include \"sub/util.h\"\n\n#define SQUARE(x) ((x) * (x))\n\nstruct Table {\n    int (*insert)(int);\n};\n\nstatic int tag(void) {\n    return 1;\n}\n\nstatic int forward(void);\n\nstatic int apply(int (*f)(int)) {\n    return f(1) + forward();\n}\n\nint run(void) {\n    return helper() + only_here() + tag() + apply(twice) + hidden() + missing();\n}\n\nint dynamic(struct Table table) {\n    int (*pick)(int) = twice;\n    return pick(1) + table.insert(1) + (int)strlen(\"x\") + SQUARE(2);\n}\n\nstatic int forward(void) {\n    return 2;\n}\n",
        ),
        ("other.h", "int only_here(void);\nint insert(int value);\n"),
        (
            "other.c",
            "#include \"other.h\"\n\nstatic int tag(void) {\n    return 2;\n}\n\nstatic int hidden(void) {\n    return 0;\n}\n\nint only_here(void) {\n    return tag();\n}\n\nint insert(int value) {\n    return value;\n}\n",
        ),
        ("sub/util.h", "int helper(void);\nint twice(int value);\n"),
        (
            "sub/util.c",
            "#include \"util.h\"\n\nint helper(void) {\n    return 1;\n}\n\nint twice(int value) {\n    return value * 2;\n}\n",
        ),
    ];

    #[test]
    fn test_calls_bind_to_definitions() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.c"),
            [
                "19 apply>f Unresolved - []",
                "19 apply>forward UniqueCandidate SameFile [main.c#forward~2]",
                "23 run>helper UniqueCandidate ImportedModule [sub/util.c#helper]",
                "23 run>only_here UniqueCandidate ImportedModule [other.c#only_here]",
                "23 run>tag UniqueCandidate SameFile [main.c#tag]",
                "23 run>apply UniqueCandidate SameFile [main.c#apply]",
                "23 run>hidden Unresolved - []",
                "23 run>missing Unresolved - []",
                "28 dynamic>pick Unresolved - []",
                "28 dynamic>table.insert Unresolved - []",
                "28 dynamic>strlen Unresolved - []",
                "28 dynamic>SQUARE Unresolved - []",
            ]
        );
        assert_eq!(
            rows(CALL_FILES, "other.c"),
            ["12 only_here>tag UniqueCandidate SameFile [other.c#tag]"]
        );
    }
}
