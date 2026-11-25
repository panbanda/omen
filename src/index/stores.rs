//! The stores that Pinia `defineStore` defines.
//!
//! `export const useUsers = defineStore(id, setup)` defines a function. Its
//! call gives the store, and each member of the store is a key of the object
//! that `setup` returns. A store of the options form has the members of its
//! `actions` object. The syntax gives a member only when it shows the
//! function of the key. Each other member abstains: a key of a value that is
//! not a function, a key that a later spread can change, and each member of
//! a setup function with more than one `return`.

use std::collections::{BTreeMap, HashMap};

use tree_sitter::Node;

use super::model::{ImportBinding, ImportKind};
use crate::parser::ParseResult;

/// The module and the function of Pinia that define a store.
const PINIA: &str = "pinia";
const DEFINE_STORE: &str = "defineStore";

/// The node kinds of a function value or a declaration of a function. A
/// `return` in one of them does not return from the function that contains it.
const FUNCTION_KINDS: [&str; 6] = [
    "function_declaration",
    "generator_function_declaration",
    "function_expression",
    "generator_function",
    "arrow_function",
    "method_definition",
];

/// The node kinds of a function value that a declaration can hold.
const FUNCTION_VALUE_KINDS: [&str; 3] = [
    "function_expression",
    "generator_function",
    "arrow_function",
];

/// One store at the top level of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// The name of the variable that holds the function of the store.
    pub name: String,
    /// The names, different from `name`, under which the file exports the store.
    pub aliases: Vec<String>,
    /// The members whose function the syntax shows, by key: the position of
    /// the function in `FileSymbols::symbols`. Each other member abstains.
    pub members: BTreeMap<String, usize>,
}

/// A member of the object of a store, before the walk knows the symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    /// The start byte of the name of the function.
    Function(usize),
    Abstain,
}

/// The stores of one ECMAScript file, sorted by name. `symbols_by_name`
/// gives the position of each symbol by the start byte of its name. The
/// function asks for it only in a file that imports `defineStore`.
pub fn stores(
    parsed: &ParseResult,
    imports: &[ImportBinding],
    export_aliases: &[(String, String)],
    symbols_by_name: impl FnOnce() -> HashMap<usize, usize>,
) -> Vec<Store> {
    let defines: Vec<&str> = imports
        .iter()
        .filter(|binding| {
            binding.kind == ImportKind::Named
                && binding.module_path == PINIA
                && binding.imported_name.as_deref() == Some(DEFINE_STORE)
        })
        .filter_map(|binding| binding.local_name.as_deref())
        .collect();
    if defines.is_empty() {
        return Vec::new();
    }
    let symbol_at = symbols_by_name();
    let source = &parsed.source[..];
    let text = |node: Node<'_>| std::str::from_utf8(&source[node.byte_range()]).unwrap_or("");
    let mut found = Vec::new();
    let root = parsed.root_node();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        let declaration = match statement.kind() {
            "export_statement" => statement.child_by_field_name("declaration"),
            _ => Some(statement),
        };
        let Some(declaration) = declaration.filter(|node| node.kind() == "lexical_declaration")
        else {
            continue;
        };
        let mut inner = declaration.walk();
        for declarator in declaration.named_children(&mut inner) {
            let name = declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier");
            let call = declarator
                .child_by_field_name("value")
                .filter(|value| value.kind() == "call_expression");
            let (Some(name), Some(call)) = (name, call) else {
                continue;
            };
            let is_define = call
                .child_by_field_name("function")
                .is_some_and(|function| defines.contains(&text(function)));
            if !is_define {
                continue;
            }
            let name = text(name).to_string();
            let members = call
                .child_by_field_name("arguments")
                .map(|arguments| definition_members(arguments, &text))
                .unwrap_or_default();
            let members = members
                .into_iter()
                .filter_map(|(key, member)| match member {
                    Member::Function(byte) => Some((key, *symbol_at.get(&byte)?)),
                    Member::Abstain => None,
                })
                .collect();
            let aliases = export_aliases
                .iter()
                .filter(|(local, _)| *local == name)
                .map(|(_, alias)| alias.clone())
                .collect();
            found.push(Store {
                name,
                aliases,
                members,
            });
        }
    }
    found.sort_by(|left, right| left.name.cmp(&right.name));
    found
}

