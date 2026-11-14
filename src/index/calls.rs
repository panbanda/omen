//! Call sites and local bindings. A [`CallSyntax`] table tells the tree walk
//! how a language writes a call, a receiver, and a declaration of a local name.

use tree_sitter::Node;

use super::extract::Context;
use super::model::{Receiver, ReferenceKind};

/// The call syntax of a language. Each tuple names a node kind and its fields.
/// An empty field name means that the node has no such field.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallSyntax {
    /// `(call kind, callee field, arguments field)`. With an empty callee
    /// field, the call node is also the callee expression.
    pub calls: &'static [(&'static str, &'static str, &'static str)],
    /// `(kind, type field, arguments field)` for `new Type(...)`.
    pub constructions: &'static [(&'static str, &'static str, &'static str)],
    /// `(kind, receiver field, name field)` for `receiver.name`.
    pub members: &'static [(&'static str, &'static str, &'static str)],
    /// `(kind, qualifier field, name field)` for `qualifier::name`.
    pub paths: &'static [(&'static str, &'static str, &'static str)],
    /// The node kinds that are one name.
    pub names: &'static [&'static str],
    /// The node kinds and the texts of the object itself, for example `this`.
    pub self_words: &'static [&'static str],
    /// The node kinds and the texts of the parent type, for example `super`.
    pub super_words: &'static [&'static str],
    /// `(kind, name field, type field, value field)` for a declaration of a local name.
    pub locals: &'static [(&'static str, &'static str, &'static str, &'static str)],
    /// `(kind, name field, type field)` for a parameter.
    pub parameters: &'static [(&'static str, &'static str, &'static str)],
    /// The field of a definition node that has the type of the result.
    pub return_type_field: &'static str,
    /// `(literal kind, type name)`. The type name is the name that a
    /// parameter of exactly that type has in the source. The names `int`,
    /// `double`, and `number` also read the suffix of the literal.
    pub literals: &'static [(&'static str, &'static str)],
}

