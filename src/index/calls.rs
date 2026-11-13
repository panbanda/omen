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
            let name_node = callee.child_by_field_name(name)?;
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

    /// Describes a call node or a construction node.
    pub fn call<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Call<'tree>> {
        let kind = node.kind();
        let arity = |field: &str| {
            node.child_by_field_name(field)
                .map_or(0, |arguments| arguments.named_child_count())
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
                arity: arity(arguments),
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
            arity: arity(arguments),
        })
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
    pub arity: usize,
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
}

/// Splits the text of a type into its qualifier and its name. The function
/// removes pointer marks, reference marks, keywords, and generic arguments.
/// `*shapes.Circle` gives `shapes` and `Circle`.
pub fn split_type(text: &str) -> (Option<String>, String) {
    const KEYWORDS: &[&str] = &[
        "const", "mut", "dyn", "struct", "class", "enum", "final", "impl",
    ];
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
        assert_eq!(split("[]int"), "|");
    }
}