/// The members of the arguments of `defineStore`. `defineStore(id, setup)`
/// and `defineStore(id, options)` have the definition second, and
/// `defineStore(options)` has it first.
fn definition_members<'tree>(
    arguments: Node<'tree>,
    text: &impl Fn(Node<'tree>) -> &'tree str,
) -> BTreeMap<String, Member> {
    let mut cursor = arguments.walk();
    let values: Vec<Node<'tree>> = arguments
        .named_children(&mut cursor)
        .filter(|node| node.kind() != "comment")
        .collect();
    let definition = match values.as_slice() {
        [first, ..] if first.kind() == "object" => Some(*first),
        [_, second, ..] => Some(*second),
        _ => None,
    };
    match definition {
        Some(options) if options.kind() == "object" => options_members(options, text),
        Some(setup) if FUNCTION_VALUE_KINDS.contains(&setup.kind()) => setup_members(setup, text),
        _ => BTreeMap::new(),
    }
}

/// The members of the `actions` object of the options form. A spread or a
/// computed key in the options can replace `actions`, so it gives no member.
fn options_members<'tree>(
    options: Node<'tree>,
    text: &impl Fn(Node<'tree>) -> &'tree str,
) -> BTreeMap<String, Member> {
    let mut actions = Vec::new();
    let mut cursor = options.walk();
    for entry in options.named_children(&mut cursor) {
        match entry.kind() {
            "pair" => match entry
                .child_by_field_name("key")
                .map(|key| key_text(key, text))
            {
                Some(Some(key)) if key == "actions" => actions.extend(
                    entry
                        .child_by_field_name("value")
                        .filter(|value| value.kind() == "object"),
                ),
                Some(Some(_)) => {}
                _ => return BTreeMap::new(),
            },
            // `{ actions }` names a value that the syntax does not show.
            "spread_element" | "shorthand_property_identifier" => return BTreeMap::new(),
            _ => {}
        }
    }
    match actions.as_slice() {
        [actions] => object_members(*actions, &HashMap::new(), text),
        _ => BTreeMap::new(),
    }
}

/// The members of the object that a setup function returns. The function
/// must return one object, from one `return` directly in its body or from
/// the expression of an arrow function.
fn setup_members<'tree>(
    setup: Node<'tree>,
    text: &impl Fn(Node<'tree>) -> &'tree str,
) -> BTreeMap<String, Member> {
    let Some(body) = setup.child_by_field_name("body") else {
        return BTreeMap::new();
    };
    if body.kind() != "statement_block" {
        return match unparenthesized(body) {
            object if object.kind() == "object" => object_members(object, &HashMap::new(), text),
            _ => BTreeMap::new(),
        };
    }
    let returns = returns_of(body);
    let [only] = returns.as_slice() else {
        return BTreeMap::new();
    };
    let returned = only
        .parent()
        .filter(|parent| parent.id() == body.id())
        .and_then(|_| only.named_child(0))
        .map(unparenthesized)
        .filter(|value| value.kind() == "object");
    match returned {
        Some(object) => object_members(object, &declarations(body, text), text),
        None => BTreeMap::new(),
    }
}

/// The `return` statements of a function body, without the statements of
/// the functions in it.
fn returns_of(body: Node<'_>) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut pending = vec![body];
    while let Some(node) = pending.pop() {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "return_statement" {
                found.push(child);
            } else if !FUNCTION_KINDS.contains(&child.kind()) {
                pending.push(child);
            }
        }
    }
    found
}