impl CallSyntax {
    /// Each node kind for which the walk reads a call.
    pub fn call_kinds(&self) -> Vec<&'static str> {
        let calls = self.calls.iter().map(|rule| rule.0);
        calls
            .chain(self.constructions.iter().map(|rule| rule.0))
            .collect()
    }

    /// Each node kind for which the walk reads a local name.
    pub fn local_kinds(&self) -> Vec<&'static str> {
        let locals = self.locals.iter().map(|rule| rule.0);
        locals
            .chain(self.parameters.iter().map(|rule| rule.0))
            .collect()
    }

    /// Each node kind in the table. The grammar lint examines these kinds.
    pub fn kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_kinds();
        kinds.extend(self.local_kinds());
        kinds.extend(self.members.iter().map(|rule| rule.0));
        kinds.extend(self.paths.iter().map(|rule| rule.0));
        kinds.extend(self.names);
        kinds.extend(self.literals.iter().map(|rule| rule.0));
        kinds
    }

    /// Each field name in the table, with the node kind that has the field.
    pub fn fields(&self) -> Vec<(&'static str, &'static str)> {
        let mut fields = Vec::new();
        let triples = [self.calls, self.constructions, self.members, self.paths];
        for (kind, first, second) in triples.into_iter().flatten().chain(self.parameters) {
            fields.push((*kind, *first));
            fields.push((*kind, *second));
        }
        for (kind, name, type_field, value) in self.locals {
            fields.extend([(*kind, *name), (*kind, *type_field), (*kind, *value)]);
        }
        fields.retain(|(_, field)| !field.is_empty());
        fields
    }

    fn is_word(words: &[&str], node: Node<'_>, context: &Context<'_, '_>) -> bool {
        words.contains(&node.kind()) || words.contains(&context.text(node).as_str())
    }

    /// The receiver or the qualifier that an expression before a name gives.
    fn before_name(
        &self,
        node: Node<'_>,
        context: &Context<'_, '_>,
    ) -> (Option<Receiver>, Option<String>) {
        if Self::is_word(self.self_words, node, context) {
            return (Some(Receiver::SelfValue), None);
        }
        if Self::is_word(self.super_words, node, context) {
            return (Some(Receiver::Super), None);
        }
        // `super().name()` in Python.
        let calls_super = self.calls.iter().any(|(kind, callee, _)| {
            node.kind() == *kind
                && node
                    .child_by_field_name(callee)
                    .is_some_and(|callee| Self::is_word(self.super_words, callee, context))
        });
        if calls_super {
            return (Some(Receiver::Super), None);
        }
        if self.names.contains(&node.kind()) {
            return (Some(Receiver::Name(context.text(node))), None);
        }
        if self.is_name_chain(node) {
            let text: String = context.text(node).split_whitespace().collect();
            return (None, Some(text));
        }
        (Some(Receiver::Expression), None)
    }

    /// Tells if an expression is only names with `.` or `::` between them.
    fn is_name_chain(&self, node: Node<'_>) -> bool {
        if self.names.contains(&node.kind()) {
            return true;
        }
        // A call node can also be a member rule. A call is not a name.
        if self.calls.iter().any(|rule| rule.0 == node.kind()) {
            return false;
        }
        let rule = self.members.iter().chain(self.paths);
        rule.filter(|(kind, _, _)| node.kind() == *kind)
            .any(|(_, before, name)| {
                let before = node.child_by_field_name(before);
                let name = node.child_by_field_name(name);
                before.is_some_and(|before| self.is_name_chain(before))
                    && name.is_some_and(|name| name.named_child_count() == 0)
            })
    }

    /// Describes the callee expression of a call.
    pub fn callee<'tree>(
        &self,
        callee: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Callee<'tree>> {
        let kind = callee.kind();
        if let Some((_, receiver, name)) = self.members.iter().find(|rule| rule.0 == kind) {
            let name_node = callee.child_by_field_name(name)?;
            let (receiver, qualifier) = match callee.child_by_field_name(receiver) {
                Some(receiver) => self.before_name(receiver, context),
                None => (None, None),
            };
            return Some(Callee {
                name_node,
                receiver,
                qualifier,
            });
        }
        if let Some((_, qualifier, name)) = self.paths.iter().find(|rule| rule.0 == kind) {
            let mut name_node = callee.child_by_field_name(name)?;
            // A C++ path nests to the right: `a::b::c` is `a` and the path `b::c`.
            let mut inner = Vec::new();
            while name_node.kind() == kind {
                let scope = name_node.child_by_field_name(qualifier)?;
                inner.push(context.text(scope));
                name_node = name_node.child_by_field_name(name)?;
            }
            let (receiver, qualifier) = match callee.child_by_field_name(qualifier) {
                Some(qualifier) => match self.before_name(qualifier, context) {
                    (Some(Receiver::Name(name)), _) => (None, Some(name)),
                    (Some(Receiver::Expression), _) => {
                        let text: String = context.text(qualifier).split_whitespace().collect();
                        (None, Some(text))
                    }
                    other => other,
                },
                None => (None, None),
            };
            let qualifier = match qualifier {
                Some(first) if !inner.is_empty() => Some(format!("{first}::{}", inner.join("::"))),
                other => other,
            };
            return Some(Callee {
                name_node,
                receiver,
                qualifier,
            });
        }
        if Self::is_word(self.super_words, callee, context) {
            return Some(Callee {
                name_node: callee,
                receiver: Some(Receiver::Super),
                qualifier: None,
            });
        }
        self.names.contains(&kind).then_some(Callee {
            name_node: callee,
            receiver: None,
            qualifier: None,
        })
    }

    /// The byte range of the receiver expression of a call node.
    pub fn receiver_range(&self, node: Node<'_>) -> Option<(usize, usize)> {
        let (_, callee, _) = self.calls.iter().find(|rule| rule.0 == node.kind())?;
        let callee = if callee.is_empty() {
            node
        } else {
            node.child_by_field_name(callee)?
        };
        let (_, receiver, _) = self.members.iter().find(|rule| rule.0 == callee.kind())?;
        let receiver = callee.child_by_field_name(receiver)?;
        Some((receiver.start_byte(), receiver.end_byte()))
    }

    /// Describes a call node or a construction node.
    pub fn call<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Call<'tree>> {
        let kind = node.kind();
        let read_arguments = |field: &str| -> Vec<Argument> {
            let Some(arguments) = node.child_by_field_name(field) else {
                return Vec::new();
            };
            let mut cursor = arguments.walk();
            arguments
                .named_children(&mut cursor)
                .map(|argument| self.argument(argument, context))
                .collect()
        };
        if let Some((_, callee, arguments)) = self.calls.iter().find(|rule| rule.0 == kind) {
            let callee = if callee.is_empty() {
                node
            } else {
                node.child_by_field_name(callee)?
            };
            return Some(Call {
                callee: self.callee(callee, context)?,
                kind: ReferenceKind::Call,
                arguments: read_arguments(arguments),
            });
        }
        let (_, type_field, arguments) = self.constructions.iter().find(|rule| rule.0 == kind)?;
        let type_node = node.child_by_field_name(type_field)?;
        let (qualifier, _) = split_type(&context.text(type_node));
        Some(Call {
            callee: Callee {
                name_node: type_node,
                receiver: None,
                qualifier,
            },
            kind: ReferenceKind::Construct,
            arguments: read_arguments(arguments),
        })
    }

    /// The fact that the syntax gives about the type of one argument.
    fn argument(&self, node: Node<'_>, context: &Context<'_, '_>) -> Argument {
        // Some grammars put each argument in a node of its own.
        let node = match node.kind() {
            "argument" => node.named_child(0).unwrap_or(node),
            _ => node,
        };
        if self.names.contains(&node.kind()) {
            return Argument::Name(context.text(node));
        }
        self.literals
            .iter()
            .find(|(kind, _)| node.kind() == *kind)
            .and_then(|(_, base)| literal_type(base, &context.text(node)))
            .map_or(Argument::Other, Argument::Literal)
    }

    /// Adds the local names that a declaration node or a parameter node binds.
    pub fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        let kind = node.kind();
        let local = self.locals.iter().find(|rule| rule.0 == kind).copied();
        let parameter = self.parameters.iter().find(|rule| rule.0 == kind);
        let (name_field, type_field, value_field, is_parameter) = match (local, parameter) {
            (Some((_, name, type_field, value)), _) => (name, type_field, value, false),
            (None, Some((_, name, type_field))) => (*name, *type_field, "", true),
            (None, None) => return,
        };
        let field = |field: &str| {
            (!field.is_empty())
                .then(|| node.child_by_field_name(field))
                .flatten()
        };
        let type_name = field(type_field).map(|type_node| context.text(type_node));
        let value_start = field(value_field).map(|value| value.start_byte());
        let mut cursor = node.walk();
        for name in node.children_by_field_name(name_field, &mut cursor) {
            if self.names.contains(&name.kind()) {
                out.push(LocalName {
                    name: context.text(name),
                    type_name: type_name.clone(),
                    value_start,
                    is_parameter,
                    element: false,
                    result_index: None,
                    visible: None,
                });
            }
        }
    }
}

