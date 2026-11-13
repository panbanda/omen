//! JavaScript, JSX, TypeScript, and TSX symbols.

use tree_sitter::Node;

use crate::core::Language;
use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole};
use crate::index::model::{Exported, Span, SymbolKind};
use crate::parser::function_binding_name;

use crate::index::model::{ImportBinding, ImportKind};

const IMPORT_STATEMENT: &str = "import_statement";
const IMPORT_CLAUSE: &str = "import_clause";
const NAMESPACE_IMPORT: &str = "namespace_import";
const NAMED_IMPORTS: &str = "named_imports";
const IMPORT_SPECIFIER: &str = "import_specifier";
const NAMESPACE_EXPORT: &str = "namespace_export";
const CALL_EXPRESSION: &str = "call_expression";
const OBJECT_PATTERN: &str = "object_pattern";
const SHORTHAND_PATTERN: &str = "shorthand_property_identifier_pattern";
const PAIR_PATTERN: &str = "pair_pattern";
const STRING: &str = "string";
const IMPORT_REQUIRE_CLAUSE: &str = "import_require_clause";

use crate::index::calls::{CallSyntax, LocalName};

const FORMAL_PARAMETERS: &str = "formal_parameters";

const JAVASCRIPT_CALLS: CallSyntax = CallSyntax {
    calls: &[(CALL_EXPRESSION, "function", "arguments")],
    constructions: &[("new_expression", "constructor", "arguments")],
    members: &[(MEMBER_EXPRESSION, "object", "property")],
    paths: &[],
    names: &[IDENTIFIER],
    self_words: &["this"],
    super_words: &["super"],
    locals: &[(VARIABLE_DECLARATOR, "name", "", "value")],
    parameters: &[],
    return_type_field: "",
    literals: &[],
};

const TYPESCRIPT_CALLS: CallSyntax = CallSyntax {
    locals: &[(VARIABLE_DECLARATOR, "name", "type", "value")],
    parameters: &[
        ("required_parameter", "pattern", "type"),
        ("optional_parameter", "pattern", "type"),
    ],
    return_type_field: "return_type",
    ..JAVASCRIPT_CALLS
};

pub struct Ecma {
    typescript: bool,
}

pub static JAVASCRIPT: Ecma = Ecma { typescript: false };
pub static TYPESCRIPT: Ecma = Ecma { typescript: true };

const ARROW_FUNCTION: &str = "arrow_function";
const FUNCTION_EXPRESSION: &str = "function_expression";
const GENERATOR_FUNCTION: &str = "generator_function";
const CLASS_EXPRESSION: &str = "class";
const CLASS_DECLARATION: &str = "class_declaration";
const ABSTRACT_CLASS_DECLARATION: &str = "abstract_class_declaration";
const INTERFACE_DECLARATION: &str = "interface_declaration";
const METHOD_DEFINITION: &str = "method_definition";
const METHOD_SIGNATURE: &str = "method_signature";
const ABSTRACT_METHOD_SIGNATURE: &str = "abstract_method_signature";
const VARIABLE_DECLARATOR: &str = "variable_declarator";
const PAIR: &str = "pair";
const FIELD_DEFINITION: &str = "field_definition";
const PUBLIC_FIELD_DEFINITION: &str = "public_field_definition";
const ASSIGNMENT_EXPRESSION: &str = "assignment_expression";
const EXPORT_STATEMENT: &str = "export_statement";
const EXPORT_CLAUSE: &str = "export_clause";
const EXPORT_SPECIFIER: &str = "export_specifier";
const EXPRESSION_STATEMENT: &str = "expression_statement";
const MEMBER_EXPRESSION: &str = "member_expression";
const OBJECT: &str = "object";
const SHORTHAND_PROPERTY_IDENTIFIER: &str = "shorthand_property_identifier";
const IDENTIFIER: &str = "identifier";
const TYPE_IDENTIFIER: &str = "type_identifier";
const NESTED_TYPE_IDENTIFIER: &str = "nested_type_identifier";
const PROPERTY_IDENTIFIER: &str = "property_identifier";
const PRIVATE_PROPERTY_IDENTIFIER: &str = "private_property_identifier";
const ACCESSIBILITY_MODIFIER: &str = "accessibility_modifier";
const DECORATOR: &str = "decorator";
const CLASS_HERITAGE: &str = "class_heritage";
const EXTENDS_TYPE_CLAUSE: &str = "extends_type_clause";
const TYPE_ARGUMENTS: &str = "type_arguments";
const PARENTHESIZED_EXPRESSION: &str = "parenthesized_expression";
const AS_EXPRESSION: &str = "as_expression";
const SATISFIES_EXPRESSION: &str = "satisfies_expression";
const NON_NULL_EXPRESSION: &str = "non_null_expression";

