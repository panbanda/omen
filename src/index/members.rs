//! Facts of the members of a class. The class metrics use them.
//!
//! The extraction records the decision points, the declared fields, the uses
//! of a field through the object itself, and the references to types. The
//! analyzer of the metrics reads these facts and parses no source.
//!
//! A language that has no class has [`ClassModel::NotApplicable`] and a reason.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::extract::{Local, Scope, ScopeRole};
use super::model::{SharedStr, SymbolKind};
use crate::core::Language;
use crate::parser::queries::{get_decision_node_types, get_nested_scope_node_types, get_operator};

/// The syntax of the classes of one language.
#[derive(Debug, Clone, Copy)]
pub struct ClassSyntax {
    /// The kinds of the symbols that are classes for the metrics.
    pub classes: &'static [SymbolKind],
    /// The node kinds that can name a type.
    pub references: &'static [&'static str],
    /// The node kinds that declare a field.
    pub declarations: &'static [&'static str],
    /// The node kinds that read or write a field of the object itself.
    pub accesses: &'static [&'static str],
    /// The node kinds that can name a field of the object without `this`.
    pub implicit: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub enum ClassModel {
    /// The language has no class. The text is the reason.
    NotApplicable(&'static str),
    Classes(ClassSyntax),
}

/// The class model of a language.
pub fn class_model(language: Language) -> ClassModel {
    use SymbolKind::{Class, Enum, Interface, Module, Struct, Trait};
    match language {
        Language::Java => ClassModel::Classes(ClassSyntax {
            classes: &[Class, Interface, Enum],
            references: &["type_identifier"],
            declarations: &["field_declaration"],
            accesses: &["field_access"],
            implicit: &["identifier"],
        }),
        Language::CSharp => ClassModel::Classes(ClassSyntax {
            classes: &[Class, Interface, Struct],
            references: &["identifier"],
            declarations: &["field_declaration", "property_declaration", "parameter"],
            accesses: &["member_access_expression"],
            implicit: &["identifier"],
        }),
        Language::Cpp => ClassModel::Classes(ClassSyntax {
            classes: &[Class, Struct],
            references: &["type_identifier"],
            declarations: &["field_declaration"],
            accesses: &["field_expression"],
            implicit: &["identifier"],
        }),
        Language::TypeScript | Language::Tsx => ClassModel::Classes(ClassSyntax {
            classes: &[Class],
            references: &["type_identifier", "identifier"],
            declarations: &["public_field_definition"],
            accesses: &["member_expression"],
            implicit: &[],
        }),
        Language::JavaScript | Language::Jsx => ClassModel::Classes(ClassSyntax {
            classes: &[Class],
            references: &["identifier"],
            declarations: &["field_definition"],
            accesses: &["member_expression"],
            implicit: &[],
        }),
        Language::Python => ClassModel::Classes(ClassSyntax {
            classes: &[Class],
            references: &["identifier"],
            declarations: &["assignment"],
            accesses: &["attribute"],
            implicit: &[],
        }),
        Language::Ruby => ClassModel::Classes(ClassSyntax {
            classes: &[Class, Module],
            references: &["constant", "scope_resolution"],
            declarations: &["instance_variable"],
            accesses: &["instance_variable"],
            implicit: &[],
        }),
        Language::Php => ClassModel::Classes(ClassSyntax {
            classes: &[Class, Interface, Trait, Enum],
            references: &["named_type"],
            declarations: &["property_declaration", "property_promotion_parameter"],
            accesses: &["member_access_expression"],
            implicit: &[],
        }),
        Language::Rust => ClassModel::Classes(ClassSyntax {
            classes: &[Struct, Enum, Trait],
            references: &["type_identifier", "scoped_identifier"],
            declarations: &["field_declaration"],
            accesses: &["field_expression"],
            implicit: &[],
        }),
        Language::Go => ClassModel::Classes(ClassSyntax {
            classes: &[Struct],
            references: &["type_identifier", "qualified_type"],
            declarations: &["field_declaration"],
            accesses: &["selector_expression"],
            implicit: &[],
        }),
        Language::C => ClassModel::NotApplicable("C has structs but no methods, so no class"),
        Language::Bash => ClassModel::NotApplicable("Bash has no type that has methods"),
    }
}

/// Every node kind that a class model names, for the grammar lint. The list
/// of decision kinds is one list for all languages, and a grammar does not
/// need to have each kind. Thus the lint does not examine it.
pub fn syntax_kinds(language: Language) -> Vec<(&'static str, Vec<&'static str>)> {
    match class_model(language) {
        ClassModel::NotApplicable(_) => Vec::new(),
        ClassModel::Classes(syntax) => vec![
            ("member_reference", syntax.references.to_vec()),
            ("member_declaration", syntax.declarations.to_vec()),
            ("member_access", syntax.accesses.to_vec()),
            ("member_implicit", syntax.implicit.to_vec()),
        ],
    }
}

pub const DECISION: u8 = 1;
pub const REFERENCE: u8 = 2;
pub const DECLARATION: u8 = 4;
pub const ACCESS: u8 = 8;
pub const IMPLICIT: u8 = 16;
pub const BOUNDARY: u8 = 32;

/// The kinds of the expressions whose operator can add a decision point.
const LOGICAL_KINDS: &[&str] = &["binary_expression", "logical_expression"];

/// The roles of a node kind, as a set of bits.
pub fn kind_bits(language: Language, syntax: &ClassSyntax, kind: &str) -> u8 {
    let mut bits = 0;
    if get_decision_node_types(language).contains(&kind) || LOGICAL_KINDS.contains(&kind) {
        bits |= DECISION;
    }
    if get_nested_scope_node_types(language).contains(&kind) {
        bits |= BOUNDARY;
    }
    if syntax.references.contains(&kind) {
        bits |= REFERENCE;
    }
    if syntax.declarations.contains(&kind) {
        bits |= DECLARATION;
    }
    if syntax.accesses.contains(&kind) {
        bits |= ACCESS;
    }
    if syntax.implicit.contains(&kind) {
        bits |= IMPLICIT;
    }
    bits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UseKind {
    Reference,
    Declaration,
    Access,
    Implicit,
}

/// A name in the source, before the finish step gives it to a symbol.
#[derive(Debug, Clone, Copy)]
struct RawUse {
    kind: UseKind,
    /// The node ID of the nearest scope that contains the name.
    scope: usize,
    start: usize,
    end: usize,
}

/// A name that a symbol uses or declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberUse {
    /// The position in `FileSymbols::symbols` of the nearest definition that
    /// contains the name. For the name in an `impl` block that is not in a
    /// method, the position is the position of the type.
    pub owner: u32,
    pub name: SharedStr,
}

/// A block that adds members to a type but does not define a symbol, as a Rust `impl` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeBlock {
    /// The position in `FileSymbols::symbols` of the type.
    pub class: u32,
    pub end_line: u32,
}

