//! C++ symbols.

use tree_sitter::Node;

use super::c;
use crate::index::extract::{
    Context, DefinitionRule, Draft, LanguageSpec, Linked, NameRule, Scope, ScopeRole,
};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::ImportBinding;

use crate::index::calls::{Argument, Call, CallSyntax, Callee, LocalName};
use crate::index::model::ReferenceKind;

const NAMESPACE_ALIAS_DEFINITION: &str = "namespace_alias_definition";
const USING_DECLARATION: &str = "using_declaration";
const OPTIONAL_PARAMETER_DECLARATION: &str = "optional_parameter_declaration";
const ARGUMENT_LIST: &str = "argument_list";
const INITIALIZER_LIST: &str = "initializer_list";
const FOR_RANGE_LOOP: &str = "for_range_loop";
const COMPOUND_STATEMENT: &str = "compound_statement";
const DECLARATION_LIST: &str = "declaration_list";
const TRANSLATION_UNIT: &str = "translation_unit";
/// The kinds of the blocks that end the scope of a namespace alias.
const ALIAS_BLOCKS: &[&str] = &[COMPOUND_STATEMENT, DECLARATION_LIST, TRANSLATION_UNIT];

/// `Type{...}` is a construction. A field access with `.` or `->` gives a
/// member, and `scope::name` gives a path.
const CALL_SYNTAX: CallSyntax = CallSyntax {
    constructions: &[
        ("new_expression", "type", "arguments"),
        ("compound_literal_expression", "type", "value"),
    ],
    paths: &[(QUALIFIED_IDENTIFIER, "scope", "name")],
    self_words: &["this"],
    ..c::CALL_SYNTAX
};

pub struct Cpp;

const CLASS_SPECIFIER: &str = "class_specifier";
const NAMESPACE_DEFINITION: &str = "namespace_definition";
const ATTRIBUTE_DECLARATION: &str = "attribute_declaration";
const LAMBDA_EXPRESSION: &str = "lambda_expression";
const INIT_DECLARATOR: &str = "init_declarator";
const QUALIFIED_IDENTIFIER: &str = "qualified_identifier";
const FIELD_IDENTIFIER: &str = "field_identifier";
const DESTRUCTOR_NAME: &str = "destructor_name";
const OPERATOR_NAME: &str = "operator_name";
const TEMPLATE_TYPE: &str = "template_type";
const TEMPLATE_ARGUMENT_LIST: &str = "template_argument_list";
const ACCESS_SPECIFIER: &str = "access_specifier";
const FRIEND_DECLARATION: &str = "friend_declaration";
const BASE_CLASS_CLAUSE: &str = "base_class_clause";
const FIELD_DECLARATION_LIST: &str = "field_declaration_list";
const LINKAGE_SPECIFICATION: &str = "linkage_specification";
const PARAMETER_KINDS: &[&str] = &[
    "parameter_declaration",
    "optional_parameter_declaration",
    "variadic_parameter_declaration",
];

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::new(CLASS_SPECIFIER, SymbolKind::Class, NameRule::Field("name")),
    DefinitionRule::new(
        NAMESPACE_DEFINITION,
        SymbolKind::Module,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(
        "alias_declaration",
        SymbolKind::TypeAlias,
        NameRule::Field("name"),
    ),
    DefinitionRule::new(LAMBDA_EXPRESSION, SymbolKind::Function, NameRule::Hook),
    // The name of each declarator of a data member is one field. A field
    // access and the name of a member function have this node kind too, and
    // `name_node` rejects them.
    DefinitionRule::new(FIELD_IDENTIFIER, SymbolKind::Field, NameRule::Hook),
];

const FUNCTION_NAME_KINDS: &[&str] = &[
    c::IDENTIFIER,
    FIELD_IDENTIFIER,
    DESTRUCTOR_NAME,
    OPERATOR_NAME,
    QUALIFIED_IDENTIFIER,
];

fn is_function(node: Node<'_>) -> bool {
    [c::FUNCTION_DEFINITION, c::FUNCTION_DECLARATOR].contains(&node.kind())
}

/// Tells if `extern "C"` contains the function, as a block or before it.
fn has_c_linkage(node: Node<'_>, context: &Context<'_, '_>) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == LINKAGE_SPECIFICATION
            && parent
                .child_by_field_name("value")
                .is_some_and(|value| context.text(value) == "\"C\"")
        {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// The scope names of a qualified name, and the node of its last name.
/// `a::B::run` gives `["a", "B"]` and the node of `run`.
fn split_qualified_name<'tree>(
    mut name: Node<'tree>,
    context: &Context<'_, 'tree>,
) -> (Vec<String>, Node<'tree>) {
    let mut scopes = Vec::new();
    while name.kind() == QUALIFIED_IDENTIFIER {
        if let Some(scope) = name.child_by_field_name("scope") {
            let scope = match scope.kind() {
                TEMPLATE_TYPE => scope.child_by_field_name("name").unwrap_or(scope),
                _ => scope,
            };
            scopes.push(context.text(scope));
        }
        match name.child_by_field_name("name") {
            Some(inner) => name = inner,
            None => break,
        }
    }
    (scopes, name)
}

/// The child of a class body that contains `node`.
fn class_member<'tree>(node: Node<'tree>, class: Node<'tree>) -> Option<Node<'tree>> {
    let body = class.child_by_field_name("body")?;
    let mut member = node;
    while member.parent()? != body {
        member = member.parent()?;
    }
    Some(member)
}

/// The members of a `class` are private until an access specifier. The
/// members of a `struct` or a `union` are public.
fn is_public_member(member: Node<'_>, class: Node<'_>, context: &Context<'_, '_>) -> bool {
    let mut public = class.kind() != CLASS_SPECIFIER;
    let Some(body) = class.child_by_field_name("body") else {
        return public;
    };
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child == member {
            break;
        }
        if child.kind() == ACCESS_SPECIFIER {
            public = context.text(child).starts_with("public");
        }
    }
    public
}

fn exported_if(condition: bool) -> Exported {
    if condition {
        Exported::Exported
    } else {
        Exported::NotExported
    }
}

fn collect_base_names(node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            ACCESS_SPECIFIER | TEMPLATE_ARGUMENT_LIST => {}
            c::TYPE_IDENTIFIER => out.push(context.text(child)),
            QUALIFIED_IDENTIFIER => {
                let (mut names, last) = split_qualified_name(child, context);
                let last = match last.kind() {
                    TEMPLATE_TYPE => last.child_by_field_name("name").unwrap_or(last),
                    _ => last,
                };
                names.push(context.text(last));
                out.push(names.join("::"));
            }
            _ => collect_base_names(child, context, out),
        }
    }
}