/// The callee expression of a call.
#[derive(Debug, Clone)]
pub struct Callee<'tree> {
    pub name_node: Node<'tree>,
    pub receiver: Option<Receiver>,
    pub qualifier: Option<String>,
}

/// A call site before the walk gives it an enclosing definition.
#[derive(Debug, Clone)]
pub struct Call<'tree> {
    pub callee: Callee<'tree>,
    pub kind: ReferenceKind,
    pub arguments: Vec<Argument>,
}

/// What the syntax tells about the type of an argument of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// A literal. The text is the name of its type.
    Literal(String),
    /// One name, for example a local variable.
    Name(String),
    Other,
}

/// The type name of a number literal with its suffix, or of a different
/// literal. The result is `None` for a suffix that this function does not know.
fn literal_type(base: &str, text: &str) -> Option<String> {
    let hexadecimal = text.starts_with("0x") || text.starts_with("0X");
    let base = match base {
        "number" if !hexadecimal && text.contains(['.', 'e', 'E']) => "double",
        "number" => "int",
        other => other,
    };
    let suffix: String = text
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect();
    let name = match (base, suffix.to_ascii_lowercase().as_str()) {
        ("int" | "double", _) if hexadecimal => "int",
        ("int", "") => "int",
        ("int", "l") => "long",
        ("double", "" | "d") => "double",
        ("double", "f") => "float",
        ("double", "m") => "decimal",
        ("int" | "double", _) => return None,
        (other, _) => other,
    };
    Some(name.to_string())
}