/// The facts of the members of the classes of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberFacts {
    /// For each symbol, the count of the decision points in its range. A file
    /// of a language that has no class has an empty list.
    pub decision_points: Vec<u32>,
    /// The declared fields, in source order.
    pub declarations: Vec<MemberUse>,
    /// The fields that a definition reads or writes through the object itself.
    pub accesses: Vec<MemberUse>,
    /// The names of types, as the syntax shows them.
    pub references: Vec<MemberUse>,
    pub blocks: Vec<TypeBlock>,
}

/// A block of the file that adds members to a type. The finish step needs its path.
#[derive(Debug, Clone)]
pub struct PendingBlock {
    pub node: usize,
    pub path: Vec<String>,
    pub end_line: u32,
}

/// Records the nodes that give member facts while the extraction walks the tree.
pub struct Recorder {
    language: Language,
    pub syntax: ClassSyntax,
    decisions: Vec<usize>,
    boundaries: Vec<(usize, usize)>,
    uses: Vec<RawUse>,
    pub blocks: Vec<PendingBlock>,
}

fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn range_of(node: Node<'_>) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
}

impl Recorder {
    /// A node of a decision kind adds a decision point. A logical expression adds one when its operator is a logical operator.
    fn adds_decision(&self, node: Node<'_>, source: &[u8]) -> bool {
        if !LOGICAL_KINDS.contains(&node.kind()) {
            return true;
        }
        matches!(
            get_operator(&node, source),
            Some("&&" | "||" | "and" | "or")
        )
    }