/// The kind of the scope that the name of a definition gives.
enum Owner {
    Missing,
    /// The index does not give one answer.
    NotOne,
    Type,
    Namespace,
}

/// Finds the scope that has this full path. Many files can open one
/// namespace, but a type has one definition.
fn owner(symbols: &[Linked<'_>], scope_path: &[String]) -> Owner {
    let Some((name, container_path)) = scope_path.split_last() else {
        return Owner::Missing;
    };
    let mut types = 0;
    let mut namespaces = 0;
    for candidate in symbols {
        if candidate.symbol.name != *name || candidate.facts.container_path != container_path {
            continue;
        }
        match candidate.symbol.kind {
            SymbolKind::Class | SymbolKind::Struct => types += 1,
            SymbolKind::Module => namespaces += 1,
            _ => {}
        }
    }
    match (types, namespaces) {
        (0, 0) => Owner::Missing,
        (1, 0) => Owner::Type,
        (0, _) => Owner::Namespace,
        _ => Owner::NotOne,
    }
}

/// Finds the kind and the visibility on which the declarations of the
/// definition at `position` agree. The error is the start of a reason.
fn declaration(
    symbols: &[Linked<'_>],
    position: usize,
    in_namespace: bool,
) -> Result<(SymbolKind, Exported), &'static str> {
    let definition = &symbols[position];
    let mut found: Option<(SymbolKind, &Exported)> = None;
    for (other, candidate) in symbols.iter().enumerate() {
        let callable = matches!(
            candidate.symbol.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        );
        if other == position
            || !callable
            || candidate.symbol.receiver_type.is_some()
            || candidate.symbol.name != definition.symbol.name
            || candidate.facts.container_path != definition.facts.container_path
            || candidate.facts.parameter_count != definition.facts.parameter_count
        {
            continue;
        }
        let fact = (candidate.symbol.kind, &candidate.symbol.exported);
        match found {
            None => found = Some(fact),
            Some(first) if first == fact => {}
            Some(_) => return Err("the declarations do not agree for"),
        }
    }
    let Some((kind, exported)) = found else {
        return Err("the index has no declaration of");
    };
    if (kind == SymbolKind::Function) != in_namespace {
        return Err("the declaration has an unexpected kind for");
    }
    Ok((kind, exported.clone()))
}

/// The number of parameters that a parameter list declares. `(void)` declares none.
fn parameter_count(parameters: Node<'_>, context: &Context<'_, '_>) -> usize {
    let mut cursor = parameters.walk();
    let declared: Vec<Node<'_>> = parameters
        .named_children(&mut cursor)
        .filter(|child| PARAMETER_KINDS.contains(&child.kind()))
        .collect();
    match declared.as_slice() {
        [only] if context.text(*only) == "void" => 0,
        _ => declared.len(),
    }
}

/// The declarators that are between a field declaration and the name of a
/// data member: a pointer, a reference, or an array.
const MEMBER_WRAPPERS: &[&str] = &[
    c::POINTER_DECLARATOR,
    c::REFERENCE_DECLARATOR,
    c::ARRAY_DECLARATOR,
];

/// The field declaration of a data member, and the array mark of its
/// declarator. `None` means that the name is not the name of a data member.
fn data_member<'tree>(
    name: Node<'tree>,
    context: &Context<'_, 'tree>,
) -> Option<(Node<'tree>, bool)> {
    // The walk gives the first parent without a search. A field access and
    // the name of a member function stop here.
    let mut parent = context.parent(name)?;
    let mut array = false;
    while MEMBER_WRAPPERS.contains(&parent.kind()) {
        array |= parent.kind() == c::ARRAY_DECLARATOR;
        parent = parent.parent()?;
    }
    (parent.kind() == c::FIELD_DECLARATION).then_some((parent, array))
}

/// The text between the `<` at `open` and the `>` that closes it.
fn template_arguments(text: &str, open: usize) -> Option<(&str, usize)> {
    let mut depth = 0usize;
    for (position, character) in text[open..].char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&text[open + 1..open + position], open + position + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// The declared type in the form that the resolver reads. A smart pointer
/// `std::unique_ptr<T>` or `std::shared_ptr<T>` is `T`, as a pointer is.
/// `std::array<T, N>` is `T[]`, and so is an array declarator.
fn declared_type(text: &str, array: bool) -> String {
    const SMART_POINTERS: &[&str] = &[
        "std::unique_ptr<",
        "std::shared_ptr<",
        "unique_ptr<",
        "shared_ptr<",
    ];
    let mut text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let found = SMART_POINTERS
            .iter()
            .filter_map(|prefix| text.find(prefix).map(|at| (at, prefix.len())))
            .min();
        let Some((at, length)) = found else {
            break;
        };
        let Some((inside, end)) = template_arguments(&text, at + length - 1) else {
            break;
        };
        // A deleter is the second argument of `std::unique_ptr`.
        let pointee = crate::index::calls::split_list(inside)
            .first()
            .map_or("", |pointee| *pointee)
            .to_string();
        // The mark `*` tells the resolver that the member is a pointer.
        text.replace_range(at..end, &format!("{pointee}*"));
    }
    for prefix in ["std::array<", "array<"] {
        if !text.starts_with(prefix) {
            continue;
        }
        if let Some((inside, end)) = template_arguments(&text, prefix.len() - 1) {
            let element = crate::index::calls::split_list(inside)
                .first()
                .map_or("", |element| *element)
                .to_string();
            if end == text.len() {
                text = format!("{element}[]");
            }
        }
    }
    if array {
        text.push_str("[]");
    }
    text
}

const FIELD_EXPRESSION: &str = "field_expression";

/// The members of `std::unique_ptr` and `std::shared_ptr`. `owner.reset()`
/// calls the pointer, and `owner->reset()` calls the object.
const SMART_POINTER_MEMBERS: &[&str] = &[
    "get",
    "reset",
    "release",
    "swap",
    "use_count",
    "unique",
    "owner_before",
    "get_deleter",
];

/// Adds the declared type of each declaration of `name` below `node`.
fn declared_types_of(node: Node<'_>, name: &str, context: &Context<'_, '_>, out: &mut Vec<String>) {
    let declares = [
        c::DECLARATION,
        c::PARAMETER_DECLARATION,
        OPTIONAL_PARAMETER_DECLARATION,
    ]
    .contains(&node.kind());
    if declares {
        let mut names = Vec::new();
        c::declared_names(node, context, &mut names);
        out.extend(
            names
                .into_iter()
                .filter(|local| local.name == name)
                .map(|local| local.type_name.unwrap_or_default()),
        );
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        declared_types_of(child, name, context, out);
    }
}