/// A name that a function binds: a parameter or a local variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalName {
    pub name: String,
    /// The declared type, as the source has it.
    pub type_name: Option<String>,
    /// The start byte of the expression that gives the first value.
    pub value_start: Option<usize>,
    pub is_parameter: bool,
    /// The name is one element of the value, as a loop variable is.
    pub element: bool,
    /// The position of the name in a list of names that take the results of
    /// one call, as `a, b := f()` has.
    pub result_index: Option<usize>,
    /// The bytes where the name is visible: from its declaration to the end
    /// of the enclosing block. Only a C++ namespace alias has this range.
    pub visible: Option<std::ops::Range<usize>>,
}

/// The type without the marks that make a value optional: a `:` before a
/// type annotation, PHP `?T`, C# `T?`, and a union with `null`, `undefined`,
/// or `None`.
pub fn required_type(text: &str) -> &str {
    let text = text.trim().trim_start_matches(':').trim();
    // A Python type can be in a string, and `Optional[T]` is `T` or `None`.
    let text = text.trim_matches(|quote| quote == '"' || quote == '\'');
    let text = ["Optional[", "typing.Optional["]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix)?.strip_suffix(']'))
        .unwrap_or(text);
    let text = text.strip_prefix('?').unwrap_or(text);
    let text = text.strip_suffix('?').unwrap_or(text);
    let mut parts = text
        .split('|')
        .map(str::trim)
        .filter(|part| !["null", "undefined", "None"].contains(part));
    match (parts.next(), parts.next()) {
        (Some(only), None) => only,
        _ => text,
    }
}

/// Splits a list at each comma that is not in brackets.
pub fn split_list(list: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (position, character) in list.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(list[start..position].trim());
                start = position + 1;
            }
            _ => {}
        }
    }
    parts.push(list[start..].trim());
    parts
}