    pub fn new(language: Language) -> Option<Self> {
        match class_model(language) {
            ClassModel::NotApplicable(_) => None,
            ClassModel::Classes(syntax) => Some(Self {
                language,
                syntax,
                decisions: Vec::new(),
                boundaries: Vec::new(),
                uses: Vec::new(),
                blocks: Vec::new(),
            }),
        }
    }

    /// Records the node. `bits` are the roles of the kind of the node.
    pub fn visit(
        &mut self,
        node: Node<'_>,
        bits: u8,
        source: &[u8],
        scopes: &[Scope<'_>],
        ancestors: &[Node<'_>],
    ) {
        if bits & DECISION != 0 && self.adds_decision(node, source) {
            self.decisions.push(node.start_byte());
        }
        if bits & BOUNDARY != 0 {
            self.boundaries.push(range_of(node));
        }
        let Some(scope) = scopes
            .iter()
            .rev()
            .find(|scope| scope.role != ScopeRole::Namespace)
        else {
            return;
        };
        let scope_id = scope.node.id();
        let mut push = |kind: UseKind, (start, end): (usize, usize)| {
            self.uses.push(RawUse {
                kind,
                scope: scope_id,
                start,
                end,
            });
        };
        if bits & REFERENCE != 0 && is_reference(self.language, node, ancestors) {
            push(UseKind::Reference, range_of(node));
        }
        if bits & DECLARATION != 0 {
            for range in declared_names(self.language, node, source, ancestors) {
                push(UseKind::Declaration, range);
            }
        }
        if bits & ACCESS != 0 {
            if let Some(range) = self_access(self.language, node, source, scopes) {
                push(UseKind::Access, range);
            }
        }
        if bits & IMPLICIT != 0
            && scope.role == ScopeRole::Function
            && is_implicit_candidate(self.language, node, ancestors.last().copied())
        {
            push(UseKind::Implicit, range_of(node));
        }
    }
}

/// The symbols of a file, as the finish step needs them.
pub struct SymbolRow<'a> {
    pub node: usize,
    pub kind: SymbolKind,
    pub name: &'a str,
    pub container_path: &'a [String],
    pub start_byte: usize,
    pub end_byte: usize,
    /// The bytes of the body, when the definition has a body field.
    pub body: Option<(usize, usize)>,
    pub has_receiver: bool,
}

fn interned<'a>(pool: &mut HashMap<&'a str, SharedStr>, name: &'a str) -> SharedStr {
    pool.entry(name)
        .or_insert_with(|| SharedStr::from(name))
        .clone()
}

