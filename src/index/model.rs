//! Facts that the shared index holds for each file.

use serde::{Deserialize, Serialize};

use crate::core::Language;

/// A string that many facts share. A clone adds a count and copies no text.
///
/// The index has one key for each symbol, and each call site that names the
/// symbol holds a clone of that key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SharedStr(std::sync::Arc<str>);

impl SharedStr {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SharedStr {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SharedStr {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for SharedStr {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SharedStr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<&str> for SharedStr {
    fn from(text: &str) -> Self {
        Self(text.into())
    }
}

impl From<String> for SharedStr {
    fn from(text: String) -> Self {
        Self(text.into())
    }
}

impl From<&String> for SharedStr {
    fn from(text: &String) -> Self {
        Self(text.as_str().into())
    }
}

impl From<SharedStr> for String {
    fn from(text: SharedStr) -> Self {
        text.0.to_string()
    }
}

impl PartialEq<str> for SharedStr {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for SharedStr {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

impl PartialEq<String> for SharedStr {
    fn eq(&self, other: &String) -> bool {
        &*self.0 == other.as_str()
    }
}

impl PartialEq<SharedStr> for String {
    fn eq(&self, other: &SharedStr) -> bool {
        self.as_str() == &*other.0
    }
}

impl PartialEq<SharedStr> for &str {
    fn eq(&self, other: &SharedStr) -> bool {
        *self == &*other.0
    }
}

impl Serialize for SharedStr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SharedStr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

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

/// The relation of a definition to test code.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TestRole {
    /// The definition is not test code.
    #[default]
    None,
    /// The definition is in test code, and no test runner calls it by its syntax.
    Support,
    /// A test runner calls the definition: a test, a benchmark, a fixture, or a hook.
    Entry,
}

impl TestRole {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// `rel/path#Container.name`. See [`symbol_key`].
    pub key: SharedStr,
    pub name: String,
    pub kind: SymbolKind,
    /// The name of the nearest type or function that contains the definition.
    pub container: Option<String>,
    /// The type to which a method belongs when the method is not in the body of that type.
    pub receiver_type: Option<String>,
    pub supertypes: Vec<String>,
    /// The declaration, from its first modifier or keyword to its end. The
    /// annotations, attributes, decorators, and comments before the
    /// declaration are not in the range. `range.start_line` is the one start
    /// line of the symbol for each consumer.
    pub range: Span,
    pub name_range: Span,
    pub body_range: Option<Span>,
    pub signature: String,
    pub doc: Option<String>,
    pub exported: Exported,
    /// The SCIP symbol. The tree-sitter extraction does not supply it.
    pub scip_symbol: Option<String>,
    /// The relation of the definition to test code. See `crate::index::test_code`.
    #[serde(default, skip_serializing_if = "TestRole::is_none")]
    pub test_role: TestRole,
}

impl Symbol {
    /// The definition is test code: a test, or a definition that only test code can use.
    pub fn is_test(&self) -> bool {
        self.test_role != TestRole::None
    }

    /// The symbol is a function, a method, or a constructor.
    pub fn is_callable(&self) -> bool {
        matches!(
            self.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        )
    }