/// Tells if the function that contains `call` declares `name` one or more
/// times, each time with a type that is not a smart pointer and not `auto`.
fn is_local_value(call: Node<'_>, name: &str, context: &Context<'_, '_>) -> bool {
    let mut function = call.parent();
    while let Some(node) = function.filter(|node| node.kind() != c::FUNCTION_DEFINITION) {
        function = node.parent();
    }
    let Some(function) = function else {
        return false;
    };
    let mut types = Vec::new();
    declared_types_of(function, name, context, &mut types);
    !types.is_empty()
        && types.iter().all(|declared| {
            !declared.is_empty() && !declared.contains("ptr") && !declared.contains("auto")
        })
}

/// What a call `receiver.member()` is, when `member` is also a member of a
/// smart pointer.
enum PointerMember {
    /// The call is a different call, or the receiver is an object for sure.
    No,
    /// The receiver is one name that can be a smart pointer.
    OneName,
    /// The receiver is a chain. The resolver gives a chain a type.
    Chain,
}

fn pointer_member(call: Node<'_>, context: &Context<'_, '_>) -> PointerMember {
    let function = call
        .child_by_field_name("function")
        .filter(|function| function.kind() == FIELD_EXPRESSION);
    let Some(function) = function else {
        return PointerMember::No;
    };
    let dot = function
        .child_by_field_name("operator")
        .is_some_and(|operator| context.text(operator) == ".");
    let member = function
        .child_by_field_name("field")
        .map(|field| context.text(field));
    let is_member = member.is_some_and(|member| SMART_POINTER_MEMBERS.contains(&member.as_str()));
    let Some(receiver) = function
        .child_by_field_name("argument")
        .filter(|_| dot && is_member)
    else {
        return PointerMember::No;
    };
    if receiver.kind() != c::IDENTIFIER {
        return PointerMember::Chain;
    }
    if is_local_value(call, &context.text(receiver), context) {
        PointerMember::No
    } else {
        PointerMember::OneName
    }
}