impl Recorder {
    /// Gives each recorded name to a symbol.
    pub fn finish(
        self,
        source: &[u8],
        symbols: &[SymbolRow<'_>],
        locals: &[Local],
        positions: &HashMap<usize, usize>,
    ) -> MemberFacts {
        let language = self.language;
        let mut decisions = self.decisions;
        decisions.sort_unstable();
        let mut boundaries = self.boundaries;
        boundaries.sort_unstable_by_key(|(from, to)| (*from, std::cmp::Reverse(*to)));
        let decision_points: Vec<u32> = symbols
            .iter()
            .map(|symbol| {
                let callable = matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                );
                if !callable {
                    return 0;
                }
                let (start, end) = symbol.body.unwrap_or((symbol.start_byte, symbol.end_byte));
                let in_range = |from: usize, to: usize| {
                    let below = decisions.partition_point(|at| *at < from);
                    let above = decisions.partition_point(|at| *at < to);
                    above - below
                };
                // A nested function has its own decision points. The body
                // that is itself a function is a function value, and the
                // function that holds it has none.
                let first = boundaries.partition_point(|(from, _)| *from < start);
                let mut count = in_range(start, end);
                let mut next_free = start;
                for (from, to) in &boundaries[first..] {
                    if *from >= end {
                        break;
                    }
                    if *from < next_free || *to > end {
                        continue;
                    }
                    if (*from, *to) == (start, end) {
                        if symbol.body.is_some() {
                            return 0;
                        }
                        continue;
                    }
                    count -= in_range(*from, *to);
                    next_free = *to;
                }
                count as u32
            })
            .collect();

        let mut block_owner: HashMap<usize, u32> = HashMap::new();
        let mut blocks = Vec::new();
        for block in &self.blocks {
            let class = symbols.iter().position(|symbol| {
                self.syntax.classes.contains(&symbol.kind)
                    && symbol.container_path.len() + 1 == block.path.len()
                    && symbol.container_path == &block.path[..block.path.len() - 1]
                    && symbol.name == block.path[block.path.len() - 1]
            });
            if let Some(class) = class {
                block_owner.insert(block.node, class as u32);
                blocks.push(TypeBlock {
                    class: class as u32,
                    end_line: block.end_line,
                });
            }
        }
        let owner_of = |scope: usize| -> Option<u32> {
            positions
                .get(&scope)
                .map(|position| *position as u32)
                .or_else(|| block_owner.get(&scope).copied())
        };

        let declared_in_file: HashSet<&str> = self
            .uses
            .iter()
            .filter(|raw| raw.kind == UseKind::Declaration)
            .filter_map(|raw| std::str::from_utf8(&source[raw.start..raw.end]).ok())
            .map(|name| strip_sigil(language, name))
            .collect();

        let mut pool: HashMap<&str, SharedStr> = HashMap::new();
        let mut seen: HashSet<(u8, u32, &str)> = HashSet::new();
        let mut facts = MemberFacts {
            decision_points,
            blocks,
            ..MemberFacts::default()
        };
        for raw in &self.uses {
            let Some(owner) = owner_of(raw.scope) else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(&source[raw.start..raw.end]) else {
                continue;
            };
            let name = strip_sigil(language, name);
            let (slot, kind) = match raw.kind {
                UseKind::Reference => {
                    if !is_valid_class_reference(name, language) {
                        continue;
                    }
                    (0u8, UseKind::Reference)
                }
                UseKind::Declaration => (1, UseKind::Declaration),
                UseKind::Access => (2, UseKind::Access),
                UseKind::Implicit => {
                    let symbol = &symbols[owner as usize];
                    let out_of_line = symbol.has_receiver;
                    let local = locals.iter().any(|local| {
                        local.owner == Some(owner as usize) && local.name.name == name
                    });
                    if local || !(out_of_line || declared_in_file.contains(name)) {
                        continue;
                    }
                    (2, UseKind::Access)
                }
            };
            if !seen.insert((slot, owner, name)) {
                continue;
            }
            let member = MemberUse {
                owner,
                name: interned(&mut pool, name),
            };
            match kind {
                UseKind::Reference => facts.references.push(member),
                UseKind::Declaration => facts.declarations.push(member),
                UseKind::Access | UseKind::Implicit => facts.accesses.push(member),
            }
        }
        facts
    }
}

fn strip_sigil(language: Language, name: &str) -> &str {
    if language == Language::Php {
        name.trim_start_matches('$')
    } else {
        name
    }
}

