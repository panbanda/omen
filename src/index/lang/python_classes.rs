//! Facts of Python classes that the symbols do not give: the names that a
//! class body binds, and the attributes that a module writes on an object.

use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::index::model::{Symbol, SymbolKind};

/// The name in `attribute_writes` that stands for each attribute: a
/// `setattr` or `delattr` whose name is not a literal can write any of them.
pub const ANY_ATTRIBUTE: &str = "<any attribute>";

/// The names in a class body that can change its attributes in a way that
/// the body does not show.
const DYNAMIC_NAMES: &[&str] = &[
    "__dict__", "delattr", "eval", "exec", "globals", "locals", "setattr", "vars",
];

/// The facts of one class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassBody {
    /// The position of the class in the symbols of the file.
    pub symbol: usize,
    /// Each identifier in the body outside nested function and class
    /// bodies. The set has each name that the body binds, and can have
    /// more names.
    pub names: BTreeSet<String>,
    /// The class has a decorator, a keyword argument such as `metaclass`, an
    /// unpacked base, or a body that can bind names that it does not show.
    pub opaque: bool,
}

/// Collects the class bodies and the attribute writes of a Python file
/// during the walk of the extraction, so that the facts need no walk of
/// their own.
#[derive(Default)]
pub struct Collector {
    /// The start byte of the name of each class, its names, and its
    /// `opaque` mark, in the order of the walk.
    classes: Vec<(usize, BTreeSet<String>, bool)>,
    /// The depth of each class or function that contains the current node,
    /// with the class whose body has the names at that depth. A function or
    /// a lambda has no class: the names of its body are local names.
    owners: Vec<(usize, Option<usize>)>,
    /// The start bytes of the classes that have a decorator.
    decorated: BTreeSet<usize>,
    writes: BTreeSet<String>,
}

impl Collector {
    pub fn enter(&mut self, node: Node<'_>, depth: usize, source: &[u8]) {
        let current = self.owners.last().and_then(|(_, class)| *class);
        match node.kind() {
            "identifier" => {
                if let Some(class) = current {
                    let names = &mut self.classes[class].1;
                    let text = String::from_utf8_lossy(&source[node.byte_range()]);
                    if !names.contains(text.as_ref()) {
                        names.insert(text.into_owned());
                    }
                }
            }
            "decorated_definition" => {
                if let Some(definition) = node.child_by_field_name("definition") {
                    self.decorated.insert(definition.start_byte());
                }
            }
            "class_definition" => {
                let Some(name) = node.child_by_field_name("name") else {
                    return;
                };
                // The name of a nested class is an attribute of the outer class.
                if let Some(class) = current {
                    let text = String::from_utf8_lossy(&source[name.byte_range()]).into_owned();
                    self.classes[class].1.insert(text);
                }
                let unusual_base = node
                    .child_by_field_name("superclasses")
                    .is_some_and(|bases| {
                        let mut cursor = bases.walk();
                        let found = bases.named_children(&mut cursor).any(|base| {
                            matches!(
                                base.kind(),
                                "keyword_argument" | "list_splat" | "dictionary_splat"
                            )
                        });
                        found
                    });
                let opaque = unusual_base || self.decorated.contains(&node.start_byte());
                self.classes
                    .push((name.start_byte(), BTreeSet::new(), opaque));
                self.owners.push((depth, Some(self.classes.len() - 1)));
            }
            "function_definition" | "lambda" => {
                if let (Some(class), Some(name)) = (current, node.child_by_field_name("name")) {
                    let text = String::from_utf8_lossy(&source[name.byte_range()]).into_owned();
                    self.classes[class].1.insert(text);
                }
                self.owners.push((depth, None));
            }
            "assignment" | "augmented_assignment" | "for_statement" => {
                if let Some(left) = node.child_by_field_name("left") {
                    targets(left, source, &mut self.writes);
                }
            }
            "delete_statement" | "as_pattern_target" => {
                targets(node, source, &mut self.writes);
            }
            "call" => self.call(node, source),
            _ => {}
        }
    }

    pub fn leave(&mut self, depth: usize) {
        if self.owners.last().is_some_and(|(at, _)| *at == depth) {
            self.owners.pop();
        }
    }

    /// `setattr(object, "name", value)` and `delattr(object, "name")`.
    fn call(&mut self, node: Node<'_>, source: &[u8]) {
        let named = node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "identifier"
                    && matches!(&source[function.byte_range()], b"setattr" | b"delattr")
            });
        if !named {
            return;
        }
        let name = node.child_by_field_name("arguments").and_then(|arguments| {
            let mut cursor = arguments.walk();
            let second = arguments.named_children(&mut cursor).nth(1);
            second
                .filter(|value| value.kind() == "string")
                .and_then(|value| {
                    let text = &source[value.byte_range()];
                    let inner = b"\"'"
                        .iter()
                        .find_map(|quote| text.strip_prefix(&[*quote])?.strip_suffix(&[*quote]))?;
                    let identifier = !inner.is_empty()
                        && inner
                            .iter()
                            .all(|byte| *byte == b'_' || byte.is_ascii_alphanumeric());
                    identifier.then(|| String::from_utf8_lossy(inner).into_owned())
                })
        });
        self.writes
            .insert(name.unwrap_or_else(|| ANY_ATTRIBUTE.to_string()));
    }

    /// The class bodies of the classes that are symbols, in the order of the
    /// symbols, and the attribute writes.
    pub fn finish(self, symbols: &[Symbol]) -> (Vec<ClassBody>, BTreeSet<String>) {
        let mut positions: Vec<(usize, usize)> = symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| symbol.kind == SymbolKind::Class)
            .map(|(position, symbol)| (symbol.name_range.start_byte, position))
            .collect();
        positions.sort_unstable();
        let mut bodies: Vec<ClassBody> = self
            .classes
            .into_iter()
            .filter_map(|(start, names, opaque)| {
                let found = positions
                    .binary_search_by_key(&start, |(name, _)| *name)
                    .ok()?;
                let dynamic = DYNAMIC_NAMES.iter().any(|name| names.contains(*name));
                Some(ClassBody {
                    symbol: positions[found].1,
                    names,
                    opaque: opaque || dynamic,
                })
            })
            .collect();
        bodies.sort_unstable_by_key(|body| body.symbol);
        (bodies, self.writes)
    }
}

/// Adds the attribute names that the targets in `node` write on an object
/// other than `self`. A target can be in a tuple or a list of targets.
fn targets(node: Node<'_>, source: &[u8], names: &mut BTreeSet<String>) {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.kind() == "attribute" {
            let object = node.child_by_field_name("object");
            let is_self = object.is_some_and(|object| {
                object.kind() == "identifier" && &source[object.byte_range()] == b"self"
            });
            if let (false, Some(name)) = (is_self, node.child_by_field_name("attribute")) {
                names.insert(String::from_utf8_lossy(&source[name.byte_range()]).into_owned());
            }
            continue;
        }
        if matches!(node.kind(), "subscript" | "call") {
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}
