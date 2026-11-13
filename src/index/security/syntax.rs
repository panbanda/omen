//! Syntax helpers for the argument predicates and the syntax sinks.

use tree_sitter::Node;

use super::spec::{LanguageSyntax, Predicate, TOKEN_FORMATS};
use crate::core::Language;

pub fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

/// The innermost node with exactly the byte range of a call. The ranges of
/// wrapper nodes, such as a statement, can be the same.
pub fn call_node<'tree>(
    root: Node<'tree>,
    start: usize,
    end: usize,
    call_kinds: &[&str],
) -> Option<Node<'tree>> {
    let mut node = root.descendant_for_byte_range(start, end)?;
    let mut fallback = None;
    loop {
        if node.start_byte() != start || node.end_byte() != end {
            return fallback;
        }
        if call_kinds.contains(&node.kind()) {
            return Some(node);
        }
        fallback.get_or_insert(node);
        node = node.parent()?;
    }
}

/// The argument nodes of a call, without comments and without the wrapper of an argument.
pub fn arguments<'tree>(call: Node<'tree>, language: Language) -> Vec<Node<'tree>> {
    let mut nodes: Vec<Node<'tree>> = Vec::new();
    if language == Language::Bash {
        let mut cursor = call.walk();
        nodes.extend(call.children_by_field_name("argument", &mut cursor));
    } else if let Some(list) = call.child_by_field_name("arguments") {
        let mut cursor = list.walk();
        nodes.extend(list.named_children(&mut cursor));
    }
    nodes
        .into_iter()
        .filter(|node| !node.kind().contains("comment"))
        .map(|node| {
            if node.kind() == "argument" {
                let count = node.named_child_count();
                node.named_child(count.saturating_sub(1) as u32)
                    .unwrap_or(node)
            } else {
                node
            }
        })
        .collect()
}

/// The text of a string literal without its prefix and its quotation marks.
pub fn unquote(text: &str) -> &str {
    let text = text.trim();
    let Some(quote) = text.find(['"', '\'', '`']) else {
        return text;
    };
    let prefix = &text[..quote];
    if quote > 2
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '$' || c == '@')
    {
        return text;
    }
    let body = &text[quote..];
    for mark in ["\"\"\"", "'''"] {
        if let Some(inner) = body.strip_prefix(mark).and_then(|r| r.strip_suffix(mark)) {
            return inner;
        }
    }
    let mut chars = body.chars();
    let first = chars.next();
    match (first, body.chars().last()) {
        (Some(open), Some(close)) if open == close && body.len() >= 2 => {
            &body[open.len_utf8()..body.len() - close.len_utf8()]
        }
        _ => text,
    }
}

/// The key and the value of a keyword argument or of a field in a literal, in lower case.
pub fn key_value(node: Node<'_>, source: &[u8]) -> Option<(String, String)> {
    let clean = |text: &str| {
        text.trim()
            .trim_matches(|c: char| c == ':' || c == '"' || c == '\'' || c == ' ')
            .to_lowercase()
    };
    let (key, value) = match node.kind() {
        "keyword_argument" => (
            node.child_by_field_name("name")?,
            node.child_by_field_name("value")?,
        ),
        "pair" => (
            node.child_by_field_name("key")?,
            node.child_by_field_name("value")?,
        ),
        "keyed_element" => {
            let count = node.named_child_count();
            if count < 2 {
                return None;
            }
            (node.named_child(0)?, node.named_child((count - 1) as u32)?)
        }
        _ => return None,
    };
    Some((clean(text(key, source)), clean(text(value, source))))
}

fn has_descendant(node: Node<'_>, test: &dyn Fn(Node<'_>) -> bool) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| test(child) || has_descendant(child, test))
}

fn is_interpolation(kind: &str) -> bool {
    kind.contains("interpolation") || kind.contains("substitution")
}

fn has_interpolation(node: Node<'_>) -> bool {
    is_interpolation(node.kind())
        || has_descendant(node, &|child| is_interpolation(child.kind()))
        || (node.kind() == "encapsed_string"
            && has_descendant(node, &|child| child.kind() == "variable_name"))
}

const FORMAT_CALLEES: [&str; 8] = [
    ".format",
    "Sprintf",
    "sprintf",
    "String.Format",
    "String.format",
    "format!",
    "concat",
    "format",
];

/// A string that code builds from parts: a concatenation with a literal, an
/// interpolation, or a call of a formatting function.
pub fn is_built_string(node: Node<'_>, source: &[u8], syntax: &LanguageSyntax) -> bool {
    match node.kind() {
        "parenthesized_expression"
        | "reference_expression"
        | "unary_expression"
        | "await_expression"
        | "argument" => node
            .named_child(0)
            .is_some_and(|inner| is_built_string(inner, source, syntax)),
        "binary_expression" | "binary_operator" | "binary" | "additive_expression" => {
            let mut cursor = node.walk();
            let joins = node
                .children(&mut cursor)
                .any(|child| !child.is_named() && matches!(text(child, source), "+" | "." | "%"));
            joins && has_descendant(node, &|child| syntax.string_kinds.contains(&child.kind()))
        }
        "call"
        | "call_expression"
        | "macro_invocation"
        | "method_invocation"
        | "invocation_expression"
        | "function_call_expression"
        | "member_call_expression" => {
            let whole = text(node, source);
            let callee = whole
                .split(['(', '[', '{'])
                .next()
                .unwrap_or("")
                .trim_end_matches(' ');
            FORMAT_CALLEES.iter().any(|name| callee.ends_with(name))
        }
        _ => has_interpolation(node),
    }
}