/// The ranges of the names that a node declares as fields.
fn declared_names(
    language: Language,
    node: Node<'_>,
    source: &[u8],
    ancestors: &[Node<'_>],
) -> Vec<(usize, usize)> {
    let mut names = Vec::new();
    match language {
        Language::Java => {
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                if let Some(name) = declarator.child_by_field_name("name") {
                    names.push(range_of(name));
                }
            }
        }
        Language::CSharp => {
            if node.kind() == "parameter" {
                // A positional parameter of a record is a property of the record.
                let in_record = ancestors
                    .iter()
                    .rev()
                    .nth(1)
                    .is_some_and(|owner| owner.kind() == "record_declaration");
                if in_record {
                    names.extend(node.child_by_field_name("name").map(range_of));
                }
            } else if node.kind() == "property_declaration" {
                names.extend(node.child_by_field_name("name").map(range_of));
            } else {
                let mut cursor = node.walk();
                for declaration in node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "variable_declaration")
                {
                    let mut inner = declaration.walk();
                    for declarator in declaration
                        .named_children(&mut inner)
                        .filter(|child| child.kind() == "variable_declarator")
                    {
                        let name = declarator
                            .child_by_field_name("name")
                            .or_else(|| declarator.named_child(0));
                        names.extend(name.map(range_of));
                    }
                }
            }
        }
        Language::Cpp => {
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                let mut current = declarator;
                loop {
                    match current.kind() {
                        "field_identifier" => {
                            names.push(range_of(current));
                            break;
                        }
                        "pointer_declarator"
                        | "reference_declarator"
                        | "array_declarator"
                        | "parenthesized_declarator"
                        | "init_declarator" => {
                            let inner = current
                                .child_by_field_name("declarator")
                                .or_else(|| current.named_child(0));
                            match inner {
                                Some(inner) => current = inner,
                                None => break,
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        Language::TypeScript | Language::Tsx => {
            names.extend(node.child_by_field_name("name").map(range_of));
        }
        Language::JavaScript | Language::Jsx => {
            names.extend(node.child_by_field_name("property").map(range_of));
        }
        Language::Python => {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "attribute" {
                    let object = left.child_by_field_name("object");
                    if object.is_some_and(|object| text(object, source) == "self") {
                        names.extend(left.child_by_field_name("attribute").map(range_of));
                    }
                }
            }
        }
        Language::Ruby => names.push(range_of(node)),
        Language::Php => {
            if node.kind() == "property_promotion_parameter" {
                names.extend(
                    node.child_by_field_name("name")
                        .or_else(|| first_child_of_kind(node, "variable_name"))
                        .map(range_of),
                );
            } else {
                let mut cursor = node.walk();
                for element in node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "property_element")
                {
                    names.extend(
                        element
                            .child_by_field_name("name")
                            .or_else(|| first_child_of_kind(element, "variable_name"))
                            .map(range_of),
                    );
                }
            }
        }
        Language::Rust => names.extend(node.child_by_field_name("name").map(range_of)),
        Language::Go => {
            let mut cursor = node.walk();
            for name in node.children_by_field_name("name", &mut cursor) {
                names.push(range_of(name));
            }
        }
        Language::C | Language::Bash => {}
    }
    names
}

fn first_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == kind);
    found
}

/// The range of the field that a node reads or writes through the object itself.
fn self_access(
    language: Language,
    node: Node<'_>,
    source: &[u8],
    scopes: &[Scope<'_>],
) -> Option<(usize, usize)> {
    let through = |object: &str, name: &str, keyword: &str| -> Option<(usize, usize)> {
        let object = node.child_by_field_name(object)?;
        if text(object, source) != keyword {
            return None;
        }
        node.child_by_field_name(name).map(range_of)
    };
    match language {
        Language::Python => through("object", "attribute", "self"),
        Language::Ruby => Some(range_of(node)),
        Language::Java => through("object", "field", "this"),
        Language::CSharp => through("expression", "name", "this"),
        Language::Cpp => through("argument", "field", "this"),
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            through("object", "property", "this")
        }
        Language::Php => through("object", "name", "$this"),
        Language::Rust => through("value", "field", "self"),
        Language::Go => {
            let operand = node.child_by_field_name("operand")?;
            if operand.kind() != "identifier" {
                return None;
            }
            let receiver = go_receiver_name(scopes, source)?;
            if text(operand, source) != receiver {
                return None;
            }
            node.child_by_field_name("field").map(range_of)
        }
        Language::C | Language::Bash => None,
    }
}