impl LanguageSpec for Cpp {
    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn call_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().call_kinds();
        kinds.push(c::DECLARATION);
        kinds
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        vec![
            c::DECLARATION,
            c::PARAMETER_DECLARATION,
            OPTIONAL_PARAMETER_DECLARATION,
            NAMESPACE_ALIAS_DEFINITION,
            USING_DECLARATION,
            FOR_RANGE_LOOP,
        ]
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![
            COMPOUND_STATEMENT,
            DECLARATION_LIST,
            TRANSLATION_UNIT,
            NAMESPACE_ALIAS_DEFINITION,
            USING_DECLARATION,
            OPTIONAL_PARAMETER_DECLARATION,
            ARGUMENT_LIST,
            INITIALIZER_LIST,
            FOR_RANGE_LOOP,
            FIELD_EXPRESSION,
        ]
    }

    /// `Type name(arguments);` and `Type name{arguments};` in a function body
    /// make an object, thus they call a constructor of the type.
    fn call<'tree>(&self, node: Node<'tree>, context: &Context<'_, 'tree>) -> Option<Call<'tree>> {
        if node.kind() != c::DECLARATION {
            // The syntax does not tell if the receiver of `owner.reset()` is a
            // smart pointer. A declared pointee type must not give an edge to
            // a member of the pointee. Thus one name becomes an expression
            // without a type, and a chain gives no call.
            let mut call = self.call_syntax().call(node, context)?;
            match pointer_member(node, context) {
                PointerMember::No => {}
                PointerMember::OneName => {
                    call.callee.receiver = Some(crate::index::model::Receiver::Expression);
                    call.callee.qualifier = None;
                }
                PointerMember::Chain => return None,
            }
            return Some(call);
        }
        if !context.in_function() {
            return None;
        }
        let type_node = node.child_by_field_name("type").filter(|type_node| {
            [c::TYPE_IDENTIFIER, QUALIFIED_IDENTIFIER].contains(&type_node.kind())
        })?;
        let declarator = node.child_by_field_name("declarator")?;
        let arguments = match declarator.kind() {
            c::FUNCTION_DECLARATOR => declarator.child_by_field_name("parameters")?,
            INIT_DECLARATOR => declarator
                .child_by_field_name("value")
                .filter(|value| [ARGUMENT_LIST, INITIALIZER_LIST].contains(&value.kind()))?,
            _ => return None,
        };
        Some(Call {
            callee: Callee {
                name_node: type_node,
                receiver: None,
                qualifier: None,
            },
            kind: ReferenceKind::Construct,
            arguments: vec![Argument::Other; arguments.named_child_count()],
        })
    }

    /// A namespace alias is a local name with the namespace as its type.
    /// `using namespace name;` is the local name `*` with that namespace as its type.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        let namespace = |name: String, target: Node<'_>| LocalName {
            name,
            type_name: Some(context.text(target)),
            value_start: None,
            is_parameter: false,
            element: false,
            result_index: None,
            visible: None,
        };
        match node.kind() {
            NAMESPACE_ALIAS_DEFINITION => {
                let alias = node.child_by_field_name("name");
                let target = node.named_child(node.named_child_count().saturating_sub(1) as u32);
                if let (Some(alias), Some(target)) = (alias, target) {
                    let mut block = node.parent();
                    while let Some(found) =
                        block.filter(|found| !ALIAS_BLOCKS.contains(&found.kind()))
                    {
                        block = found.parent();
                    }
                    let end = block.map_or(node.end_byte(), |block| block.end_byte());
                    out.push(LocalName {
                        visible: Some(node.start_byte()..end),
                        ..namespace(context.text(alias), target)
                    });
                }
            }
            USING_DECLARATION => {
                let directive = context.text(node).starts_with("using namespace");
                if let (true, Some(target)) = (directive, node.named_child(0)) {
                    out.push(namespace("*".to_string(), target));
                }
            }
            FOR_RANGE_LOOP => {
                // `for (Type name : values)` binds an element of the values.
                // With `auto`, the declared type of the values gives the type.
                let field = |field| node.child_by_field_name(field);
                let mut name = field("declarator");
                while let Some(wrapper) = name.filter(|name| MEMBER_WRAPPERS.contains(&name.kind()))
                {
                    name = wrapper.named_child(0);
                }
                let name = name.filter(|name| name.kind() == c::IDENTIFIER);
                if let (Some(name), Some(values)) = (name, field("right")) {
                    out.push(LocalName {
                        name: context.text(name),
                        type_name: field("type")
                            .map(|declared| declared_type(&context.text(declared), false)),
                        value_start: Some(values.start_byte()),
                        is_parameter: false,
                        element: true,
                        result_index: None,
                        visible: None,
                    });
                }
            }
            _ => {
                let first = out.len();
                c::declared_names(node, context, out);
                for local in &mut out[first..] {
                    local.type_name = local
                        .type_name
                        .take()
                        .map(|declared| declared_type(&declared, false));
                }
            }
        }
    }

    fn import_kinds(&self) -> Vec<&'static str> {
        vec![c::PREPROC_INCLUDE]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![c::SYSTEM_LIB_STRING]
    }

    /// `using namespace` and a namespace alias are not imports of a file.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        c::include_binding(node, context, out);
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        let mut definitions = c::DEFINITIONS.to_vec();
        definitions.extend_from_slice(DEFINITIONS);
        definitions
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        let mut kinds = c::HOOK_KINDS.to_vec();
        kinds.extend_from_slice(&[
            c::REFERENCE_DECLARATOR,
            INIT_DECLARATOR,
            QUALIFIED_IDENTIFIER,
            FIELD_IDENTIFIER,
            DESTRUCTOR_NAME,
            OPERATOR_NAME,
            TEMPLATE_TYPE,
            TEMPLATE_ARGUMENT_LIST,
            ACCESS_SPECIFIER,
            FRIEND_DECLARATION,
            BASE_CLASS_CLAUSE,
            FIELD_DECLARATION_LIST,
        ]);
        kinds.extend_from_slice(PARAMETER_KINDS);
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
        if node.kind() == FIELD_IDENTIFIER {
            return data_member(node, context).map(|_| node);
        }
        if node.kind() == LAMBDA_EXPRESSION {
            // A lambda has a name only when a declaration binds it to one variable.
            let declarator = node
                .parent()
                .filter(|parent| parent.kind() == INIT_DECLARATOR)?;
            if declarator.child_by_field_name("value") != Some(node) {
                return None;
            }
            return declarator
                .child_by_field_name("declarator")
                .filter(|name| name.kind() == c::IDENTIFIER);
        }
        if is_function(node) {
            // A friend declaration names a function of a different scope.
            let friend = c::function_holder(node)
                .parent()
                .is_some_and(|parent| parent.kind() == FRIEND_DECLARATION);
            if friend {
                return None;
            }
        }
        c::name_node(node, FUNCTION_NAME_KINDS, context)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        if draft.kind == SymbolKind::Field {
            let Some((declaration, array)) = data_member(draft.node, context) else {
                return false;
            };
            draft.return_type = declaration
                .child_by_field_name("type")
                .map(|declared| declared_type(&context.text(declared), array));
            draft.span = Span::of(&declaration);
            return true;
        }
        if !c::refine(draft) {
            return false;
        }
        let node = draft.node;
        // The range of a declaration does not include its attributes.
        if node
            .child(0)
            .is_some_and(|first| first.kind() == ATTRIBUTE_DECLARATION)
        {
            let mut cursor = node.walk();
            let start = node
                .children(&mut cursor)
                .find(|child| child.kind() != ATTRIBUTE_DECLARATION);
            if let Some(start) = start {
                draft.span = Span::between(&start, &node);
            }
        }
        match node.kind() {
            // `class Name;` refers to a class. It does not define the class.
            CLASS_SPECIFIER if draft.body.is_none() => return false,
            LAMBDA_EXPRESSION => draft.span = Span::between(&draft.name_node, &node),
            // `namespace a::b` defines `b` in the scope `a`.
            NAMESPACE_DEFINITION => {
                let mut names: Vec<String> = draft
                    .name
                    .split("::")
                    .map(|name| name.trim().to_string())
                    .collect();
                if let Some(last) = names.pop() {
                    draft.name = last;
                    draft.container_path.extend(names);
                }
            }
            _ => {}
        }
        if draft.kind.is_type() {
            let mut cursor = node.walk();
            for clause in node.children(&mut cursor) {
                if clause.kind() == BASE_CLASS_CLAUSE {
                    collect_base_names(clause, context, &mut draft.supertypes);
                }
            }
        }
        if !is_function(node) {
            return true;
        }
        draft.c_linkage = has_c_linkage(node, context);
        draft.parameter_count = draft
            .name_node
            .parent()
            .and_then(|declarator| declarator.child_by_field_name("parameters"))
            .map(|parameters| parameter_count(parameters, context));
        draft.parameter_types = draft
            .name_node
            .parent()
            .and_then(|declarator| declarator.child_by_field_name("parameters"))
            .map(|parameters| crate::index::extract::parameter_types(parameters, context.source));
        if draft.name_node.kind() == QUALIFIED_IDENTIFIER {
            // The definition is not in the body of its class or its namespace.
            // The kind is `Method` until `link` finds that the scope is a namespace.
            let (scopes, name) = split_qualified_name(draft.name_node, context);
            draft.name_node = name;
            draft.name = context.text(name);
            if let Some(owner) = scopes.last() {
                draft.kind = if &draft.name == owner {
                    SymbolKind::Constructor
                } else {
                    SymbolKind::Method
                };
                draft.container = Some(owner.clone());
                draft.receiver_type = Some(scopes.join("::"));
                draft.container_path.extend(scopes);
            }
        } else if let Some(scope) = context.container() {
            if scope.role == ScopeRole::Type && scope.name == draft.name {
                draft.kind = SymbolKind::Constructor;
            }
        }
        true
    }

    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let anonymous_namespace = context.scopes.iter().any(|scope| {
            scope.role == ScopeRole::Namespace && scope.exported == Exported::NotExported
        });
        if context.in_function() || anonymous_namespace {
            return Exported::NotExported;
        }
        if draft.receiver_type.is_some() {
            return Exported::Unknown(
                "the declaration in the class or the namespace gives the visibility".to_string(),
            );
        }
        let node = draft.node;
        let holder = match node.kind() {
            LAMBDA_EXPRESSION => node
                .parent()
                .and_then(|declarator| declarator.parent())
                .unwrap_or(node),
            FIELD_IDENTIFIER => {
                data_member(node, context).map_or(node, |(declaration, _)| declaration)
            }
            _ => c::function_holder(node),
        };
        match context.scopes.last() {
            Some(scope) if scope.role == ScopeRole::Type => {
                let public = class_member(holder, scope.node)
                    .is_some_and(|member| is_public_member(member, scope.node, context));
                if public {
                    scope.exported.clone()
                } else {
                    Exported::NotExported
                }
            }
            _ if is_function(node) || node.kind() == LAMBDA_EXPRESSION => {
                exported_if(!c::has_storage_class(holder, "static", context))
            }
            _ if node.kind() == NAMESPACE_DEFINITION => Exported::Exported,
            _ => exported_if(c::is_header(context.file)),
        }
    }

    /// A namespace without a name gives internal linkage to its members. It is
    /// not a symbol and not a container.
    fn scope_kinds(&self) -> Vec<&'static str> {
        vec![NAMESPACE_DEFINITION]
    }

    fn scope<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Scope<'tree>> {
        if node.kind() != NAMESPACE_DEFINITION || node.child_by_field_name("name").is_some() {
            return None;
        }
        Some(Scope {
            node,
            name: String::new(),
            role: ScopeRole::Namespace,
            kind: None,
            exported: Exported::NotExported,
            path: context.path(),
        })
    }

    /// A definition such as `int Scope::name() {}` does not tell if `Scope` is
    /// a class or a namespace, and it does not have the access of the member.
    /// The declaration in a different file gives these facts.
    ///
    /// A declaration agrees with a definition when the full scope path, the
    /// name, and the number of parameters are the same. If the index does not
    /// give one answer, the definition keeps its kind and its unknown visibility.
    fn link(&self, symbols: &mut [Linked<'_>]) {
        let mut updates = Vec::new();
        for (position, definition) in symbols.iter().enumerate() {
            let symbol = &definition.symbol;
            if symbol.receiver_type.is_none() || !matches!(symbol.exported, Exported::Unknown(_)) {
                continue;
            }
            let scope_path = &definition.facts.container_path;
            let name = format!("{}::{}", scope_path.join("::"), symbol.name);
            let outcome = match owner(symbols, scope_path) {
                Owner::Missing => Err(format!(
                    "the index has no scope `{}`",
                    scope_path.join("::")
                )),
                Owner::NotOne => Err(format!(
                    "more than one definition has the name `{}`",
                    scope_path.join("::")
                )),
                Owner::Type => declaration(symbols, position, false)
                    .map_err(|problem| format!("{problem} `{name}`")),
                Owner::Namespace => declaration(symbols, position, true)
                    .map_err(|problem| format!("{problem} `{name}`")),
            };
            updates.push((position, outcome));
        }
        for (position, outcome) in updates {
            let symbol = &mut *symbols[position].symbol;
            match outcome {
                Ok((kind, exported)) => {
                    symbol.kind = kind;
                    symbol.exported = exported;
                    if kind == SymbolKind::Function {
                        symbol.receiver_type = None;
                    }
                }
                Err(reason) => symbol.exported = Exported::Unknown(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, linked_files, rows, rows_in, symbols};

    #[test]
    fn test_free_functions_and_linkage() {
        let source = "namespace {\n\nint local() {\n    return 4;\n}\n\n}  // namespace\n\nstatic int twice_local(int value) {\n    return value * 2;\n}\n\nint run() {\n    auto add = [](int n) { return n + 1; };\n    Circle circle(1.0);\n    std::transform(a, b, [](int n) { return n; });\n    return add(1);\n}\n\ntemplate <typename T>\nT largest(T a, T b) {\n    return a;\n}\n\nstd::string describe();\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Function local - 3-5 not_exported",
                "Function twice_local - 9-11 not_exported",
                "Function run - 13-18 exported",
                "Function add run 14-14 not_exported",
                "Function largest - 21-23 exported",
                "Function describe - 25-25 exported",
            ]
        );
    }

    #[test]
    fn test_namespaces_are_modules_and_containers() {
        let source = "namespace other {\n\nint helper();\n\nint only_here() {\n    return 3;\n}\n\nnamespace inner {\nint deep();\n}\n\n}  // namespace other\nnamespace tools = other;\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Module other - 1-13 exported",
                "Function helper other 3-3 exported",
                "Function only_here other 5-7 exported",
                "Module inner other 9-11 exported",
                "Function deep inner 10-10 exported",
            ]
        );
        assert_eq!(keys(Language::Cpp, source)[4], "file#other.inner.deep");
    }

    #[test]
    fn test_class_members_and_access() {
        let source = "class Shape {\npublic:\n    virtual ~Shape() = default;\n    virtual double area() const = 0;\n};\n\nclass Circle : public Shape, private detail::Base<int> {\n    int hidden();\npublic:\n    explicit Circle(double radius);\n    ~Circle() override;\n    double radius() const {\n        return radius_;\n    }\n    explicit Circle(int side) : radius_(side) {\n    }\n    static int count();\n    int operator+(int x) const;\n    friend void peek(const Circle& circle);\n    class Inner {};\n\nprotected:\n    double scaled(Scale factor) const;\n\nprivate:\n    double radius_;\n    using Local = int;\n};\n\nstruct Point {\n    double x;\n    double norm() const;\n};\n\nclass Forward;\n";
        assert_eq!(
            rows_in(Language::Cpp, "shapes.hpp", source),
            [
                "Class Shape - 1-5 exported",
                "Method ~Shape Shape 3-3 exported",
                "Method area Shape 4-4 exported",
                "Class Circle - 7-28 exported",
                "Method hidden Circle 8-8 not_exported",
                "Constructor Circle Circle 10-10 exported",
                "Method ~Circle Circle 11-11 exported",
                "Method radius Circle 12-14 exported",
                "Constructor Circle Circle 15-16 exported",
                "Method count Circle 17-17 exported",
                "Method operator+ Circle 18-18 exported",
                "Class Inner Circle 20-20 exported",
                "Method scaled Circle 23-23 not_exported",
                "TypeAlias Local Circle 27-27 not_exported",
                "Struct Point - 30-33 exported",
                "Method norm Point 32-32 exported",
            ]
        );
        assert_eq!(
            symbols(Language::Cpp, source)[3].supertypes,
            ["Shape", "detail::Base"]
        );
        assert_eq!(
            rows_in(
                Language::Cpp,
                "shapes.cpp",
                "class Local {\npublic:\n    int go();\n};\n"
            ),
            [
                "Class Local - 1-4 not_exported",
                "Method go Local 3-3 not_exported",
            ]
        );
    }

    #[test]
    fn test_types() {
        let source = "using Scale = double;\nenum class Unit {\n    Metre\n};\ntypedef int Old;\ntemplate <typename T>\nclass Box {\n    T value;\n};\n";
        assert_eq!(
            rows_in(Language::Cpp, "types.hpp", source),
            [
                "TypeAlias Scale - 1-1 exported",
                "Enum Unit - 2-4 exported",
                "TypeAlias Old - 5-5 exported",
                "Class Box - 7-9 exported",
            ]
        );
    }

    #[test]
    fn test_out_of_line_definition_without_its_declaration() {
        let source = "Circle::Circle(double radius) : radius_(radius) {\n}\n\nCircle::~Circle() {\n}\n\ndouble Circle::area() const {\n    return 1;\n}\n\nint a::b::deep() {\n    return 1;\n}\n";
        assert_eq!(
            rows(Language::Cpp, source),
            [
                "Constructor Circle Circle 1-2 unknown",
                "Method ~Circle Circle 4-5 unknown",
                "Method area Circle 7-9 unknown",
                "Method deep b 11-13 unknown",
            ]
        );
        let symbols = symbols(Language::Cpp, source);
        assert_eq!(symbols[2].receiver_type.as_deref(), Some("Circle"));
        assert_eq!(symbols[3].receiver_type.as_deref(), Some("a::b"));
        assert_eq!(symbols[3].key, "file#a.b.deep");
    }

    const CIRCLE_HEADER: &str = "class Circle {\npublic:\n    explicit Circle(double radius);\n    double area() const;\nprivate:\n    double area(double factor) const;\nprotected:\n    double scaled(double factor) const;\n};\n";
    const CIRCLE_SOURCE: &str = "Circle::Circle(double radius) {\n}\ndouble Circle::area() const {\n    return 1;\n}\ndouble Circle::area(double factor) const {\n    return 2;\n}\ndouble Circle::scaled(double factor) const {\n    return 3;\n}\n";

    #[test]
    fn test_link_gives_the_access_of_the_declaration_with_the_same_parameter_count() {
        let files = linked_files(&[
            (Language::Cpp, "shapes.hpp", CIRCLE_HEADER),
            (Language::Cpp, "shapes.cpp", CIRCLE_SOURCE),
        ]);
        assert_eq!(
            files["shapes.cpp"],
            [
                "Constructor Circle Circle 1-2 exported",
                "Method area Circle 3-5 exported",
                "Method area Circle 6-8 not_exported",
                "Method scaled Circle 9-11 not_exported",
            ]
        );
    }

    #[test]
    fn test_link_does_not_depend_on_the_file_order() {
        let header = (Language::Cpp, "shapes.hpp", CIRCLE_HEADER);
        let source = (Language::Cpp, "shapes.cpp", CIRCLE_SOURCE);
        let namespace = (
            Language::Cpp,
            "util.hpp",
            "namespace util {\nint helper();\n}\n",
        );
        let function = (
            Language::Cpp,
            "util.cpp",
            "int util::helper() {\n    return 1;\n}\n",
        );
        let ordered = linked_files(&[header, source, namespace, function]);
        assert_eq!(
            linked_files(&[function, namespace, source, header]),
            ordered
        );
        assert_eq!(
            linked_files(&[source, function, header, namespace]),
            ordered
        );
        assert_eq!(ordered["util.cpp"], ["Function helper util 1-3 exported"]);
    }

    #[test]
    fn test_link_makes_a_namespace_member_a_function() {
        let header = "namespace util {\nint helper();\nstatic int quiet();\n}\nclass Util {\npublic:\n    static int helper();\n};\n";
        let source = "int util::helper() {\n    return 1;\n}\nint util::quiet() {\n    return 2;\n}\nint Util::helper() {\n    return 5;\n}\nint missing::helper() {\n    return 6;\n}\nint util::absent() {\n    return 7;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "util.hpp", header),
            (Language::Cpp, "util.cpp", source),
        ]);
        assert_eq!(
            files["util.cpp"],
            [
                "Function helper util 1-3 exported",
                "Function quiet util 4-6 not_exported",
                "Method helper Util 7-9 exported",
                "Method helper missing 10-12 unknown",
                "Method absent util 13-15 unknown",
            ]
        );
    }

    #[test]
    fn test_link_uses_the_full_scope_path() {
        let header = "namespace a {\nclass B {\npublic:\n    void run();\n};\n}\nnamespace c {\nclass B {\n    void run();\n};\n}\nnamespace d::e {\nint deep();\n}\n";
        let source = "void a::B::run() {\n}\nvoid c::B::run() {\n}\nvoid B::run() {\n}\nnamespace a {\nvoid B::run() {\n}\n}\nint d::e::deep() {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "b.hpp", header),
            (Language::Cpp, "b.cpp", source),
        ]);
        assert_eq!(
            files["b.cpp"],
            [
                "Method run B 1-2 exported",
                "Method run B 3-4 not_exported",
                "Method run B 5-6 unknown",
                "Module a - 7-10 exported",
                "Method run B 8-9 exported",
                "Function deep e 11-13 exported",
            ]
        );
    }

    #[test]
    fn test_link_abstains_when_declarations_with_the_same_parameter_count_do_not_agree() {
        let header = "class Circle {\npublic:\n    double area(int factor) const;\nprivate:\n    double area(double factor) const;\n};\n";
        let source = "double Circle::area(double factor) const {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "shapes.hpp", header),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(files["shapes.cpp"], ["Method area Circle 1-3 unknown"]);
    }

    #[test]
    fn test_link_abstains_when_two_types_have_the_same_qualified_name() {
        let first = "class Circle {\npublic:\n    double area() const;\n};\n";
        let second = "class Circle {\npublic:\n    double area() const;\n};\n";
        let source = "double Circle::area() const {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "one.hpp", first),
            (Language::Cpp, "two.hpp", second),
            (Language::Cpp, "shapes.cpp", source),
        ]);
        assert_eq!(files["shapes.cpp"], ["Method area Circle 1-3 unknown"]);
    }

    #[test]
    fn test_link_abstains_when_a_type_and_a_namespace_have_the_same_qualified_name() {
        let namespace = "namespace shape {\nint area();\n}\n";
        let class = "class shape {\npublic:\n    int area();\n};\n";
        let source = "int shape::area() {\n    return 1;\n}\n";
        let files = linked_files(&[
            (Language::Cpp, "namespace.hpp", namespace),
            (Language::Cpp, "class.hpp", class),
            (Language::Cpp, "shape.cpp", source),
        ]);
        assert_eq!(files["shape.cpp"], ["Method area shape 1-3 unknown"]);
    }

    #[test]
    fn test_includes() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "main.cpp",
                "#include <vector>\n#include \"shapes.hpp\"\n#include \"sub/util.hpp\"\n#include \"plain.h\"\nusing namespace other;\nnamespace tools = util;\n",
            ),
            ("shapes.hpp", ""),
            ("sub/util.hpp", ""),
            ("sub/util.cpp", "#include \"util.hpp\"\n"),
            ("include/plain.h", ""),
        ];
        assert_eq!(
            rows(&files, "main.cpp"),
            [
                "1 Load vector -->- external",
                "2 Load shapes.hpp -->- shapes.hpp",
                "3 Load sub/util.hpp -->- sub/util.hpp",
                "4 Load plain.h -->- include/plain.h",
            ]
        );
        assert_eq!(
            rows(&files, "sub/util.cpp"),
            ["1 Load util.hpp -->- sub/util.hpp"]
        );
    }

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "main.cpp",
            "#include <vector>\n\n#include \"other.hpp\"\n#include \"shapes.hpp\"\n\nusing namespace other;\nnamespace tools = util;\n\nnamespace {\n\nint local() {\n    return 4;\n}\n\n}  // namespace\n\nint run() {\n    int total = local() + util::helper() + only_here() + tools::helper();\n    auto circle = Circle(1.0);\n    total += static_cast<int>(circle.area());\n    auto add = [](int n) { return n + 1; };\n    return total + add(1) + util::twice(2) + util::twice(2, 3) + Util::helper();\n}\n\ndouble measure(const Circle& size, const Circle* pointer, const Shape* shape) {\n    return size.area() + pointer->area(2.0) + shape->area();\n}\n\ndouble made() {\n    Circle braces = Circle{2.0};\n    Circle* heap = new Circle(1.0);\n    Circle declared(1.0);\n    std::vector<int> seen;\n    seen.push_back(1);\n    return braces.radius() + heap->radius() + declared.radius() + std::sqrt(4.0) + describe();\n}\n",
        ),
        (
            "other.hpp",
            "namespace other {\n\nint only_here();\nint push_back(int value);\n\n}  // namespace other\n\nnamespace util {\n\nint helper();\nint twice(int value);\nint twice(int value, int times);\n\n}  // namespace util\n\nclass Util {\npublic:\n    static int helper() {\n        return 5;\n    }\n};\n",
        ),
        (
            "other.cpp",
            "#include \"other.hpp\"\n\nnamespace other {\n\nint only_here() {\n    return 3;\n}\n\nint push_back(int value) {\n    return value;\n}\n\n}  // namespace other\n\nint util::helper() {\n    return 1;\n}\n\nint util::twice(int value) {\n    return value * 2;\n}\n\nint util::twice(int value, int times) {\n    return value * times;\n}\n",
        ),
        (
            "shapes.hpp",
            "#include <string>\n\nclass Shape {\npublic:\n    virtual double area() const = 0;\n};\n\nclass Circle : public Shape {\npublic:\n    explicit Circle(double radius);\n    double area() const override;\n    double area(double factor) const;\n    double radius() const {\n        return radius_;\n    }\n\nprotected:\n    double scaled(double factor) const;\n\nprivate:\n    double radius_;\n};\n\nstd::string describe();\n",
        ),
        (
            "shapes.cpp",
            "#include \"shapes.hpp\"\n\nCircle::Circle(double radius) : radius_(radius) {\n}\n\ndouble Circle::area() const {\n    return this->scaled(3.14);\n}\n\ndouble Circle::area(double factor) const {\n    return scaled(factor);\n}\n\ndouble Circle::scaled(double factor) const {\n    return factor;\n}\n\nstd::string describe() {\n    return \"shape\";\n}\n",
        ),
    ];

    #[test]
    fn test_calls_with_namespaces_and_constructions() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.cpp")[..11],
            [
                "18 run>local UniqueCandidate SameFile [main.cpp#local]",
                "18 run>util::helper UniqueCandidate QualifiedPath [other.cpp#util.helper]",
                "18 run>only_here UniqueCandidate ImportedModule [other.cpp#other.only_here]",
                "18 run>tools::helper UniqueCandidate QualifiedPath [other.cpp#util.helper]",
                "19 run>Circle UniqueCandidate ImportedModule [shapes.cpp#Circle.Circle]",
                "20 run>circle.area UniqueCandidate ReceiverDeclaredType [shapes.cpp#Circle.area]",
                "22 run>add UniqueCandidate LexicalScope [main.cpp#run.add]",
                "22 run>util::twice UniqueCandidate QualifiedPath [other.cpp#util.twice]",
                "22 run>util::twice UniqueCandidate QualifiedPath [other.cpp#util.twice~2]",
                "22 run>Util::helper UniqueCandidate QualifiedPath [other.hpp#Util.helper]",
                "26 measure>size.area UniqueCandidate ReceiverDeclaredType [shapes.cpp#Circle.area]",
            ]
        );
    }

    #[test]
    fn test_calls_with_declared_types() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.cpp")[11..],
            [
                "26 measure>pointer.area UniqueCandidate ReceiverDeclaredType [shapes.cpp#Circle.area~2]",
                "26 measure>shape.area Ambiguous NameOnly [shapes.cpp#Circle.area, shapes.cpp#Circle.area~2, shapes.hpp#Circle.area, shapes.hpp#Circle.area~2, shapes.hpp#Shape.area]",
                "30 made>Circle UniqueCandidate QualifiedPath [shapes.cpp#Circle.Circle]",
                "31 made>Circle UniqueCandidate QualifiedPath [shapes.cpp#Circle.Circle]",
                "32 made>Circle UniqueCandidate QualifiedPath [shapes.cpp#Circle.Circle]",
                "34 made>seen.push_back Unresolved - []",
                "35 made>braces.radius UniqueCandidate ReceiverDeclaredType [shapes.hpp#Circle.radius]",
                "35 made>heap.radius UniqueCandidate ReceiverDeclaredType [shapes.hpp#Circle.radius]",
                "35 made>declared.radius UniqueCandidate ReceiverDeclaredType [shapes.hpp#Circle.radius]",
                "35 made>std::sqrt Unresolved - []",
                "35 made>describe UniqueCandidate ImportedModule [shapes.cpp#describe]",
            ]
        );
    }

    #[test]
    fn test_calls_with_this() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "shapes.cpp"),
            [
                "7 Circle.area>self.scaled UniqueCandidate EnclosingType [shapes.cpp#Circle.scaled]",
                "11 Circle.area~2>scaled UniqueCandidate EnclosingType [shapes.cpp#Circle.scaled]",
            ]
        );
    }
    const FIELD_SOURCE: &str = r#"#include <array>