/// The element type of a slice, an array, or a list: Go `[]T` and `[4]T`,
/// Rust `[T]` and `Vec<T>`, `T[]`, and the list types of the standard
/// libraries. `None` for a different type.
pub fn element_type(text: &str) -> Option<&str> {
    const LISTS: &[&str] = &[
        "Array",
        "ReadonlyArray",
        "List",
        "ArrayList",
        "LinkedList",
        "IList",
        "IReadOnlyList",
        "IEnumerable",
        "Iterable",
        "Collection",
        "Set",
        "Vec",
        "VecDeque",
        "vector",
        "std::vector",
        "list",
        "Sequence",
    ];
    let text = text.trim();
    if let Some(variadic) = text.strip_prefix("...") {
        return Some(variadic.trim());
    }
    // A Rust reference can have a lifetime and `mut` before the slice.
    let mut bare = text.trim_start_matches('&').trim_start();
    if bare.starts_with('\'') {
        bare = bare
            .split_once(' ')
            .map_or(bare, |(_, rest)| rest.trim_start());
    }
    let bare = bare.strip_prefix("mut ").map_or(bare, str::trim_start);
    if let Some(inside) = bare.strip_prefix('[') {
        let mut depth = 1usize;
        let close = inside.char_indices().find_map(|(position, character)| {
            match character {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(position)
        })?;
        let after = inside[close + 1..].trim();
        return if after.is_empty() {
            inside[..close].split(';').next().map(str::trim)
        } else {
            Some(after)
        };
    }
    if let Some(items) = text.strip_suffix("[]") {
        return Some(items.trim());
    }
    for (open, close) in [('<', '>'), ('[', ']')] {
        let Some((name, arguments)) = text.split_once(open) else {
            continue;
        };
        if !LISTS.contains(&name.trim()) {
            continue;
        }
        let arguments = arguments.trim().strip_suffix(close)?;
        return match split_list(arguments).as_slice() {
            [only] => Some(*only),
            _ => None,
        };
    }
    None
}

/// One result of a declared result. A list in parentheses has two or more
/// results, and `index` selects one of them. Without an index, such a list
/// gives `None`. A Go result can have a name before its type.
pub fn one_result(text: &str, index: Option<usize>) -> Option<&str> {
    let text = text.trim();
    let list = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(list) = list else {
        return index.is_none().then_some(text);
    };
    let parts = split_list(list);
    let part = match (index, parts.as_slice()) {
        (Some(index), parts) => *parts.get(index)?,
        (None, [only]) => *only,
        (None, _) => return None,
    };
    let is_name = |name: &str| {
        name.chars()
            .all(|character| character.is_alphanumeric() || character == '_')
    };
    match part.split_once(' ') {
        Some((name, declared)) if is_name(name) && !declared.trim().is_empty() => {
            Some(declared.trim())
        }
        _ => Some(part),
    }
}

/// Splits the text of a type into its qualifier and its name. The function
/// removes pointer marks, reference marks, keywords, and generic arguments.
/// `*shapes.Circle` gives `shapes` and `Circle`.
pub fn split_type(text: &str) -> (Option<String>, String) {
    const KEYWORDS: &[&str] = &[
        "const", "mut", "dyn", "struct", "class", "enum", "final", "impl",
    ];
    // A Go slice or array type `[]T` has the element type `T` after the brackets.
    let text = text.trim_start();
    let text = match text.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or(text, |(_, element)| element),
        None => text,
    };
    let text = text.split(['<', '[', '(']).next().unwrap_or(text);
    let words: Vec<&str> = text
        .split(|character: char| character.is_whitespace() || "*&?!".contains(character))
        .filter(|word| !word.is_empty() && !KEYWORDS.contains(word))
        .collect();
    let Some(path) = words.last() else {
        return (None, String::new());
    };
    let path = path.trim_start_matches(['\\', ':', '.', '$']);
    let separator = ["::", "\\", "."]
        .into_iter()
        .filter_map(|separator| path.rfind(separator).map(|at| (at, separator.len())))
        .max();
    match separator {
        Some((at, length)) => (
            Some(path[..at].to_string()),
            path[at + length..].to_string(),
        ),
        None => (None, path.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_type() {
        let split = |text| {
            let (qualifier, name) = split_type(text);
            format!("{}|{name}", qualifier.unwrap_or_default())
        };
        assert_eq!(split("Circle"), "|Circle");
        assert_eq!(split("*shapes.Circle"), "shapes|Circle");
        assert_eq!(split("&mut Vec<u32>"), "|Vec");
        assert_eq!(split("&dyn Shape"), "|Shape");
        assert_eq!(split("const Circle&"), "|Circle");
        assert_eq!(split("std::vector<int>"), "std|vector");
        assert_eq!(split("\\Shop\\Shapes\\Circle"), "Shop\\Shapes|Circle");
        assert_eq!(split("?Circle"), "|Circle");
        assert_eq!(split("[]int"), "|int");
        assert_eq!(split("[]*Command"), "|Command");
    }

    #[test]
    fn test_literal_type_reads_the_suffix() {
        let name = |base, text| literal_type(base, text).unwrap_or_else(|| "-".to_string());
        assert_eq!(name("int", "2"), "int");
        assert_eq!(name("int", "2L"), "long");
        assert_eq!(name("int", "2u"), "-");
        assert_eq!(name("int", "0xFF"), "int");
        assert_eq!(name("double", "2.0"), "double");
        assert_eq!(name("double", "2.0f"), "float");
        assert_eq!(name("double", "2.0m"), "decimal");
        assert_eq!(name("number", "2"), "int");
        assert_eq!(name("number", "2.5"), "double");
        assert_eq!(name("number", "1e3"), "double");
        assert_eq!(name("number", "2.5f"), "float");
        assert_eq!(name("String", "\"a\""), "String");
    }
}