/// The name of the receiver of the nearest Go method that contains the node.
fn go_receiver_name<'a>(scopes: &[Scope<'_>], source: &'a [u8]) -> Option<&'a str> {
    let method = scopes
        .iter()
        .rev()
        .find(|scope| scope.node.kind() == "method_declaration")?;
    let receiver = method.node.child_by_field_name("receiver")?;
    let parameter = first_child_of_kind(receiver, "parameter_declaration")?;
    let name = parameter.child_by_field_name("name")?;
    let name = text(name, source);
    (!name.is_empty()).then_some(name)
}

/// The node can name a field of the object without `this`. The finish step
/// keeps the name only when a class declares a field with that name.
fn is_implicit_candidate(language: Language, node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    let Some(parent) = parent else {
        return true;
    };
    let is_field_of = |name: &str| parent.child_by_field_name(name) == Some(node);
    match language {
        Language::Java => {
            !(parent.kind() == "method_invocation" && is_field_of("name")
                || parent.kind() == "field_access" && is_field_of("field"))
        }
        Language::CSharp => {
            !(parent.kind() == "member_access_expression" && is_field_of("name")
                || parent.kind() == "invocation_expression" && is_field_of("function"))
        }
        Language::Cpp => !(parent.kind() == "call_expression" && is_field_of("function")),
        _ => false,
    }
}

/// The node names a type, as the syntax of the language shows it.
fn is_reference(language: Language, node: Node<'_>, ancestors: &[Node<'_>]) -> bool {
    let parent = ancestors.last().copied();
    match language {
        Language::Python => {
            let Some(parent) = parent else {
                return false;
            };
            ancestors
                .iter()
                .rev()
                .take(6)
                .any(|ancestor| ancestor.kind() == "type")
                || parent.kind() == "argument_list"
                    && ancestors.len() >= 2
                    && ancestors[ancestors.len() - 2].kind() == "class_definition"
        }
        Language::CSharp => {
            let Some(parent) = parent else {
                return false;
            };
            let is_field_of = |name: &str| parent.child_by_field_name(name) == Some(node);
            matches!(
                parent.kind(),
                "base_list"
                    | "type_argument_list"
                    | "array_type"
                    | "nullable_type"
                    | "pointer_type"
            ) || is_field_of("type")
                || is_field_of("returns")
        }
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            if node.kind() == "type_identifier" {
                return true;
            }
            let Some(parent) = parent else {
                return false;
            };
            matches!(parent.kind(), "class_heritage" | "extends_clause")
                || parent.kind() == "new_expression"
                    && parent.child_by_field_name("constructor") == Some(node)
        }
        _ => true,
    }
}