#include <memory>
#include <vector>

class Item {
public:
    int name() const { return 1; }
    Item* getOut() { return this; }
};

class Node {
public:
    int run(const std::vector<Item>& given);
    Item spare;

private:
    Node* parent_;
    Item item_, *extra_;
    Item& ref_;
    std::unique_ptr<Item> owned_;
    std::shared_ptr<Node> shared_;
    std::vector<Item> items_;
    std::array<Item, 3> fixed_;
    Item raw_[2];
    std::vector<std::unique_ptr<Item>> boxes_;
    int count_ = 0;
    static int total_;
    void helper();
};

int Node::run(const std::vector<Item>& given) {
    parent_->item_.name();
    this->parent_->extra_->name();
    owned_->name();
    shared_->parent_->spare.name();
    ref_.getOut()->name();
    for (auto& each : items_) { each.name(); }
    for (const auto& each : fixed_) { each.name(); }
    for (auto each : raw_) { each.name(); }
    for (auto& each : given) { each.name(); }
    for (auto& each : this->boxes_) { each->name(); }
    for (Item each : items_) { each.name(); }
    std::unique_ptr<Item> local = nullptr;
    local->name();
    items_.name();
    count_.name();
    this->missing_.name();
    parent_->missing_.name();
    return 0;
}
"#;

    #[test]
    fn test_data_members_have_the_declared_type() {
        use crate::index::extract::testing::fields;
        assert_eq!(
            fields(Language::Cpp, FIELD_SOURCE),
            [
                "Field spare Node 14-14 not_exported",
                "Field parent_ Node 17-17 not_exported",
                "Field item_ Node 18-18 not_exported",
                "Field extra_ Node 18-18 not_exported",
                "Field ref_ Node 19-19 not_exported",
                "Field owned_ Node 20-20 not_exported",
                "Field shared_ Node 21-21 not_exported",
                "Field items_ Node 22-22 not_exported",
                "Field fixed_ Node 23-23 not_exported",
                "Field raw_ Node 24-24 not_exported",
                "Field boxes_ Node 25-25 not_exported",
                "Field count_ Node 26-26 not_exported",
                "Field total_ Node 27-27 not_exported",
            ]
        );
        // A data member does not change the rows of the other symbols.
        assert_eq!(
            rows(Language::Cpp, FIELD_SOURCE),
            [
                "Class Item - 5-9 not_exported",
                "Method name Item 7-7 not_exported",
                "Method getOut Item 8-8 not_exported",
                "Class Node - 11-29 not_exported",
                "Method run Node 13-13 not_exported",
                "Method helper Node 28-28 not_exported",
                "Method run Node 31-50 unknown",
            ]
        );
    }

    #[test]
    fn test_member_chains_and_range_for_give_the_receiver_type() {
        use crate::index::resolve::testing::rows;
        let found: Vec<String> = rows(&[("node.cpp", FIELD_SOURCE)], "node.cpp")
            .into_iter()
            .filter(|row| row.contains("name ") || row.contains(".getOut "))
            .map(|row| row.replace("Node.run~2>", "").replace("Node.run>", ""))
            .collect();
        let name = "UniqueCandidate ReceiverDeclaredType [node.cpp#Item.name]";
        let no_type = "Ambiguous NameOnly [node.cpp#Item.name]";
        assert_eq!(
            found,
            [
                format!("32 parent_->item_::name {name}"),
                format!("33 (..).name {name}"),
                format!("34 owned_.name {name}"),
                format!("35 shared_->parent_->spare::name {name}"),
                format!("36 (..).name {name}"),
                "36 ref_.getOut UniqueCandidate ReceiverDeclaredType [node.cpp#Item.getOut]"
                    .to_string(),
                format!("37 each.name {name}"),
                format!("38 each.name {name}"),
                format!("39 each.name {name}"),
                format!("40 each.name {name}"),
                format!("41 each.name {name}"),
                format!("42 each.name {name}"),
                format!("44 local.name {name}"),
                // A collection is not an element.
                "45 items_.name Unresolved - []".to_string(),
                "46 count_.name Unresolved - []".to_string(),
                // A hop without a declared type gives candidates and no edge.
                format!("47 (..).name {no_type}"),
                format!("48 parent_->missing_::name {no_type}"),
            ]
        );
    }
    #[test]
    fn test_a_member_of_the_smart_pointer_itself_gives_no_edge() {
        use crate::index::resolve::testing::rows;
        // `owned_.reset()` calls `std::unique_ptr::reset`. `Item::reset` has
        // the same name, and an edge to it is false.
        let source = r#"#include <memory>

class Item {
public:
    void reset() {}
    Item* get() { return this; }
};

class Holder {
public:
    void run(Item& given, std::unique_ptr<Item> passed);

private:
    std::unique_ptr<Item> owned_;
    Item plain_;
    Holder* next_;
};

void Holder::run(Item& given, std::unique_ptr<Item> passed) {
    owned_.reset();
    owned_->reset();
    this->owned_.reset();
    next_->owned_.get();
    passed.reset();
    passed->reset();
    Item local;
    local.reset();
    given.get();
    auto made = std::make_unique<Item>();
    made.reset();
    plain_.reset();
}
"#;
        let found: Vec<String> = rows(&[("holder.cpp", source)], "holder.cpp")
            .into_iter()
            .filter(|row| row.contains("reset ") || row.contains("get "))
            .map(|row| row.replace("Holder.run~2>", "").replace("Holder.run>", ""))
            .collect();
        let reset = "UniqueCandidate ReceiverDeclaredType [holder.cpp#Item.reset]";
        assert_eq!(
            found,
            [
                // The receiver is not a local name with a declared value type.
                "20 (..).reset Ambiguous NameOnly [holder.cpp#Item.reset]".to_string(),
                format!("21 owned_.reset {reset}"),
                // Lines 22 and 23 have a chain before `.`. They give no row.
                "24 (..).reset Ambiguous NameOnly [holder.cpp#Item.reset]".to_string(),
                format!("25 passed.reset {reset}"),
                // A local name and a parameter with a value type are safe.
                format!("27 local.reset {reset}"),
                "28 given.get UniqueCandidate ReceiverDeclaredType [holder.cpp#Item.get]"
                    .to_string(),
                "30 (..).reset Ambiguous NameOnly [holder.cpp#Item.reset]".to_string(),
                // A data member with a declared value type is an object.
                format!("31 (..).reset {reset}"),
            ]
        );
    }
}