    /// The symbol is a type that a listing of a file shows with its methods
    /// and its fields: a class, a struct, an interface, a trait, an enum, or
    /// a Ruby module. A module of another language only groups definitions.
    pub fn is_class_like(&self, language: Language) -> bool {
        match self.kind {
            SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Interface
            | SymbolKind::Trait
            | SymbolKind::Enum => true,
            SymbolKind::Module => language == Language::Ruby,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// The import binds a module, a package, or a namespace to one name.
    Module,
    /// The import binds one name of a module.
    Named,
    /// The import makes all names of a module available without a qualifier.
    Wildcard,
    /// The import loads a file and binds no name, as `#include` and `source` do.
    Load,
    /// The statement declares a module that is in a different file, as Rust `mod name;` does.
    ModuleDeclaration,
    /// The statement exports one name of a different module.
    ReExportNamed,
    /// The statement exports all names of a different module.
    ReExportWildcard,
}

/// The file to which an import refers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportTarget {
    Resolved(String),
    /// The files of the one package or namespace that the import names, in
    /// path order. This target is as certain as `Resolved`.
    Package(Vec<String>),
    /// Two or more modules or packages match the import path, and the index
    /// cannot tell which one the import names. The list is in path order.
    Ambiguous(Vec<String>),
    /// The module is not a part of the repository.
    External,
    /// The module can be a part of the repository, but the index did not find its file.
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBinding {
    pub file: String,
    pub range: Span,
    /// The path in the source, without quotation marks.
    pub module_path: String,
    pub kind: ImportKind,
    /// The name in the module. `*` is the module itself and `default` is the default export.
    pub imported_name: Option<String>,
    /// The name that the import binds in this file.
    pub local_name: Option<String>,
    pub target_file: ImportTarget,
    /// The key of the symbol to which a named import refers.
    pub target_symbol: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Call,
    /// A call that makes an object of a type, as `new T()` does.
    Construct,
    /// A use of a function as a value, as the argument `worker` in
    /// `spawn(worker)` is. It is not a call: the function can run later.
    FunctionReference,
}

/// The arity of a reference that is not a call and thus has no arguments. No
/// definition has this number of parameters, thus the arity selects no overload.
pub const NO_ARITY: usize = usize::MAX;

/// The expression on which a call selects its callee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Receiver {
    /// `self`, `this`, or `$this`.
    SelfValue,
    /// `super`, `base`, or `parent`.
    Super,
    /// One name, for example a variable, a type, or a module.
    Name(String),
    /// An expression that is not one name.
    Expression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    /// An external index gave the definition. The tree-sitter index does not use this status.
    Resolved,
    /// The rules found one definition, or the definitions of one Rust item
    /// under different `cfg` conditions.
    UniqueCandidate,
    /// The rules found more than one definition, or the receiver has no known type.
    Ambiguous,
    /// The rules found no definition, or a local value has the name.
    Unresolved,
    /// The name refers to a definition that is not in the repository.
    External,
}

/// The rule that gave the candidates of a resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionBasis {
    Scip,
    LexicalScope,
    SameFile,
    ImportedName,
    ImportedAlias,
    ImportedModule,
    EnclosingType,
    ReceiverDeclaredType,
    QualifiedPath,
    SamePackage,
    /// The candidates of a call through an interface: the method of the
    /// interface and the methods of the types that implement it. The
    /// syntax does not tell the type of the value, thus no candidate is an edge.
    Implementation,
    NameOnly,
}

impl ResolutionBasis {
    /// Each basis, from the weakest evidence to the strongest evidence.
    pub const RANKED: [Self; 12] = [
        Self::NameOnly,
        Self::Implementation,
        Self::SamePackage,
        Self::ImportedModule,
        Self::QualifiedPath,
        Self::ReceiverDeclaredType,
        Self::EnclosingType,
        Self::ImportedAlias,
        Self::ImportedName,
        Self::SameFile,
        Self::LexicalScope,
        Self::Scip,
    ];

    /// The position of the basis in `RANKED`.
    pub fn rank(self) -> usize {
        Self::RANKED
            .iter()
            .position(|basis| *basis == self)
            .unwrap_or(0)
    }

    /// The name of the basis in the output, for example `name_only`.
    pub fn name(self) -> &'static str {
        match self {
            Self::NameOnly => "name_only",
            Self::Implementation => "implementation",
            Self::SamePackage => "same_package",
            Self::ImportedModule => "imported_module",
            Self::QualifiedPath => "qualified_path",
            Self::ReceiverDeclaredType => "receiver_declared_type",
            Self::EnclosingType => "enclosing_type",
            Self::ImportedAlias => "imported_alias",
            Self::ImportedName => "imported_name",
            Self::SameFile => "same_file",
            Self::LexicalScope => "lexical_scope",
            Self::Scip => "scip",
        }
    }

    /// The basis that has this name in the output.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::RANKED.into_iter().find(|basis| basis.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub status: ResolutionStatus,
    /// `None` when no rule applied.
    pub basis: Option<ResolutionBasis>,
    /// The keys of the candidate symbols, in key order. An edge exists only
    /// for the status `Resolved` or `UniqueCandidate`. With one of these
    /// statuses, each candidate has an edge.
    pub candidates: Vec<SharedStr>,
    /// The keys of the definitions that a later definition with the same
    /// name replaces, in key order. They are not targets of an edge. Only a
    /// language that permits a second definition of a name, as Bash does,
    /// gives them.
    pub superseded: Vec<SharedStr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub file: SharedStr,
    pub name_range: Span,
    pub name: String,
    pub kind: ReferenceKind,
    /// The key of the nearest definition that contains the reference. A call
    /// in a callback thus has the function that contains the callback.
    pub enclosing: Option<SharedStr>,
    /// The reference is in a function value that has no name.
    pub via_closure: bool,
    pub receiver: Option<Receiver>,
    /// The path before the name, for example `a::b` in `a::b::run()`.
    pub qualifier: Option<String>,
    pub arity: usize,
    pub resolution: Resolution,
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
    fn test_shared_string_clone_shares_the_text() {
        let key = SharedStr::from("a.rs#run");
        let clone = key.clone();
        assert_eq!(key.as_ptr(), clone.as_ptr());
        assert_eq!(key, "a.rs#run");
        assert_eq!(key, "a.rs#run".to_string());
        assert_eq!(key.len(), 8);
    }

    #[test]
    fn test_shared_string_is_a_plain_string_in_json() {
        let key = SharedStr::from("a.rs#run");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"a.rs#run\"");
        let back: SharedStr = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
    }

    #[test]
    fn test_reference_holds_no_more_than_pointers_for_keys() {
        assert!(std::mem::size_of::<SharedStr>() <= 16);
    }

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