/// The simple name of a base type expression. A generic argument list and a
/// qualifier are not a part of the name: `pkg.Base`, `Base<T>`, `Base[int]`,
/// `ns::Base`, and `\ns\Base` give `Base`.
fn simple_type_name(expression: &str) -> String {
    let head = expression
        .split(['<', '[', '('])
        .next()
        .unwrap_or(expression)
        .trim();
    head.rsplit(['.', ':', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(head)
        .trim()
        .to_string()
}

/// The names of the base types of a type definition, in source order. The
/// first name is the parent class that the output shows. DIT and NOC use all
/// the names.
pub fn bases(language: Language, node: Node<'_>, source: &[u8]) -> Vec<String> {
    let names_of = |parent: Node<'_>, kinds: &[&str]| -> Vec<String> {
        let mut cursor = parent.walk();
        let names = parent
            .named_children(&mut cursor)
            .filter(|child| kinds.contains(&child.kind()))
            .map(|child| simple_type_name(text(child, source)))
            .filter(|name| !name.is_empty())
            .collect();
        names
    };
    match language {
        Language::Java => {
            const TYPES: &[&str] = &["type_identifier", "generic_type", "scoped_type_identifier"];
            if let Some(clause) = node.child_by_field_name("superclass") {
                return names_of(clause, TYPES);
            }
            let Some(clause) = first_child_of_kind(node, "extends_interfaces") else {
                return Vec::new();
            };
            match first_child_of_kind(clause, "type_list") {
                Some(list) => names_of(list, TYPES),
                None => names_of(clause, TYPES),
            }
        }
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            let Some(heritage) = first_child_of_kind(node, "class_heritage") else {
                return Vec::new();
            };
            let clause = first_child_of_kind(heritage, "extends_clause").unwrap_or(heritage);
            names_of(
                clause,
                &[
                    "identifier",
                    "type_identifier",
                    "member_expression",
                    "nested_type_identifier",
                    "generic_type",
                ],
            )
            .into_iter()
            .take(1)
            .collect()
        }
        Language::Python => match node.child_by_field_name("superclasses") {
            Some(bases) => names_of(bases, &["identifier", "attribute", "subscript"]),
            None => Vec::new(),
        },
        Language::CSharp => {
            let Some(bases) = first_child_of_kind(node, "base_list") else {
                return Vec::new();
            };
            let mut cursor = bases.walk();
            let names = bases
                .named_children(&mut cursor)
                .filter(|base| base.kind() != "argument_list")
                .map(|base| simple_type_name(text(base, source)))
                .filter(|name| !name.is_empty())
                .collect();
            names
        }
        Language::Cpp => match first_child_of_kind(node, "base_class_clause") {
            Some(clause) => names_of(
                clause,
                &["type_identifier", "qualified_identifier", "template_type"],
            ),
            None => Vec::new(),
        },
        Language::Ruby => node
            .child_by_field_name("superclass")
            .and_then(|superclass| superclass.named_child(0))
            .filter(|name| ["constant", "scope_resolution"].contains(&name.kind()))
            .map(|name| vec![simple_type_name(text(name, source))])
            .unwrap_or_default(),
        Language::Php => match first_child_of_kind(node, "base_clause") {
            Some(base) => names_of(base, &["name", "qualified_name"]),
            None => Vec::new(),
        },
        Language::Rust | Language::Go | Language::C | Language::Bash => Vec::new(),
    }
}

/// A name that a type can have in the metrics of coupling.
pub fn is_valid_class_reference(name: &str, language: Language) -> bool {
    if is_primitive_type(name) || (name.len() <= 1 && language != Language::Go) {
        return false;
    }
    match language {
        Language::Ruby => {
            let first = name.chars().next().unwrap_or('a');
            first.is_ascii_uppercase()
                && !matches!(
                    name,
                    "TRUE"
                        | "FALSE"
                        | "STDIN"
                        | "STDOUT"
                        | "STDERR"
                        | "ENV"
                        | "ARGV"
                        | "DATA"
                        | "RUBY_VERSION"
                        | "RUBY_PLATFORM"
                )
                && !name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        }
        Language::Rust => name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
        // A Go type of the package can have a lower-case name. These names are predeclared types.
        Language::Go => !matches!(
            name,
            "error" | "rune" | "uintptr" | "complex64" | "complex128" | "comparable"
        ),
        _ => true,
    }
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "float"
            | "float32"
            | "float64"
            | "double"
            | "bool"
            | "boolean"
            | "Boolean"
            | "string"
            | "String"
            | "str"
            | "void"
            | "None"
            | "null"
            | "nil"
            | "byte"
            | "char"
            | "short"
            | "long"
            | "any"
            | "object"
            | "Object"
            | "number"
            | "Number"
            | "true"
            | "false"
            | "self"
            | "this"
            | "super"
    )
}