const COMMON_DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "function_declaration",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "generator_function_declaration",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(ARROW_FUNCTION, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(FUNCTION_EXPRESSION, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(GENERATOR_FUNCTION, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(METHOD_DEFINITION, SymbolKind::Method, NameRule::Hook),
    DefinitionRule::new(
        CLASS_DECLARATION,
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(CLASS_EXPRESSION, SymbolKind::Class, NameRule::Hook),
];

const TYPESCRIPT_DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(
        "function_signature",
        SymbolKind::Function,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(METHOD_SIGNATURE, SymbolKind::Method, NameRule::Hook),
    DefinitionRule::new(
        ABSTRACT_METHOD_SIGNATURE,
        SymbolKind::Method,
        NameRule::Hook,
    ),
    DefinitionRule::new(
        ABSTRACT_CLASS_DECLARATION,
        SymbolKind::Class,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        INTERFACE_DECLARATION,
        SymbolKind::Interface,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "type_alias_declaration",
        SymbolKind::TypeAlias,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "enum_declaration",
        SymbolKind::Enum,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "internal_module",
        SymbolKind::Module,
        NameRule::Field("name"),
    ),
];

const COMMON_HOOK_KINDS: &[&str] = &[
    VARIABLE_DECLARATOR,
    PAIR,
    ASSIGNMENT_EXPRESSION,
    EXPORT_STATEMENT,
    EXPORT_CLAUSE,
    EXPORT_SPECIFIER,
    EXPRESSION_STATEMENT,
    MEMBER_EXPRESSION,
    OBJECT,
    SHORTHAND_PROPERTY_IDENTIFIER,
    IDENTIFIER,
    PROPERTY_IDENTIFIER,
    PRIVATE_PROPERTY_IDENTIFIER,
    DECORATOR,
    CLASS_HERITAGE,
    PARENTHESIZED_EXPRESSION,
];

const JAVASCRIPT_HOOK_KINDS: &[&str] = &[FIELD_DEFINITION];

const TYPESCRIPT_HOOK_KINDS: &[&str] = &[
    PUBLIC_FIELD_DEFINITION,
    TYPE_IDENTIFIER,
    NESTED_TYPE_IDENTIFIER,
    ACCESSIBILITY_MODIFIER,
    EXTENDS_TYPE_CLAUSE,
    TYPE_ARGUMENTS,
    AS_EXPRESSION,
    SATISFIES_EXPRESSION,
    NON_NULL_EXPRESSION,
];

const FUNCTION_VALUE_KINDS: &[&str] = &[ARROW_FUNCTION, FUNCTION_EXPRESSION, GENERATOR_FUNCTION];

/// A wrapper that does not change the variable to which an expression binds.
const TRANSPARENT_WRAPPERS: &[&str] = &[
    PARENTHESIZED_EXPRESSION,
    AS_EXPRESSION,
    SATISFIES_EXPRESSION,
    NON_NULL_EXPRESSION,
];

fn has_field(parent: Node<'_>, field: &str, child: Node<'_>) -> bool {
    parent.child_by_field_name(field) == Some(child)
}

/// A key that `object.name` can address. A string key and a calculated key are not static names.
fn static_name(key: Node<'_>) -> Option<Node<'_>> {
    [PROPERTY_IDENTIFIER, PRIVATE_PROPERTY_IDENTIFIER]
        .contains(&key.kind())
        .then_some(key)
}

/// The syntax that gives a name to a function value or a class value.
struct Binding<'tree> {
    /// The declarator, the pair, the field, the assignment, or the export statement.
    holder: Node<'tree>,
    /// `None` for `export default`, which has no name node.
    name: Option<Node<'tree>>,
    /// The symbol range starts here.
    start: Node<'tree>,
}

fn binding(value: Node<'_>) -> Option<Binding<'_>> {
    let mut current = value;
    while let Some(parent) = current.parent() {
        if !TRANSPARENT_WRAPPERS.contains(&parent.kind()) {
            break;
        }
        current = parent;
    }
    let holder = current.parent()?;
    let name = match holder.kind() {
        VARIABLE_DECLARATOR if has_field(holder, "value", current) => holder
            .child_by_field_name("name")
            .filter(|name| name.kind() == IDENTIFIER)?,
        PAIR if has_field(holder, "value", current) => {
            static_name(holder.child_by_field_name("key")?)?
        }
        PUBLIC_FIELD_DEFINITION if has_field(holder, "value", current) => {
            static_name(holder.child_by_field_name("name")?)?
        }
        FIELD_DEFINITION if has_field(holder, "value", current) => {
            static_name(holder.child_by_field_name("property")?)?
        }
        ASSIGNMENT_EXPRESSION if has_field(holder, "right", current) => {
            let left = holder.child_by_field_name("left")?;
            let name = match left.kind() {
                IDENTIFIER => left,
                MEMBER_EXPRESSION => static_name(left.child_by_field_name("property")?)?,
                _ => return None,
            };
            return Some(Binding {
                holder,
                name: Some(name),
                start: left,
            });
        }
        EXPORT_STATEMENT if has_field(holder, "value", current) => {
            return Some(Binding {
                holder,
                name: None,
                start: value,
            })
        }
        _ => return None,
    };
    Some(Binding {
        holder,
        name: Some(name),
        start: name,
    })
}

fn is_private_member(member: Node<'_>, name: Node<'_>, context: &Context<'_, '_>) -> bool {
    if name.kind() == PRIVATE_PROPERTY_IDENTIFIER {
        return true;
    }
    let mut cursor = member.walk();
    let restricted = member
        .children(&mut cursor)
        .any(|child| child.kind() == ACCESSIBILITY_MODIFIER && context.text(child) != "public");
    restricted
}

/// The local names that the module exports with a statement that is not on the
/// definition: `export { a }`, `export default a`, `module.exports = { a }`,
/// `module.exports.a = a`, and `exports.a = a`.
fn names_exported_by_statement(program: Node<'_>, context: &Context<'_, '_>) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = program.walk();
    for statement in program.children(&mut cursor) {
        match statement.kind() {
            EXPORT_STATEMENT => {
                if statement.child_by_field_name("source").is_some() {
                    continue;
                }
                if let Some(value) = statement.child_by_field_name("value") {
                    if value.kind() == IDENTIFIER {
                        names.push(context.text(value));
                    }
                }
                let mut cursor = statement.walk();
                for clause in statement.children(&mut cursor) {
                    if clause.kind() != EXPORT_CLAUSE {
                        continue;
                    }
                    let mut cursor = clause.walk();
                    for specifier in clause.children(&mut cursor) {
                        if specifier.kind() != EXPORT_SPECIFIER {
                            continue;
                        }
                        if let Some(name) = specifier.child_by_field_name("name") {
                            names.push(context.text(name));
                        }
                    }
                }
            }
            EXPRESSION_STATEMENT => {
                let Some(assignment) = statement
                    .named_child(0)
                    .filter(|expression| expression.kind() == ASSIGNMENT_EXPRESSION)
                else {
                    continue;
                };
                let (Some(left), Some(right)) = (
                    assignment.child_by_field_name("left"),
                    assignment.child_by_field_name("right"),
                ) else {
                    continue;
                };
                if !is_commonjs_export(left, context) {
                    continue;
                }
                match right.kind() {
                    IDENTIFIER => names.push(context.text(right)),
                    OBJECT if context.text(left) == "module.exports" => {
                        let mut cursor = right.walk();
                        for member in right.named_children(&mut cursor) {
                            let name = match member.kind() {
                                SHORTHAND_PROPERTY_IDENTIFIER => Some(member),
                                PAIR => member
                                    .child_by_field_name("value")
                                    .filter(|value| value.kind() == IDENTIFIER),
                                _ => None,
                            };
                            names.extend(name.map(|name| context.text(name)));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    names
}

/// `module.exports`, `module.exports.name`, or `exports.name`.
fn is_commonjs_export(target: Node<'_>, context: &Context<'_, '_>) -> bool {
    if target.kind() != MEMBER_EXPRESSION {
        return false;
    }
    let text = context.text(target);
    text == "module.exports" || text.starts_with("module.exports.") || text.starts_with("exports.")
}

fn exported_if(condition: bool) -> Exported {
    if condition {
        Exported::Exported
    } else {
        Exported::NotExported
    }
}

/// The visibility of a declaration that is not a member: the `export` keyword
/// on the declaration, or an export statement that names it.
fn declaration_exported(declaration: Node<'_>, name: &str, context: &Context<'_, '_>) -> Exported {
    let Some(parent) = declaration.parent() else {
        return Exported::NotExported;
    };
    if parent.kind() == EXPORT_STATEMENT {
        return Exported::Exported;
    }
    if parent.parent().is_some() {
        // An export statement can name only a declaration at the top level of the module.
        return Exported::NotExported;
    }
    exported_if(names_exported_by_statement(parent, context).contains(&name.to_string()))
}

/// The visibility of the variable that holds an object literal.
fn object_exported(object: Node<'_>, context: &Context<'_, '_>) -> Exported {
    let Some(holder) = object.parent() else {
        return Exported::NotExported;
    };
    match holder.kind() {
        VARIABLE_DECLARATOR => match holder.child_by_field_name("name") {
            Some(name) if name.kind() == IDENTIFIER => holder
                .parent()
                .map(|statement| declaration_exported(statement, &context.text(name), context))
                .unwrap_or(Exported::NotExported),
            _ => Exported::NotExported,
        },
        ASSIGNMENT_EXPRESSION => exported_if(
            holder
                .child_by_field_name("left")
                .is_some_and(|left| is_commonjs_export(left, context)),
        ),
        _ => Exported::NotExported,
    }
}

impl Ecma {
    fn language(&self) -> Language {
        if self.typescript {
            Language::TypeScript
        } else {
            Language::JavaScript
        }
    }

    fn collect_supertypes(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<String>) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                TYPE_ARGUMENTS => {}
                IDENTIFIER | TYPE_IDENTIFIER | NESTED_TYPE_IDENTIFIER | MEMBER_EXPRESSION => {
                    out.push(context.text(child))
                }
                _ => self.collect_supertypes(child, context, out),
            }
        }
    }
}

impl LanguageSpec for Ecma {
    fn call_syntax(&self) -> CallSyntax {
        if self.typescript {
            TYPESCRIPT_CALLS
        } else {
            JAVASCRIPT_CALLS
        }
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().local_kinds();
        kinds.push(FORMAL_PARAMETERS);
        kinds
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![FORMAL_PARAMETERS]
    }

    /// A JavaScript parameter is a name directly in the parameter list.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        if node.kind() != FORMAL_PARAMETERS {
            self.call_syntax().locals(node, context, out);
            return;
        }
        let mut cursor = node.walk();
        for parameter in node.named_children(&mut cursor) {
            if parameter.kind() == IDENTIFIER {
                out.push(LocalName {
                    name: context.text(parameter),
                    type_name: None,
                    value_start: None,
                    is_parameter: true,
                });
            }
        }
    }

    fn import_kinds(&self) -> Vec<&'static str> {
        vec![IMPORT_STATEMENT, EXPORT_STATEMENT, CALL_EXPRESSION]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        let mut kinds = vec![
            IMPORT_CLAUSE,
            NAMESPACE_IMPORT,
            NAMED_IMPORTS,
            IMPORT_SPECIFIER,
            NAMESPACE_EXPORT,
            OBJECT_PATTERN,
            SHORTHAND_PATTERN,
            PAIR_PATTERN,
            STRING,
        ];
        if self.typescript {
            kinds.push(IMPORT_REQUIRE_CLAUSE);
        }
        kinds
    }

    /// The imported name `default` is the default export, and `*` is the module.
    /// `require('path')` with a literal path is an import. An `export` statement
    /// with a source is a re-export.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let text = |node: Node<'_>| context.text(node);
        let literal = |string: Node<'_>| {
            text(string)
                .trim_matches(|quote| quote == '\'' || quote == '"' || quote == '`')
                .to_string()
        };
        // The name and the alias of an import specifier or an export specifier.
        let specifier = |specifier: Node<'_>| -> Option<(String, String)> {
            let name = text(specifier.child_by_field_name("name")?);
            let alias = specifier.child_by_field_name("alias").map(text);
            Some((name.clone(), alias.unwrap_or(name)))
        };
        let module = |local: Node<'_>, path: &str, kind: ImportKind| {
            context.import(node, path, kind, Some("*".to_string()), Some(text(local)))
        };
        match node.kind() {
            IMPORT_STATEMENT => {
                let Some(source) = node.child_by_field_name("source") else {
                    // `import name = require('path')`
                    if let Some(clause) = child_of_kind(node, IMPORT_REQUIRE_CLAUSE) {
                        if let (Some(local), Some(source)) =
                            (clause.named_child(0), clause.child_by_field_name("source"))
                        {
                            out.push(module(local, &literal(source), ImportKind::Module));
                        }
                    }
                    return;
                };
                let path = literal(source);
                let Some(clause) = child_of_kind(node, IMPORT_CLAUSE) else {
                    out.push(context.import(node, path, ImportKind::Load, None, None));
                    return;
                };
                let mut cursor = clause.walk();
                for part in clause.named_children(&mut cursor) {
                    match part.kind() {
                        IDENTIFIER => out.push(context.import(
                            node,
                            path.as_str(),
                            ImportKind::Named,
                            Some("default".to_string()),
                            Some(text(part)),
                        )),
                        NAMESPACE_IMPORT => {
                            if let Some(local) = part.named_child(0) {
                                out.push(module(local, &path, ImportKind::Module));
                            }
                        }
                        NAMED_IMPORTS => {
                            let mut cursor = part.walk();
                            for (name, local) in
                                part.named_children(&mut cursor).filter_map(specifier)
                            {
                                out.push(context.import(
                                    node,
                                    path.as_str(),
                                    ImportKind::Named,
                                    Some(name),
                                    Some(local),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            EXPORT_STATEMENT => {
                let Some(source) = node.child_by_field_name("source") else {
                    return;
                };
                let path = literal(source);
                if let Some(clause) = child_of_kind(node, EXPORT_CLAUSE) {
                    let mut cursor = clause.walk();
                    for (name, local) in clause.named_children(&mut cursor).filter_map(specifier) {
                        out.push(context.import(
                            node,
                            path.as_str(),
                            ImportKind::ReExportNamed,
                            Some(name),
                            Some(local),
                        ));
                    }
                } else if let Some(local) =
                    child_of_kind(node, NAMESPACE_EXPORT).and_then(|export| export.named_child(0))
                {
                    out.push(module(local, &path, ImportKind::ReExportNamed));
                } else {
                    out.push(context.import(node, path, ImportKind::ReExportWildcard, None, None));
                }
            }
            CALL_EXPRESSION => {
                let is_require = node
                    .child_by_field_name("function")
                    .is_some_and(|function| {
                        function.kind() == IDENTIFIER && text(function) == "require"
                    });
                let argument = node
                    .child_by_field_name("arguments")
                    .filter(|arguments| arguments.named_child_count() == 1)
                    .and_then(|arguments| arguments.named_child(0))
                    .filter(|argument| argument.kind() == STRING);
                let (true, Some(argument)) = (is_require, argument) else {
                    return;
                };
                let path = literal(argument);
                let pattern = node
                    .parent()
                    .filter(|parent| {
                        parent.kind() == VARIABLE_DECLARATOR && has_field(*parent, "value", node)
                    })
                    .and_then(|declarator| declarator.child_by_field_name("name"));
                match pattern {
                    Some(local) if local.kind() == IDENTIFIER => {
                        out.push(module(local, &path, ImportKind::Module));
                    }
                    Some(pattern) if pattern.kind() == OBJECT_PATTERN => {
                        let mut cursor = pattern.walk();
                        for member in pattern.named_children(&mut cursor) {
                            let names = match member.kind() {
                                SHORTHAND_PATTERN => Some((text(member), text(member))),
                                PAIR_PATTERN => member
                                    .child_by_field_name("key")
                                    .zip(member.child_by_field_name("value"))
                                    .filter(|(_, value)| value.kind() == IDENTIFIER)
                                    .map(|(key, value)| (text(key), text(value))),
                                _ => None,
                            };
                            if let Some((name, local)) = names {
                                out.push(context.import(
                                    node,
                                    path.as_str(),
                                    ImportKind::Named,
                                    Some(name),
                                    Some(local),
                                ));
                            }
                        }
                    }
                    _ => out.push(context.import(node, path, ImportKind::Load, None, None)),
                }
            }
            _ => {}
        }
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        let mut definitions = COMMON_DEFINITIONS.to_vec();
        if self.typescript {
            definitions.extend_from_slice(TYPESCRIPT_DEFINITIONS);
        }
        definitions
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        let mut kinds = COMMON_HOOK_KINDS.to_vec();
        kinds.extend_from_slice(if self.typescript {
            TYPESCRIPT_HOOK_KINDS
        } else {
            JAVASCRIPT_HOOK_KINDS
        });
        kinds
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
            METHOD_DEFINITION | METHOD_SIGNATURE | ABSTRACT_METHOD_SIGNATURE => {
                static_name(node.child_by_field_name("name")?)
            }
            CLASS_EXPRESSION => binding(node)
                .and_then(|binding| binding.name)
                .or_else(|| node.child_by_field_name("name")),
            _ => {
                let binding = binding(node)?;
                // A pair in an object literal is a name here. `function_binding_name`
                // gives no name for it, because dead code detection cannot follow it.
                if binding.holder.kind() != PAIR {
                    function_binding_name(&node, context.source, self.language())?;
                }
                Some(binding.name.unwrap_or(node))
            }
        }
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        let node = draft.node;
        let kind = node.kind();
        draft.default_export = node.parent().is_some_and(|parent| {
            parent.kind() == EXPORT_STATEMENT && context.text(parent).starts_with("export default")
        });
        // A member of an object literal has the name of the variable in its key.
        let member = binding(node).map_or(node, |binding| binding.holder);
        let variable = member
            .parent()
            .filter(|object| object.kind() == OBJECT)
            .and_then(|object| object.parent())
            .filter(|holder| holder.kind() == VARIABLE_DECLARATOR)
            .and_then(|holder| holder.child_by_field_name("name"))
            .filter(|name| name.kind() == IDENTIFIER);
        if let Some(variable) = variable {
            draft.container_path.push(context.text(variable));
        }
        if FUNCTION_VALUE_KINDS.contains(&kind) || kind == CLASS_EXPRESSION {
            if let Some(binding) = binding(node) {
                draft.span = Span::between(&binding.start, &node);
                if binding.name.is_none() {
                    draft.name = "default".to_string();
                }
            }
        }
        let in_class = context
            .scopes
            .last()
            .is_some_and(|scope| scope.kind == Some(SymbolKind::Class));
        if kind == METHOD_DEFINITION && in_class && draft.name == "constructor" {
            draft.kind = SymbolKind::Constructor;
        }
        if draft.kind.is_type() {
            // The range of a class does not include its decorators.
            if node.child(0).is_some_and(|first| first.kind() == DECORATOR) {
                let mut cursor = node.walk();
                let first = node
                    .children(&mut cursor)
                    .find(|child| child.kind() != DECORATOR);
                if let Some(first) = first {
                    draft.span = Span::between(&first, &node);
                }
            }
            let mut cursor = node.walk();
            for clause in node.children(&mut cursor) {
                if [CLASS_HERITAGE, EXTENDS_TYPE_CLAUSE].contains(&clause.kind()) {
                    self.collect_supertypes(clause, context, &mut draft.supertypes);
                }
            }
        }
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        if context.in_function() {
            return Exported::NotExported;
        }
        let node = draft.node;
        let binding = (FUNCTION_VALUE_KINDS.contains(&node.kind())
            || node.kind() == CLASS_EXPRESSION)
            .then(|| binding(node))
            .flatten();
        // The member is the node that a class body or an object literal contains.
        let member = binding.as_ref().map_or(node, |binding| binding.holder);

        if member
            .parent()
            .is_some_and(|parent| parent.kind() == OBJECT)
        {
            return member
                .parent()
                .map(|object| object_exported(object, context))
                .unwrap_or(Exported::NotExported);
        }
        match context.scopes.last() {
            Some(scope) if scope.role == ScopeRole::Type => {
                if is_private_member(member, draft.name_node, context) {
                    Exported::NotExported
                } else {
                    scope.exported.clone()
                }
            }
            Some(scope) => {
                let keyword = member
                    .parent()
                    .is_some_and(|parent| parent.kind() == EXPORT_STATEMENT);
                exported_if(keyword && scope.exported == Exported::Exported)
            }
            None => match binding {
                None => declaration_exported(node, &draft.name, context),
                Some(binding) => match binding.holder.kind() {
                    EXPORT_STATEMENT => Exported::Exported,
                    VARIABLE_DECLARATOR => binding
                        .holder
                        .parent()
                        .map(|statement| declaration_exported(statement, &draft.name, context))
                        .unwrap_or(Exported::NotExported),
                    ASSIGNMENT_EXPRESSION => exported_if(
                        binding
                            .holder
                            .child_by_field_name("left")
                            .is_some_and(|left| is_commonjs_export(left, context)),
                    ),
                    _ => Exported::NotExported,
                },
            },
        }
    }
}

fn child_of_kind<'tree>(parent: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = parent.walk();
    let found = parent
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    found
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_function_declarations_and_export_keyword() {
        let source = "export function open() {}\nfunction closed() {}\nexport default function main() {}\nexport function* stream() {}\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Function open - 1-1 exported",
                "Function closed - 2-2 not_exported",
                "Function main - 3-3 exported",
                "Function stream - 4-4 exported",
            ]
        );
    }

    #[test]
    fn test_bound_function_values() {
        let source = "export const triple = (value) => value * 3;\nconst sub = function (n) {\n  return n - 1;\n};\nfunction run() {\n  const add = (n) => n + 1;\n  [1].map((n) => n);\n  return add(1);\n}\nconst wrapped = (() => 1);\nconst [first] = [() => 1];\nexport default () => 2;\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Function triple - 1-1 exported",
                "Function sub - 2-4 not_exported",
                "Function run - 5-9 not_exported",
                "Function add run 6-6 not_exported",
                "Function wrapped - 10-10 not_exported",
                "Function default - 12-12 exported",
            ]
        );
    }

    #[test]
    fn test_object_literal_members() {
        let source = "const registry = {\n  count: 0,\n  bump() {\n    return 1;\n  },\n  reset: () => 0,\n  'quoted': () => 1,\n  [key]: () => 2,\n};\nexport const api = {\n  get() {},\n  put: function () {},\n};\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Method bump - 3-5 not_exported",
                "Function reset - 6-6 not_exported",
                "Method get - 11-11 exported",
                "Function put - 12-12 exported",
            ]
        );
    }

    #[test]
    fn test_class_members() {
        let source = "export class Circle extends Base {\n  constructor(radius) {\n    super();\n  }\n  area() {\n    return 1;\n  }\n  #scaled(factor) {\n    return factor;\n  }\n  static unit() {\n    return new Circle(1);\n  }\n  grow = () => this.radius + 1;\n  #shrink = () => 0;\n}\nclass Hidden {\n  area() {}\n}\nconst Expression = class {\n  go() {}\n};\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Class Circle - 1-16 exported",
                "Constructor constructor Circle 2-4 exported",
                "Method area Circle 5-7 exported",
                "Method #scaled Circle 8-10 not_exported",
                "Method unit Circle 11-13 exported",
                "Function grow Circle 14-14 exported",
                "Function #shrink Circle 15-15 not_exported",
                "Class Hidden - 17-19 not_exported",
                "Method area Hidden 18-18 not_exported",
                "Class Expression - 20-22 not_exported",
                "Method go Expression 21-21 not_exported",
            ]
        );
        assert_eq!(
            symbols(Language::JavaScript, source)[0].supertypes,
            ["Base"]
        );
        assert_eq!(keys(Language::JavaScript, source)[3], "file#Circle.#scaled");
    }

    #[test]
    fn test_export_statements_that_name_a_declaration() {
        let source = "function a() {}\nfunction b() {}\nconst c = () => 1;\nfunction d() {}\nclass E {}\nexport { a, c as renamed };\nexport default E;\nexport { d } from './other.js';\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Function a - 1-1 exported",
                "Function b - 2-2 not_exported",
                "Function c - 3-3 exported",
                "Function d - 4-4 not_exported",
                "Class E - 5-5 exported",
            ]
        );
    }

    #[test]
    fn test_commonjs_exports() {
        let source = "function legacyName() {}\nfunction second() {}\nfunction third() {}\nfunction hidden() {}\nmodule.exports = { legacyName, alias: second };\nexports.third = third;\nmodule.exports.fourth = function () {};\nlocal.fifth = function () {};\n";
        assert_eq!(
            rows(Language::JavaScript, source),
            [
                "Function legacyName - 1-1 exported",
                "Function second - 2-2 exported",
                "Function third - 3-3 exported",
                "Function hidden - 4-4 not_exported",
                "Function fourth - 7-7 exported",
                "Function fifth - 8-8 not_exported",
            ]
        );
    }

    #[test]
    fn test_typescript_types_and_signatures() {
        let source = "export interface Shape extends Named<string> {\n  area(): number;\n}\nexport type Scale = number;\nexport enum Unit {\n  Metre,\n}\ninterface Props {\n  label: string;\n}\nexport function format(value: number): string;\nexport function format(value: string): string;\nexport function format(value: number | string): string {\n  return String(value);\n}\nexport namespace Tools {\n  export function open(): void {}\n  function closed(): void {}\n}\n";
        assert_eq!(
            rows(Language::TypeScript, source),
            [
                "Interface Shape - 1-3 exported",
                "Method area Shape 2-2 exported",
                "TypeAlias Scale - 4-4 exported",
                "Enum Unit - 5-7 exported",
                "Interface Props - 8-10 not_exported",
                "Function format - 11-11 exported",
                "Function format - 12-12 exported",
                "Function format - 13-15 exported",
                "Module Tools - 16-19 exported",
                "Function open Tools 17-17 exported",
                "Function closed Tools 18-18 not_exported",
            ]
        );
        assert_eq!(
            symbols(Language::TypeScript, source)[0].supertypes,
            ["Named"]
        );
        assert_eq!(
            keys(Language::TypeScript, source)[5..8],
            ["file#format", "file#format~2", "file#format~3"]
        );
    }

    #[test]
    fn test_typescript_class_members() {
        let source = "@Component\nexport abstract class Base implements Shape, Other {\n  constructor(name: string) {}\n  abstract area(): number;\n  private doubled(): number {\n    return 2;\n  }\n  protected half(): number {\n    return 1;\n  }\n  public whole(): number {\n    return 3;\n  }\n  grow = (): number => 1;\n  private shrink = (): number => 0;\n}\nconst typed = ((n: number) => n) as Handler;\n";
        assert_eq!(
            rows(Language::TypeScript, source),
            [
                "Class Base - 2-16 exported",
                "Constructor constructor Base 3-3 exported",
                "Method area Base 4-4 exported",
                "Method doubled Base 5-7 not_exported",
                "Method half Base 8-10 not_exported",
                "Method whole Base 11-13 exported",
                "Function grow Base 14-14 exported",
                "Function shrink Base 15-15 not_exported",
                "Function typed - 17-17 not_exported",
            ]
        );
        assert_eq!(
            symbols(Language::TypeScript, source)[0].supertypes,
            ["Shape", "Other"]
        );
    }

    #[test]
    fn test_jsx_components() {
        let source = "export function Badge({ label }) {\n  return <span>{label}</span>;\n}\nexport const Counter = () => {\n  const handleClick = () => 1;\n  return <button onClick={() => handleClick()} />;\n};\n";
        assert_eq!(
            rows(Language::Jsx, source),
            [
                "Function Badge - 1-3 exported",
                "Function Counter - 4-7 exported",
                "Function handleClick Counter 5-5 not_exported",
            ]
        );
        assert_eq!(rows(Language::Tsx, source), rows(Language::Jsx, source));
    }

    #[test]
    fn test_typescript_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "src/main.ts",
                "import { basename } from 'node:path';\nimport * as os from 'node:os';\nimport type { Shape } from './shapes';\nimport { twice as double, helper } from './util';\nimport fallback, { triple } from './util';\nimport './side';\nimport { onlyHere, triple as three } from './barrel';\nimport { deep } from './lib';\nimport old = require('./util');\nimport { gone } from './missing';\nimport { out } from '../../outside';\nimport { js } from './compiled.js';\n",
            ),
            ("src/barrel.ts", "export { triple } from './util';\nexport * from './other';\nexport * as tools from './util';\nexport { triple as local };\n"),
            ("src/other.ts", "export function onlyHere(): number {\n  return 3;\n}\nexport * from './barrel';\n"),
            ("src/util.ts", "export function twice() {}\nexport function helper() {}\nexport const triple = () => 3;\nexport default function fallback() {}\n"),
            ("src/shapes.ts", "export interface Shape {}\n"),
            ("src/side.ts", ""),
            ("src/lib/index.ts", "export function deep() {}\n"),
            ("src/compiled.ts", "export function js() {}\n"),
        ];
        assert_eq!(
            rows(&files, "src/main.ts"),
            [
                "1 Named node:path basename->basename external",
                "2 Module node:os *->os external",
                "3 Named ./shapes Shape->Shape src/shapes.ts [src/shapes.ts#Shape]",
                "4 Named ./util twice->double src/util.ts [src/util.ts#twice]",
                "4 Named ./util helper->helper src/util.ts [src/util.ts#helper]",
                "5 Named ./util default->fallback src/util.ts",
                "5 Named ./util triple->triple src/util.ts [src/util.ts#triple]",
                "6 Load ./side -->- src/side.ts",
                "7 Named ./barrel onlyHere->onlyHere src/barrel.ts [src/other.ts#onlyHere]",
                "7 Named ./barrel triple->three src/barrel.ts [src/util.ts#triple]",
                "8 Named ./lib deep->deep src/lib/index.ts [src/lib/index.ts#deep]",
                "9 Module ./util *->old src/util.ts",
                "10 Named ./missing gone->gone unresolved",
                "11 Named ../../outside out->out unresolved",
                "12 Named ./compiled.js js->js src/compiled.ts [src/compiled.ts#js]",
            ]
        );
        assert_eq!(
            rows(&files, "src/barrel.ts"),
            [
                "1 ReExportNamed ./util triple->triple src/util.ts [src/util.ts#triple]",
                "2 ReExportWildcard ./other -->- src/other.ts",
                "3 ReExportNamed ./util *->tools src/util.ts",
            ]
        );
    }

    #[test]
    fn test_javascript_require_and_extensions() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "legacy.cjs",
                "const { basename, join: glue } = require('node:path');\nconst other = require('./other.js');\nrequire('./side');\nconst name = require(dynamic);\nconst part = require('./other.js').part;\n",
            ),
            ("main.js", "import legacy from './legacy.cjs';\nimport { a } from './both';\nimport App from './components';\n"),
            ("other.js", "export function part() {}\n"),
            ("side.js", ""),
            ("both.js", ""),
            ("both.jsx", ""),
            ("components.jsx", "export default function App() {}\n"),
        ];
        assert_eq!(
            rows(&files, "legacy.cjs"),
            [
                "1 Named node:path basename->basename external",
                "1 Named node:path join->glue external",
                "2 Module ./other.js *->other other.js",
                "3 Load ./side -->- side.js",
                "5 Load ./other.js -->- other.js",
            ]
        );
        assert_eq!(
            rows(&files, "main.js"),
            [
                "1 Named ./legacy.cjs default->legacy legacy.cjs",
                "2 Named ./both a->a ambiguous(both.js,both.jsx)",
                "3 Named ./components default->App components.jsx",
            ]
        );
    }

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "main.ts",
            "import { basename } from 'node:path';\nimport { Circle, Square, describe } from './shapes';\nimport type { Shape } from './shapes';\nimport { twice as double } from './util';\nimport fallback from './util';\nimport * as other from './other';\nimport { onlyHere } from './barrel';\n\nconst registry = {\n  bump(): number {\n    return 1;\n  },\n  reset: (): number => 0,\n};\n\nfunction local(): number {\n  return 4;\n}\n\nfunction run(): number {\n  function inner(): number {\n    return 5;\n  }\n  const add = (n: number): number => n + 1;\n  const total = local() + double(2) + inner() + onlyHere() + fallback();\n  const circle = new Circle(1);\n  circle.area();\n  const numbers = [1, 2].map((n) => local());\n  return total + add(numbers.length);\n}\n\nfunction measure(size: Square, shape: Shape, callback: () => number): number {\n  return size.area() + shape.area() + callback();\n}\n\nfunction external(): number {\n  const seen = new Map<string, number>();\n  seen.set('a', 1);\n  const made = new Square();\n  return parseInt('4', 10) + basename('a').length + other.helper() + Circle.unit().radius;\n}\n\nfunction main(): void {\n  describe();\n  registry.bump() + registry.reset() + registry.gone();\n}\n",
        ),
        ("barrel.ts", "export * from './other';\n"),
        ("other.ts", "export function helper(): number {\n  return 2;\n}\n\nexport function onlyHere(): number {\n  return 3;\n}\n\nexport function set(): void {}\n"),
        (
            "shapes.ts",
            "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base {\n  constructor(name: string) {}\n\n  describe(): string {\n    return '';\n  }\n}\n\nexport class Circle extends Base implements Shape {\n  radius = 1;\n\n  constructor(radius: number) {\n    super('circle');\n  }\n\n  area(): number {\n    return this.#scaled(3);\n  }\n\n  #scaled(factor: number): number {\n    return factor;\n  }\n\n  override describe(): string {\n    return super.describe() + '!';\n  }\n\n  static unit(): Circle {\n    return new Circle(1);\n  }\n}\n\nexport class Square implements Shape {\n  area(): number {\n    return this.doubled();\n  }\n\n  private doubled(): number {\n    return 2;\n  }\n}\n\nexport function describe(): string {\n  return 'shapes';\n}\n",
        ),
        ("util.ts", "export function twice(value: number): number {\n  return value * 2;\n}\n\nexport default function fallback(): number {\n  return 7;\n}\n"),
    ];

    #[test]
    fn test_typescript_calls() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.ts"),
            [
                "25 run>local UniqueCandidate SameFile [main.ts#local]",
                "25 run>double UniqueCandidate ImportedAlias [util.ts#twice]",
                "25 run>inner UniqueCandidate LexicalScope [main.ts#run.inner]",
                "25 run>onlyHere UniqueCandidate ImportedName [other.ts#onlyHere]",
                "25 run>fallback UniqueCandidate ImportedAlias [util.ts#fallback]",
                "26 run>Circle UniqueCandidate QualifiedPath [shapes.ts#Circle.constructor]",
                "27 run>circle.area UniqueCandidate ReceiverDeclaredType [shapes.ts#Circle.area]",
                "28 run>(..).map Unresolved - []",
                "28 run>local UniqueCandidate SameFile [main.ts#local]",
                "29 run>add UniqueCandidate LexicalScope [main.ts#run.add]",
                "33 measure>size.area UniqueCandidate ReceiverDeclaredType [shapes.ts#Square.area]",
                "33 measure>shape.area Ambiguous NameOnly [shapes.ts#Circle.area, shapes.ts#Shape.area, shapes.ts#Square.area]",
                "33 measure>callback Unresolved - []",
                "37 external>Map Unresolved - []",
                "38 external>seen.set Unresolved - []",
                "39 external>Square UniqueCandidate QualifiedPath [shapes.ts#Square]",
                "40 external>parseInt Unresolved - []",
                "40 external>basename External - []",
                "40 external>other.helper UniqueCandidate ImportedModule [other.ts#helper]",
                "40 external>Circle.unit UniqueCandidate QualifiedPath [shapes.ts#Circle.unit]",
                "44 main>describe UniqueCandidate ImportedName [shapes.ts#describe]",
                "45 main>registry.bump UniqueCandidate LexicalScope [main.ts#registry.bump]",
                "45 main>registry.reset UniqueCandidate LexicalScope [main.ts#registry.reset]",
                "45 main>registry.gone Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_in_classes() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "shapes.ts"),
            [
                "17 Circle.constructor>super.super UniqueCandidate EnclosingType [shapes.ts#Base.constructor]",
                "21 Circle.area>self.#scaled UniqueCandidate EnclosingType [shapes.ts#Circle.#scaled]",
                "29 Circle.describe>super.describe UniqueCandidate EnclosingType [shapes.ts#Base.describe]",
                "33 Circle.unit>Circle UniqueCandidate QualifiedPath [shapes.ts#Circle.constructor]",
                "39 Square.area>self.doubled UniqueCandidate EnclosingType [shapes.ts#Square.doubled]",
            ]
        );
    }

    #[test]
    fn test_javascript_calls() {
        use crate::index::resolve::testing::rows;
        let files = [
            (
                "main.js",
                "import legacy from './legacy.cjs';\n\nfunction dynamic(shape) {\n  return shape.area() + legacy.legacyName() + shape();\n}\n",
            ),
            (
                "legacy.cjs",
                "const { basename } = require('node:path');\nconst other = require('./other.js');\n\nfunction legacyName() {\n  return basename('a') + other.onlyHere();\n}\n\nmodule.exports = { legacyName };\n",
            ),
            ("other.js", "export function onlyHere() {\n  return 3;\n}\n\nexport class Circle {\n  area() {\n    return 1;\n  }\n}\n"),
        ];
        assert_eq!(
            rows(&files, "main.js"),
            [
                "4 dynamic>shape.area Ambiguous NameOnly [other.js#Circle.area]",
                "4 dynamic>legacy.legacyName UniqueCandidate ImportedAlias [legacy.cjs#legacyName]",
                "4 dynamic>shape Unresolved - []",
            ]
        );
        assert_eq!(
            rows(&files, "legacy.cjs"),
            [
                "1 ->require Unresolved - []",
                "2 ->require Unresolved - []",
                "5 legacyName>basename External - []",
                "5 legacyName>other.onlyHere UniqueCandidate ImportedModule [other.js#onlyHere]",
            ]
        );
    }
}
