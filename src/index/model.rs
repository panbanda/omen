//! Facts that the shared index holds for each file.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Constructor,
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Module,
    TypeAlias,
    Field,
    Constant,
    Variable,
    Macro,
}

impl SymbolKind {
    /// A type-like kind can contain members.
    pub fn is_type(self) -> bool {
        matches!(
            self,
            Self::Class
                | Self::Struct
                | Self::Interface
                | Self::Trait
                | Self::Enum
                | Self::Module
                | Self::TypeAlias
        )
    }
}

/// Tells if the syntax makes a definition a part of the public interface of its package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exported {
    Exported,
    NotExported,
    /// The syntax of this file does not give the answer. The text is the reason.
    Unknown(String),
}

/// A source range. Lines start at 1. Columns and bytes start at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl Span {
    pub fn of(node: &tree_sitter::Node<'_>) -> Self {
        Self::between(node, node)
    }

    /// The range from the start of `first` to the end of `last`.
    pub fn between(first: &tree_sitter::Node<'_>, last: &tree_sitter::Node<'_>) -> Self {
        let start = first.start_position();
        let end = last.end_position();
        Self {
            start_byte: first.start_byte(),
            end_byte: last.end_byte(),
            start_line: start.row as u32 + 1,
            start_column: start.column as u32,
            end_line: end.row as u32 + 1,
            end_column: end.column as u32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// `rel/path#Container.name`. See [`symbol_key`].
    pub key: String,
    pub name: String,
    pub kind: SymbolKind,
    /// The name of the nearest type or function that contains the definition.
    pub container: Option<String>,
    /// The type to which a method belongs when the method is not in the body of that type.
    pub receiver_type: Option<String>,
    pub supertypes: Vec<String>,
    pub range: Span,
    pub name_range: Span,
    pub body_range: Option<Span>,
    pub signature: String,
    pub doc: Option<String>,
    pub exported: Exported,
    /// The SCIP symbol. The tree-sitter extraction does not supply it.
    pub scip_symbol: Option<String>,
}

/// Renders the key of a symbol.
///
/// `ordinal` is the position of the symbol among the symbols of one file that
/// have the same container path and name, in source order. The first symbol
/// has the ordinal 1 and no suffix. The second symbol has the suffix `~2`.
///
/// The separator is `#` because `:` occurs in `Type::method` and in Windows paths.
pub fn symbol_key(file: &str, container_path: &[String], name: &str, ordinal: usize) -> String {
    let mut key = format!("{file}#");
    for container in container_path {
        key.push_str(container);
        key.push('.');
    }
    key.push_str(name);
    if ordinal > 1 {
        key.push_str(&format!("~{ordinal}"));
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_key_without_container() {
        assert_eq!(symbol_key("src/a.rs", &[], "run", 1), "src/a.rs#run");
    }

    #[test]
    fn test_symbol_key_with_container_path() {
        let path = vec!["Outer".to_string(), "Inner".to_string()];
        assert_eq!(
            symbol_key("src/a.rs", &path, "run", 1),
            "src/a.rs#Outer.Inner.run"
        );
    }

    #[test]
    fn test_symbol_key_marks_only_later_duplicates() {
        let path = vec!["Circle".to_string()];
        assert_eq!(symbol_key("a.rs", &path, "area", 1), "a.rs#Circle.area");
        assert_eq!(symbol_key("a.rs", &path, "area", 2), "a.rs#Circle.area~2");
        assert_eq!(symbol_key("a.rs", &path, "area", 3), "a.rs#Circle.area~3");
    }
}