/// A literal without an interpolation.
pub fn is_literal(node: Node<'_>) -> bool {
    let kind = node.kind();
    if kind.contains("string") {
        return !has_interpolation(node);
    }
    [
        "integer", "float", "number", "true", "false", "null", "nil", "none", "char", "boolean",
        "decimal",
    ]
    .iter()
    .any(|word| kind.contains(word))
}

fn visit(node: Node<'_>, test: &mut dyn FnMut(Node<'_>) -> bool) -> bool {
    if test(node) {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    children.into_iter().any(|child| visit(child, test))
}

/// Tells if the predicate holds for the arguments of a call.
pub fn holds(
    predicate: &Predicate,
    arguments: &[Node<'_>],
    source: &[u8],
    syntax: &LanguageSyntax,
) -> bool {
    let argument_text = |node: &Node<'_>| unquote(text(*node, source)).to_lowercase();
    match predicate {
        Predicate::Always => true,
        Predicate::Keyword { name, value } => arguments.iter().any(|argument| {
            visit(*argument, &mut |node| {
                key_value(node, source).is_some_and(|(key, found)| key == *name && found == *value)
            })
        }),
        Predicate::ArgIn { index, values } => arguments
            .get(*index)
            .is_some_and(|node| values.contains(&argument_text(node).as_str())),
        Predicate::ArgStartsWith { index, prefixes } => arguments.get(*index).is_some_and(|node| {
            let found = argument_text(node);
            prefixes.iter().any(|prefix| found.starts_with(prefix))
        }),
        Predicate::ArgContains { index, needles } => arguments.get(*index).is_some_and(|node| {
            let found = argument_text(node);
            needles.iter().any(|needle| found.contains(needle))
        }),
        Predicate::AnyArgIn(values) => arguments
            .iter()
            .any(|node| values.contains(&argument_text(node).as_str())),
        Predicate::AnyArgStartsWith(prefixes) => arguments.iter().any(|node| {
            let found = argument_text(node);
            prefixes.iter().any(|prefix| found.starts_with(prefix))
        }),
        Predicate::NoArgContains(needle) => !arguments
            .iter()
            .any(|node| text(*node, source).contains(needle)),
        Predicate::BuiltString { index: Some(index) } => arguments
            .get(*index)
            .is_some_and(|node| is_built_string(*node, source, syntax)),
        Predicate::BuiltString { index: None } => arguments
            .iter()
            .any(|node| is_built_string(*node, source, syntax)),
        Predicate::All(all) => all
            .iter()
            .all(|inner| holds(inner, arguments, source, syntax)),
    }
}

/// The prefix of the token format that the string literal has, if it has one.
pub fn token_prefix(literal: &str) -> Option<&'static str> {
    let content = unquote(literal);
    TOKEN_FORMATS.iter().find_map(|format| {
        let tail = content.strip_prefix(format.prefix)?;
        let length = tail.chars().count();
        (length >= format.min_tail && length <= format.max_tail && tail.chars().all(format.tail))
            .then_some(format.prefix)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_removes_prefixes_and_quotation_marks() {
        assert_eq!(unquote("\"md5\""), "md5");
        assert_eq!(unquote("'md5'"), "md5");
        assert_eq!(unquote("f\"a{b}\""), "a{b}");
        assert_eq!(unquote("b'x'"), "x");
        assert_eq!(unquote("\"\"\"long\"\"\""), "long");
        assert_eq!(unquote("`tpl`"), "tpl");
        assert_eq!(unquote("0L"), "0L");
        assert_eq!(unquote("value"), "value");
        assert_eq!(unquote("call(\"x\")"), "call(\"x\")");
    }

    #[test]
    fn a_token_needs_its_prefix_and_its_length() {
        assert_eq!(token_prefix("\"AKIAIOSFODNN7EXAMPLE\""), Some("AKIA"));
        assert_eq!(token_prefix("\"AKIAEXAMPLE\""), None);
        assert_eq!(token_prefix("\"AKIAIOSFODNN7EXAMPLE!\""), None);
        assert_eq!(
            token_prefix("'ghp_0123456789abcdefghijklmnopqrstuvwxyz'"),
            Some("ghp_")
        );
        assert_eq!(token_prefix("\"xoxb-123\""), None);
        assert_eq!(token_prefix("\"plain text\""), None);
    }
}