/// The names that the statements directly in a body declare: the start
/// byte of the name of a function, or `Abstain` for a different value. A
/// `let` or a `var` can get a different value later, thus it abstains.
fn declarations<'tree>(
    body: Node<'tree>,
    text: &impl Fn(Node<'tree>) -> &'tree str,
) -> HashMap<&'tree str, Member> {
    let mut found = HashMap::new();
    let mut cursor = body.walk();
    for statement in body.named_children(&mut cursor) {
        match statement.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = statement.child_by_field_name("name") {
                    found.insert(text(name), Member::Function(name.start_byte()));
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                let constant = statement
                    .child_by_field_name("kind")
                    .is_some_and(|keyword| keyword.kind() == "const");
                let mut inner = statement.walk();
                for declarator in statement.named_children(&mut inner) {
                    let Some(name) = declarator
                        .child_by_field_name("name")
                        .filter(|name| name.kind() == "identifier")
                    else {
                        continue;
                    };
                    let function = constant
                        && declarator
                            .child_by_field_name("value")
                            .is_some_and(|value| FUNCTION_VALUE_KINDS.contains(&value.kind()));
                    let member = if function {
                        Member::Function(name.start_byte())
                    } else {
                        Member::Abstain
                    };
                    found.insert(text(name), member);
                }
            }
            _ => {}
        }
    }
    found
}

/// The members of an object literal, in the order of its entries. A later
/// key replaces an earlier one. A spread or a computed key can replace each
/// key before it, thus those keys abstain.
fn object_members<'tree>(
    object: Node<'tree>,
    declared: &HashMap<&'tree str, Member>,
    text: &impl Fn(Node<'tree>) -> &'tree str,
) -> BTreeMap<String, Member> {
    let mut members = BTreeMap::new();
    let mut cursor = object.walk();
    for entry in object.named_children(&mut cursor) {
        let declared_name =
            |name: Node<'tree>| declared.get(text(name)).copied().unwrap_or(Member::Abstain);
        let (key, member) = match entry.kind() {
            "shorthand_property_identifier" => {
                (Some(text(entry).to_string()), declared_name(entry))
            }
            "method_definition" => {
                let accessor = {
                    let mut inner = entry.walk();
                    let found = entry
                        .children(&mut inner)
                        .any(|child| ["get", "set"].contains(&child.kind()));
                    found
                };
                let name = entry.child_by_field_name("name");
                let member = match name {
                    Some(name) if !accessor => Member::Function(name.start_byte()),
                    _ => Member::Abstain,
                };
                (name.and_then(|name| key_text(name, text)), member)
            }
            "pair" => {
                let key = entry.child_by_field_name("key");
                let value = entry.child_by_field_name("value").map(unparenthesized);
                let member = match (key, value) {
                    (Some(_), Some(value)) if value.kind() == "identifier" => declared_name(value),
                    (Some(key), Some(value)) if FUNCTION_VALUE_KINDS.contains(&value.kind()) => {
                        Member::Function(key.start_byte())
                    }
                    _ => Member::Abstain,
                };
                (key.and_then(|key| key_text(key, text)), member)
            }
            "spread_element" => (None, Member::Abstain),
            _ => continue,
        };
        match key {
            Some(key) => {
                members.insert(key, member);
            }
            None => {
                for value in members.values_mut() {
                    *value = Member::Abstain;
                }
            }
        }
    }
    members
}

/// The text of a key, or `None` for a computed key.
fn key_text<'tree>(key: Node<'tree>, text: &impl Fn(Node<'tree>) -> &'tree str) -> Option<String> {
    match key.kind() {
        "property_identifier" | "number" | "private_property_identifier" => {
            Some(text(key).to_string())
        }
        "string" => {
            let mut cursor = key.walk();
            let parts: Vec<Node<'tree>> = key.named_children(&mut cursor).collect();
            // A key with an escape sequence is read as a computed key.
            parts
                .iter()
                .all(|part| part.kind() == "string_fragment")
                .then(|| parts.into_iter().map(text).collect())
        }
        _ => None,
    }
}

fn unparenthesized(mut node: Node<'_>) -> Node<'_> {
    while node.kind() == "parenthesized_expression" {
        match node.named_child(0) {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}
