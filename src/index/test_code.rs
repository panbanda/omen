//! The one rule that tells if a definition is test code.
//!
//! The extraction walk calls [`marks`] for each node of the kinds that
//! [`mark_kinds`] gives. A mark is a byte range that is test code. The path
//! of the file is the second source: [`is_test_path`]. Each consumer reads
//! the result from `Symbol::test_role` or from `FileSymbols::is_test_at`.
//!
//! The rule for each language:
//!
//! - Rust: an item with `#[cfg(test)]`, a function with a test attribute, and
//!   a module file that a `#[cfg(test)] mod name;` declares.
//! - Python: a pytest fixture, a `TestCase` subclass, and the `test_*` names
//!   of a test file.
//! - ECMAScript: the callbacks of `describe`, `it`, `test`, and the hooks.
//! - Go: the `Test`, `Benchmark`, `Fuzz`, and `Example` functions of a test file.
//! - Java and C#: a method with a test annotation, and its class.
//! - Ruby: a Minitest class, and the blocks of `describe`, `it`, and `specify`.
//! - PHP: a PHPUnit class, and a method with `@test` or `#[Test]`.
//! - C and C++: the function that a test macro such as `TEST` defines.
//! - Bash: the path of the file only. A Bats file is not a Bash file.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

use super::extract::FileSymbols;
use super::model::{ImportKind, TestRole};
use crate::core::{is_test_file, Language};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    /// The range is test code.
    Code,
    /// The range is a definition that a test runner calls.
    Entry,
    /// The range is a definition that a test runner calls when the definition
    /// is in test code. Outside test code, the mark tells nothing.
    EntryInTestCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMark {
    pub range: Range<usize>,
    /// The first line and the last line of the range. Lines start at 1.
    pub lines: (u32, u32),
    pub kind: MarkKind,
}

/// The path follows a test convention of the language.
pub fn is_test_path(file: &str, language: Language) -> bool {
    if is_test_file(file) {
        return true;
    }
    let normalized = file.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    match language {
        // The name of a file such as `tests.rs` is not a rule: a file with
        // that name can be production code. `link_files` finds the file of a
        // `#[cfg(test)] mod tests;` declaration.
        Language::Rust => normalized.split('/').any(|part| part == "benches"),
        Language::Python => name == "conftest.py",
        _ => false,
    }
}

/// The node kinds for which the walk calls [`marks`].
pub fn mark_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &["attribute_item", "inner_attribute_item"],
        Language::Python => &[
            "decorated_definition",
            "class_definition",
            "function_definition",
        ],
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            &["call_expression"]
        }
        Language::Go => &["function_declaration"],
        Language::Java => &["method_declaration"],
        Language::CSharp => &["method_declaration", "class_declaration"],
        Language::Ruby => &["class", "method", "call"],
        Language::Php => &["class_declaration", "method_declaration"],
        Language::C => &["function_definition", "compound_statement"],
        Language::Cpp => &["function_definition"],
        Language::Bash => &[],
    }
}

/// The kinds of the ancestors that [`MarkContext::owner`] gives: the class,
/// the module, or the function that contains a definition.
pub fn owner_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &["mod_item"],
        Language::Python => &["class_definition", "function_definition"],
        Language::Java | Language::CSharp | Language::Php => &["class_declaration"],
        Language::Ruby => &["class", "module", "method"],
        _ => &[],
    }
}

/// What the walk knows at one node.
pub struct MarkContext<'a, 'tree> {
    pub language: Language,
    pub source: &'a [u8],
    /// The nodes that contain the node, from the root.
    pub ancestors: &'a [Node<'tree>],
    /// The ancestors that have a kind of [`owner_kinds`], the nearest last.
    /// The walk keeps this list, thus a question for the owner does not
    /// search all the ancestors.
    pub owners: &'a [Owner<'tree>],
    pub in_test_file: bool,
    /// The callee roots that the walk found, by the id of the call. One search down a chain of
    /// calls gives the root of each call of the chain, so the walk reads each chain one time.
    pub callee_roots: &'a CalleeRoots<'tree>,
    /// The ids of the parents whose Rust attributes the walk read. The walk reads all the
    /// attributes of a parent at its first attribute, in one pass over the children.
    pub attribute_parents: &'a AttributeParents,
}

/// The ids of the parents whose attributes the walk read.
pub type AttributeParents = std::cell::RefCell<std::collections::HashSet<usize>>;

/// The root of the callee of each call that the walk read, as `ecma_callee_root` gives it.
pub type CalleeRoots<'tree> =
    std::cell::RefCell<std::collections::HashMap<usize, Option<(Node<'tree>, bool)>>>;

/// An ancestor that has a kind of [`owner_kinds`].
#[derive(Debug, Clone, Copy)]
pub struct Owner<'tree> {
    pub node: Node<'tree>,
    /// The owner is a class of a test framework, such as a `TestCase`
    /// subclass. The walk reads the class one time, and not one time for
    /// each of its methods.
    pub is_test_class: bool,
}

impl<'tree> Owner<'tree> {
    pub fn new(node: Node<'tree>, language: Language, source: &[u8]) -> Self {
        let is_test_class = match (language, node.kind()) {
            (Language::Python, "class_definition") => python_is_test_case(node, source),
            (Language::Ruby, "class") => ruby_is_test_class(node, source),
            (Language::Php, "class_declaration") => php_is_test_case(node, source),
            _ => false,
        };
        Self {
            node,
            is_test_class,
        }
    }
}

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

impl MarkContext<'_, '_> {
    fn text(&self, node: Node<'_>) -> &str {
        node_text(node, self.source)
    }

    /// The nearest ancestor that has a kind of [`owner_kinds`].
    fn owner(&self) -> Option<Node<'_>> {
        self.owners.last().map(|owner| owner.node)
    }

    /// The nearest owner is a class of a test framework.
    fn owner_is_test_class(&self) -> bool {
        self.owners.last().is_some_and(|owner| owner.is_test_class)
    }
}

fn mark(node: Node<'_>, kind: MarkKind) -> TestMark {
    TestMark {
        range: node.byte_range(),
        lines: (
            node.start_position().row as u32 + 1,
            node.end_position().row as u32 + 1,
        ),
        kind,
    }
}

/// Adds the marks that `node` gives.
pub fn marks<'tree>(node: Node<'tree>, context: &MarkContext<'_, 'tree>, out: &mut Vec<TestMark>) {
    match context.language {
        Language::Rust => rust_marks(node, context, out),
        Language::Python => python_marks(node, context, out),
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            ecma_marks(node, context, out)
        }
        Language::Go => go_marks(node, context, out),
        Language::Java => java_marks(node, context, out),
        Language::CSharp => csharp_marks(node, context, out),
        Language::Ruby => ruby_marks(node, context, out),
        Language::Php => php_marks(node, context, out),
        Language::C | Language::Cpp => c_marks(node, context, out),
        Language::Bash => {}
    }
}

/// The last segment of a path such as `tokio::test`, `pytest.fixture`, or `NUnit.Framework.Test`.
fn last_segment(path: &str) -> &str {
    path.rsplit([':', '.', '\\']).next().unwrap_or(path).trim()
}

// ----- Rust -----

/// The last segment of the path of an attribute that makes a function a test.
const RUST_TEST_ATTRIBUTES: [&str; 9] = [
    "test",
    "bench",
    "rstest",
    "test_case",
    "test_matrix",
    "quickcheck",
    "proptest",
    "wasm_bindgen_test",
    "parameterized",
];

/// The attributes that do not register a function with a test runner.
const RUST_INERT_ATTRIBUTES: [&str; 31] = [
    "allow",
    "automatically_derived",
    "cfg",
    "cfg_attr",
    "cold",
    "deny",
    "deprecated",
    "derive",
    "doc",
    "expect",
    "export_name",
    "forbid",
    "ignore",
    "inline",
    "instrument",
    "link_name",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "no_mangle",
    "non_exhaustive",
    "optimize",
    "path",
    "repr",
    "should_panic",
    "target_feature",
    "track_caller",
    "unsafe",
    "used",
    "warn",
];

/// A `cfg` predicate is true only in a test build.
///
/// `test` and `all(test, x)` are such predicates. `any(test, x)` and
/// `not(test)` are not.
///
/// The text is read one time from left to right. A list such as `all(` opens
/// a frame on a stack in the heap, and its `)` gives the answer of the frame
/// to the member that contains it. Thus the time is linear in the length of
/// the text, and a deep nesting cannot overflow the stack of the thread.
pub fn cfg_requires_test(predicate: &str) -> bool {
    let mut frames = vec![CfgFrame::new(CfgOperator::Root, 0)];
    let mut in_string = false;
    let mut escaped = false;
    for (position, character) in predicate.char_indices() {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        let Some(frame) = frames.last_mut() else {
            return false;
        };
        match character {
            '"' => {
                in_string = true;
                frame.after_list |= frame.list.is_some();
            }
            '[' | '{' => frame.brackets += 1,
            ']' | '}' => frame.brackets = frame.brackets.saturating_sub(1),
            ',' if frame.brackets == 0 => {
                frame.end_member(predicate, position, true);
                frame.member_start = position + 1;
            }
            '(' => {
                let operator = if frame.list.is_some() {
                    CfgOperator::Other
                } else {
                    match predicate[frame.member_start..position].trim() {
                        "all" => CfgOperator::All,
                        "any" => CfgOperator::Any,
                        _ => CfgOperator::Other,
                    }
                };
                frames.push(CfgFrame::new(operator, position + 1));
            }
            ')' => {
                let Some(mut closed) = frames.pop() else {
                    return false;
                };
                let Some(parent) = frames.last_mut() else {
                    // A `)` without its `(`.
                    return false;
                };
                closed.end_member(predicate, position, false);
                parent.list = Some(closed.answer());
                parent.after_list = false;
            }
            _ if !character.is_whitespace() => frame.after_list |= frame.list.is_some(),
            _ => {}
        }
    }
    if in_string || frames.len() != 1 {
        return false;
    }
    let mut root = frames.remove(0);
    root.end_member(predicate, predicate.len(), false);
    root.answer()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfgOperator {
    /// The whole predicate. It must have one member.
    Root,
    All,
    Any,
    /// `not` and each other list. It is not true only in a test build.
    Other,
}

/// One list of a `cfg` predicate that the scan has not closed.
struct CfgFrame {
    operator: CfgOperator,
    /// The byte at which the member that the scan reads starts.
    member_start: usize,
    /// The answer of the list in the current member, such as `all(test)`.
    list: Option<bool>,
    /// The current member has text after its list. Then it is not a list.
    after_list: bool,
    brackets: usize,
    members: usize,
    any_member: bool,
    each_member: bool,
}

impl CfgFrame {
    fn new(operator: CfgOperator, member_start: usize) -> Self {
        Self {
            operator,
            member_start,
            list: None,
            after_list: false,
            brackets: 0,
            members: 0,
            any_member: false,
            each_member: true,
        }
    }

    /// Adds the member that ends at `end`. An empty member counts only
    /// before a comma, as the argument list of an attribute does.
    fn end_member(&mut self, predicate: &str, end: usize, before_comma: bool) {
        let text = predicate[self.member_start..end].trim();
        let answer = match self.list.take() {
            Some(answer) => answer && !self.after_list,
            None => text == "test",
        };
        self.after_list = false;
        if text.is_empty() && !before_comma {
            return;
        }
        self.members += 1;
        self.any_member |= answer;
        self.each_member &= answer;
    }

    fn answer(&self) -> bool {
        match self.operator {
            CfgOperator::Root => self.members == 1 && self.each_member,
            CfgOperator::All => self.any_member,
            CfgOperator::Any => self.members > 0 && self.each_member,
            CfgOperator::Other => false,
        }
    }
}

/// The text names a path that ends in `::test`, outside a string.
fn names_test_path(arguments: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    let mut code = String::with_capacity(arguments.len());
    for character in arguments.chars() {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            code.push(' ');
        } else if !character.is_whitespace() {
            code.push(character);
        }
    }
    code.match_indices("::test").any(|(position, found)| {
        code[position + found.len()..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_')
    })
}

/// The path and the arguments of an attribute item, as text: `#[cfg(test)]`
/// gives `cfg` and `(test)`.
pub fn rust_attribute_text<'a>(item: Node<'_>, source: &'a [u8]) -> Option<(&'a str, &'a str)> {
    let text = item.utf8_text(source).ok()?;
    let text = text
        .trim_start_matches('#')
        .trim_start_matches('!')
        .trim()
        .strip_prefix('[')?
        .strip_suffix(']')?
        .trim();
    let end = text
        .find(|character: char| character == '(' || character == '=' || character.is_whitespace())
        .unwrap_or(text.len());
    Some((&text[..end], text[end..].trim()))
}

fn is_rust_comment(node: Node<'_>) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

/// Each run of outer attributes among the children of `parent`, and the item to which the run
/// applies. Comments can be between the attributes of a run and the item. A run with no item
/// after it applies to nothing.
///
/// One pass of a cursor reads the children. `Node::next_sibling` and `Node::prev_sibling` read
/// the children of the parent from the first child, so a step from each attribute to its
/// siblings takes time that grows with the square of the number of children.
fn rust_attribute_runs<'tree>(parent: Node<'tree>) -> Vec<(Vec<Node<'tree>>, Node<'tree>)> {
    let mut runs = Vec::new();
    let mut attributes = Vec::new();
    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        match child.kind() {
            "attribute_item" => attributes.push(child),
            _ if is_rust_comment(child) => {}
            _ if attributes.is_empty() => {}
            _ => runs.push((std::mem::take(&mut attributes), child)),
        }
    }
    runs
}

fn rust_cfg_requires_test(path: &str, arguments: &str) -> bool {
    path == "cfg"
        && arguments
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(cfg_requires_test)
}

fn rust_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    if node.kind() == "inner_attribute_item" {
        // `#![cfg(test)]` applies to the module or the file that contains it.
        let requires_test = rust_attribute_text(node, context.source)
            .is_some_and(|(path, arguments)| rust_cfg_requires_test(path, arguments));
        if requires_test {
            let owner = context
                .owner()
                .or_else(|| context.ancestors.first().copied());
            out.extend(owner.map(|owner| mark(owner, MarkKind::Code)));
        }
        return;
    }
    let Some(parent) = context.ancestors.last().copied() else {
        return;
    };
    if !context.attribute_parents.borrow_mut().insert(parent.id()) {
        return;
    }
    for (attributes, target) in rust_attribute_runs(parent) {
        for attribute in attributes {
            rust_attribute_marks(attribute, target, context, out);
        }
    }
}

fn rust_attribute_marks(
    attribute: Node<'_>,
    target: Node<'_>,
    context: &MarkContext<'_, '_>,
    out: &mut Vec<TestMark>,
) {
    let Some((path, arguments)) = rust_attribute_text(attribute, context.source) else {
        return;
    };
    if rust_cfg_requires_test(path, arguments) {
        out.push(mark(target, MarkKind::Code));
        return;
    }
    if target.kind() != "function_item" {
        return;
    }
    let name = last_segment(path);
    if RUST_TEST_ATTRIBUTES.contains(&name) || names_test_path(arguments) {
        out.push(mark(target, MarkKind::Entry));
    } else if !RUST_INERT_ATTRIBUTES.contains(&name) {
        // An attribute macro that omen does not know can register the
        // function with the test runner, as `#[perf]` of Zed does.
        out.push(mark(target, MarkKind::EntryInTestCode));
    }
}

// ----- Python -----

const PYTHON_HOOK_NAMES: [&str; 17] = [
    "setUp",
    "tearDown",
    "setUpClass",
    "tearDownClass",
    "setUpModule",
    "tearDownModule",
    "asyncSetUp",
    "asyncTearDown",
    "setup_method",
    "teardown_method",
    "setup_class",
    "teardown_class",
    "setup_module",
    "teardown_module",
    "setup_function",
    "teardown_function",
    "runTest",
];

/// The class has a superclass whose name ends in `TestCase`.
fn python_is_test_case(class: Node<'_>, source: &[u8]) -> bool {
    let Some(superclasses) = class.child_by_field_name("superclasses") else {
        return false;
    };
    let mut cursor = superclasses.walk();
    let found = superclasses
        .named_children(&mut cursor)
        .any(|base| last_segment(node_text(base, source)).ends_with("TestCase"));
    found
}

fn python_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    match node.kind() {
        "decorated_definition" => {
            let Some(definition) = node.child_by_field_name("definition") else {
                return;
            };
            let mut cursor = node.walk();
            let is_test = node
                .children(&mut cursor)
                .filter(|child| child.kind() == "decorator")
                .any(|decorator| {
                    let text = context.text(decorator).trim_start_matches('@').trim();
                    let path = text.split('(').next().unwrap_or(text).trim();
                    matches!(
                        path,
                        "pytest.fixture" | "pytest_asyncio.fixture" | "fixture"
                    ) || path.starts_with("pytest.mark.")
                });
            if is_test {
                out.push(mark(definition, MarkKind::Entry));
            }
        }
        "class_definition" => {
            let name = node
                .child_by_field_name("name")
                .map_or("", |name| context.text(name));
            if python_is_test_case(node, context.source)
                || (context.in_test_file && name.starts_with("Test"))
            {
                out.push(mark(node, MarkKind::Entry));
            }
        }
        _ => {
            let name = node
                .child_by_field_name("name")
                .map_or("", |name| context.text(name));
            let in_test_case = context.owner_is_test_class();
            let named_as_test = name.starts_with("test") || PYTHON_HOOK_NAMES.contains(&name);
            let is_pytest_hook = context.in_test_file && name.starts_with("pytest_");
            if (named_as_test && (in_test_case || context.in_test_file)) || is_pytest_hook {
                out.push(mark(node, MarkKind::Entry));
            }
        }
    }
}

// ----- ECMAScript -----

const ECMA_SUITES: [&str; 8] = [
    "describe",
    "it",
    "test",
    "fdescribe",
    "fit",
    "xdescribe",
    "xit",
    "xtest",
];
const ECMA_HOOKS: [&str; 4] = ["beforeEach", "afterEach", "beforeAll", "afterAll"];

/// The first name of the callee: `describe` in `describe.each(table)(name, run)`.
fn ecma_callee_root<'tree>(
    call: Node<'tree>,
    roots: &CalleeRoots<'tree>,
) -> Option<(Node<'tree>, bool)> {
    if let Some(known) = roots.borrow().get(&call.id()) {
        return *known;
    }
    // Each call on the way down has the same root. Its callee is qualified unless the callee
    // is the root itself.
    let mut calls = vec![call];
    let mut current = call.child_by_field_name("function");
    let root = loop {
        let Some(node) = current else {
            break None;
        };
        current = match node.kind() {
            "identifier" => break Some(node),
            "member_expression" => node.child_by_field_name("object"),
            "call_expression" => {
                if let Some(known) = roots.borrow().get(&node.id()) {
                    break known.map(|(root, _)| root);
                }
                calls.push(node);
                node.child_by_field_name("function")
            }
            _ => break None,
        };
    };
    let mut roots = roots.borrow_mut();
    for each in &calls {
        let qualified = each
            .child_by_field_name("function")
            .is_some_and(|function| function.kind() != "identifier");
        roots.insert(each.id(), root.map(|root| (root, qualified)));
    }
    root.map(|root| {
        let qualified = call
            .child_by_field_name("function")
            .is_some_and(|function| function.kind() != "identifier");
        (root, qualified)
    })
}

fn ecma_marks<'tree>(node: Node<'tree>, context: &MarkContext<'_, 'tree>, out: &mut Vec<TestMark>) {
    let Some((root, qualified)) = ecma_callee_root(node, context.callee_roots) else {
        return;
    };
    let name = context.text(root);
    let is_suite = ECMA_SUITES.contains(&name);
    if !is_suite && !ECMA_HOOKS.contains(&name) {
        return;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = arguments.walk();
    let values: Vec<Node<'_>> = arguments.named_children(&mut cursor).collect();
    let callbacks: Vec<Node<'_>> = values
        .iter()
        .copied()
        .filter(|value| {
            matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function" | "generator_function"
            )
        })
        .collect();
    if callbacks.is_empty() {
        return;
    }
    // A suite or a test has a title. A hook has only its callback. A hook of
    // an object, as `hooks.beforeEach(run)` is, has a different first name.
    let has_title = values
        .first()
        .is_some_and(|first| matches!(first.kind(), "string" | "template_string"));
    if (is_suite && !has_title) || (!is_suite && qualified) {
        return;
    }
    out.push(mark(node, MarkKind::Code));
    out.extend(
        callbacks
            .into_iter()
            .map(|callback| mark(callback, MarkKind::Entry)),
    );
}

// ----- Go -----

/// `go test` runs a function whose name is the prefix, or the prefix and
/// then a character that is not a lowercase letter.
fn go_is_test_name(name: &str) -> bool {
    ["Test", "Benchmark", "Fuzz", "Example"]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| !rest.starts_with(|first: char| first.is_lowercase()))
        })
}

fn go_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    if !context.in_test_file {
        return;
    }
    let name = node
        .child_by_field_name("name")
        .map_or("", |name| context.text(name));
    if go_is_test_name(name) {
        out.push(mark(node, MarkKind::Entry));
    }
}

// ----- Java and C# -----

/// The annotations of JUnit 4, JUnit 5, and TestNG that make a method a test or a hook.
const JAVA_TEST_ANNOTATIONS: [&str; 22] = [
    "Test",
    "ParameterizedTest",
    "RepeatedTest",
    "TestFactory",
    "TestTemplate",
    "BeforeEach",
    "AfterEach",
    "BeforeAll",
    "AfterAll",
    "Before",
    "After",
    "BeforeClass",
    "AfterClass",
    "BeforeMethod",
    "AfterMethod",
    "BeforeSuite",
    "AfterSuite",
    "BeforeTest",
    "AfterTest",
    "BeforeGroups",
    "AfterGroups",
    "DataProvider",
];

fn java_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    let mut cursor = node.walk();
    let Some(modifiers) = node
        .children(&mut cursor)
        .find(|child| child.kind() == "modifiers")
    else {
        return;
    };
    let mut cursor = modifiers.walk();
    let is_test = modifiers
        .children(&mut cursor)
        .filter(|child| matches!(child.kind(), "marker_annotation" | "annotation"))
        .any(|annotation| {
            let name = annotation
                .child_by_field_name("name")
                .map_or("", |name| last_segment(context.text(name)));
            // AspectJ has `@Before` and `@After` with a pointcut argument.
            // The JUnit 4 hooks have no argument.
            let is_advice = matches!(name, "Before" | "After") && annotation.kind() == "annotation";
            JAVA_TEST_ANNOTATIONS.contains(&name) && !is_advice
        });
    if !is_test {
        return;
    }
    out.push(mark(node, MarkKind::Entry));
    out.extend(context.owner().map(|class| mark(class, MarkKind::Code)));
}

/// The attributes of NUnit, xUnit, and MSTest that make a method a test or a hook.
const CSHARP_TEST_ATTRIBUTES: [&str; 17] = [
    "Test",
    "TestCase",
    "TestCaseSource",
    "Fact",
    "Theory",
    "TestMethod",
    "DataTestMethod",
    "SetUp",
    "TearDown",
    "OneTimeSetUp",
    "OneTimeTearDown",
    "TestInitialize",
    "TestCleanup",
    "ClassInitialize",
    "ClassCleanup",
    "AssemblyInitialize",
    "AssemblyCleanup",
];
const CSHARP_CLASS_ATTRIBUTES: [&str; 2] = ["TestFixture", "TestClass"];

fn csharp_has_attribute(node: Node<'_>, context: &MarkContext<'_, '_>, names: &[&str]) -> bool {
    let mut cursor = node.walk();
    let lists: Vec<Node<'_>> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "attribute_list")
        .collect();
    lists.into_iter().any(|list| {
        let mut cursor = list.walk();
        let found = list
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "attribute")
            .filter_map(|attribute| attribute.child_by_field_name("name"))
            .any(|name| {
                let name = last_segment(context.text(name));
                names.contains(&name.strip_suffix("Attribute").unwrap_or(name))
            });
        found
    })
}

fn csharp_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    if node.kind() == "class_declaration" {
        if csharp_has_attribute(node, context, &CSHARP_CLASS_ATTRIBUTES) {
            out.push(mark(node, MarkKind::Entry));
        }
        return;
    }
    if !csharp_has_attribute(node, context, &CSHARP_TEST_ATTRIBUTES) {
        return;
    }
    out.push(mark(node, MarkKind::Entry));
    out.extend(context.owner().map(|class| mark(class, MarkKind::Code)));
}

// ----- Ruby -----

/// The superclass is a class of Minitest, Test::Unit, or the Rails test cases.
fn ruby_is_test_class(class: Node<'_>, source: &[u8]) -> bool {
    let Some(superclass) = class.child_by_field_name("superclass") else {
        return false;
    };
    let text = node_text(superclass, source);
    let path = text.trim_start_matches('<').trim();
    let name = last_segment(path);
    path == "Minitest::Test"
        || path == "Minitest::Spec"
        || name.ends_with("TestCase")
        || name.ends_with("IntegrationTest")
}

const RUBY_SPEC_BLOCKS: [&str; 7] = [
    "describe",
    "it",
    "specify",
    "shared_examples",
    "shared_examples_for",
    "shared_context",
    "context",
];

fn ruby_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    match node.kind() {
        "class" => {
            if ruby_is_test_class(node, context.source) {
                out.push(mark(node, MarkKind::Entry));
            }
        }
        "method" => {
            let name = node
                .child_by_field_name("name")
                .map_or("", |name| context.text(name));
            let in_test_class = context.owner_is_test_class();
            let named_as_test = name.starts_with("test_") || name == "setup" || name == "teardown";
            if named_as_test && (in_test_class || context.in_test_file) {
                out.push(mark(node, MarkKind::Entry));
            }
        }
        _ => {
            if node.child_by_field_name("block").is_none() {
                return;
            }
            let method = node
                .child_by_field_name("method")
                .map_or("", |method| context.text(method));
            if !RUBY_SPEC_BLOCKS.contains(&method) {
                return;
            }
            let receiver = node
                .child_by_field_name("receiver")
                .map(|receiver| context.text(receiver));
            // `context` is a name that other libraries use. It is a group of
            // examples only in a `describe` block, which has its own mark.
            let is_spec = match receiver {
                None => method != "context",
                Some(receiver) => receiver == "RSpec",
            };
            if is_spec {
                out.push(mark(node, MarkKind::Code));
            }
        }
    }
}

// ----- PHP -----

const PHP_HOOK_NAMES: [&str; 4] = [
    "setUp",
    "tearDown",
    "setUpBeforeClass",
    "tearDownAfterClass",
];
const PHP_TEST_ATTRIBUTES: [&str; 8] = [
    "Test",
    "DataProvider",
    "Before",
    "After",
    "BeforeClass",
    "AfterClass",
    "TestWith",
    "TestDox",
];

/// The class extends a class whose name ends in `TestCase`.
fn php_is_test_case(class: Node<'_>, source: &[u8]) -> bool {
    let mut cursor = class.walk();
    let found = class
        .children(&mut cursor)
        .filter(|child| child.kind() == "base_clause")
        .any(|base| {
            let text = node_text(base, source);
            last_segment(text.trim_start_matches("extends").trim()).ends_with("TestCase")
        });
    found
}

/// The method has a PHPUnit attribute, or its documentation comment has the tag `@test`.
fn php_is_marked_test(method: Node<'_>, context: &MarkContext<'_, '_>) -> bool {
    let mut cursor = method.walk();
    let has_attribute = method
        .children(&mut cursor)
        .filter(|child| child.kind() == "attribute_list")
        .any(|list| {
            let text = context.text(list);
            text.trim_matches(|character| character == '#' || character == '[' || character == ']')
                .split(',')
                .any(|attribute| {
                    let path = attribute.split('(').next().unwrap_or(attribute).trim();
                    PHP_TEST_ATTRIBUTES.contains(&last_segment(path))
                })
        });
    if has_attribute {
        return true;
    }
    method
        .prev_sibling()
        .filter(|previous| previous.kind() == "comment")
        .is_some_and(|comment| {
            context
                .text(comment)
                .split(|character: char| !character.is_alphanumeric() && character != '@')
                .any(|word| matches!(word, "@test" | "@before" | "@after" | "@dataProvider"))
        })
}

fn php_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    if node.kind() == "class_declaration" {
        if php_is_test_case(node, context.source) {
            out.push(mark(node, MarkKind::Entry));
        }
        return;
    }
    let name = node
        .child_by_field_name("name")
        .map_or("", |name| context.text(name));
    let in_test_case = context.owner_is_test_class();
    let marked = php_is_marked_test(node, context);
    let named_as_test = name.starts_with("test") || PHP_HOOK_NAMES.contains(&name);
    if marked || (named_as_test && in_test_case) {
        out.push(mark(node, MarkKind::Entry));
        out.extend(context.owner().map(|class| mark(class, MarkKind::Code)));
    }
}

// ----- C and C++ -----

/// The macros of GoogleTest, Catch2, Boost.Test, and Criterion that define a test function.
const C_TEST_MACROS: [&str; 12] = [
    "TEST",
    "TEST_F",
    "TEST_P",
    "TYPED_TEST",
    "TYPED_TEST_P",
    "TEST_CASE",
    "TEST_CASE_METHOD",
    "SCENARIO",
    "BOOST_AUTO_TEST_CASE",
    "BOOST_FIXTURE_TEST_CASE",
    "Test",
    "ParameterizedTest",
];

fn c_marks(node: Node<'_>, context: &MarkContext<'_, '_>, out: &mut Vec<TestMark>) {
    if node.kind() == "compound_statement" {
        // The C grammar has no function without a result type. It gives the
        // use of the macro as a call statement, and then the body as a block
        // of the file.
        let at_top = context
            .ancestors
            .last()
            .is_some_and(|parent| parent.kind() == "translation_unit");
        // `Node::prev_sibling` searches from the root, so the test of the place comes first.
        let macro_name = at_top
            .then(|| node.prev_sibling())
            .flatten()
            .filter(|previous| previous.kind() == "expression_statement")
            .and_then(|statement| statement.named_child(0))
            .filter(|call| call.kind() == "call_expression")
            .and_then(|call| call.child_by_field_name("function"))
            .map_or("", |name| context.text(name));
        if C_TEST_MACROS.contains(&macro_name) {
            out.push(mark(node, MarkKind::Code));
        }
        return;
    }
    // A use of a test macro has no result type: `TEST(Suite, Name) { ... }`.
    // A constructor in the body of its class has none too.
    let in_class = context
        .ancestors
        .last()
        .is_some_and(|parent| parent.kind() == "field_declaration_list");
    if in_class || node.child_by_field_name("type").is_some() {
        return;
    }
    let name = node
        .child_by_field_name("declarator")
        .filter(|declarator| declarator.kind() == "function_declarator")
        .and_then(|declarator| declarator.child_by_field_name("declarator"))
        .map_or("", |name| context.text(name));
    if C_TEST_MACROS.contains(&name) {
        out.push(mark(node, MarkKind::Entry));
    }
}

// ----- The result for a file -----

/// The marks of one file in a form that answers each question in logarithmic time.
#[derive(Debug, Default)]
pub struct MarkTable {
    /// The ranges of test code, in order, without a range in a different range.
    pub ranges: Vec<Range<usize>>,
    /// The first line and the last line of each range of `ranges`.
    pub lines: Vec<(u32, u32)>,
    entries: HashMap<(usize, usize), MarkKind>,
}

impl MarkTable {
    pub fn new(mut marks: Vec<TestMark>) -> Self {
        let mut entries = HashMap::new();
        for mark in &marks {
            if mark.kind == MarkKind::Code {
                continue;
            }
            let entry = entries
                .entry((mark.range.start, mark.range.end))
                .or_insert(mark.kind);
            if mark.kind == MarkKind::Entry {
                *entry = MarkKind::Entry;
            }
        }
        marks.retain(|mark| mark.kind != MarkKind::EntryInTestCode);
        marks.sort_by_key(|mark| (mark.range.start, std::cmp::Reverse(mark.range.end)));
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut lines = Vec::new();
        for mark in marks {
            if ranges
                .last()
                .is_none_or(|last| mark.range.start >= last.end)
            {
                ranges.push(mark.range);
                lines.push(mark.lines);
            }
        }
        Self {
            ranges,
            lines,
            entries,
        }
    }

    /// The role of the definition with this byte range, and the answer to the
    /// question: does the definition become an entry when its file is test code?
    pub fn role(&self, range: Range<usize>, in_test_file: bool) -> (TestRole, bool) {
        let in_test_code = in_test_file || is_in_ranges(&self.ranges, range.start);
        match self.entries.get(&(range.start, range.end)) {
            Some(MarkKind::Entry) => (TestRole::Entry, false),
            Some(_) if in_test_code => (TestRole::Entry, false),
            Some(_) => (TestRole::None, true),
            None if in_test_code => (TestRole::Support, false),
            None => (TestRole::None, false),
        }
    }
}

/// A function of a test file with this name is a test by convention, in a
/// language that has a test tool which finds a test by its name only: Unity
/// for C, shUnit2 for Bash, and JUnit 3 for Java. Go, Python, Ruby, and PHP
/// have a name rule of their own in [`marks`]. The tools of the other
/// languages find a test by its syntax.
pub fn is_test_by_name(language: Language, name: &str) -> bool {
    matches!(language, Language::C | Language::Bash | Language::Java) && name.starts_with("test")
}

/// `ranges` is in order and has no range in a different range.
pub fn is_in_ranges(ranges: &[Range<usize>], byte: usize) -> bool {
    let position = ranges.partition_point(|range| range.start <= byte);
    position > 0 && byte < ranges[position - 1].end
}

/// The line ranges of `lines` contain the line. Lines start at 1.
pub fn is_in_lines(lines: &[(u32, u32)], line: u32) -> bool {
    let position = lines.partition_point(|(start, _)| *start <= line);
    position > 0 && line <= lines[position - 1].1
}

/// The ranges of test code in a tree, for a consumer that has a tree and no
/// index. The path of the file is not a part of this answer.
pub fn test_ranges_of_tree(root: Node<'_>, source: &[u8], language: Language) -> Vec<Range<usize>> {
    marks_of_tree(root, source, language).ranges
}

/// The line ranges of test code in a tree. See [`test_ranges_of_tree`].
pub fn test_lines_of_tree(root: Node<'_>, source: &[u8], language: Language) -> Vec<(u32, u32)> {
    marks_of_tree(root, source, language).lines
}

fn marks_of_tree(root: Node<'_>, source: &[u8], language: Language) -> MarkTable {
    let kinds = mark_kinds(language);
    let mut found = Vec::new();
    let mut ancestors: Vec<Node<'_>> = Vec::new();
    let mut owners: Vec<Owner<'_>> = Vec::new();
    let callee_roots = CalleeRoots::default();
    let attribute_parents = AttributeParents::default();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_named() && kinds.contains(&node.kind()) {
            let context = MarkContext {
                language,
                source,
                ancestors: &ancestors,
                owners: &owners,
                in_test_file: false,
                callee_roots: &callee_roots,
                attribute_parents: &attribute_parents,
            };
            marks(node, &context, &mut found);
        }
        if cursor.goto_first_child() {
            push_ancestor(node, language, source, &mut ancestors, &mut owners);
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return MarkTable::new(found);
            }
            pop_ancestor(&mut ancestors, &mut owners);
        }
    }
}

/// Adds the node that the walk enters to the ancestors, and to the owners
/// when it has a kind of [`owner_kinds`].
pub fn push_ancestor<'tree>(
    node: Node<'tree>,
    language: Language,
    source: &[u8],
    ancestors: &mut Vec<Node<'tree>>,
    owners: &mut Vec<Owner<'tree>>,
) {
    if owner_kinds(language).contains(&node.kind()) {
        owners.push(Owner::new(node, language, source));
    }
    ancestors.push(node);
}

/// Removes the node that the walk leaves. See [`push_ancestor`].
pub fn pop_ancestor<'tree>(ancestors: &mut Vec<Node<'tree>>, owners: &mut Vec<Owner<'tree>>) {
    if let Some(left) = ancestors.pop() {
        if owners
            .last()
            .is_some_and(|owner| owner.node.id() == left.id())
        {
            owners.pop();
        }
    }
}

/// The file of the module that `mod name;` in the Rust file `declaring`
/// declares. `find` gives the path of a file of the repository.
///
/// The path rules of Rust give the answer, and the import resolution does
/// not: a wrong import target must not make a file test code. In `lib.rs`,
/// `main.rs`, and `mod.rs`, the module file is next to the file. In a
/// different file, it is in the directory with the name of that file. Such
/// a file can also be the root of a crate, as a file with the `[lib] path`
/// of a manifest and each file of `tests/` are. Then the module file is next
/// to it. The attribute `#[path]` is not read.
pub fn rust_module_file<T>(
    declaring: &str,
    name: &str,
    find: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    let (directory, file_name) = declaring.rsplit_once('/').unwrap_or(("", declaring));
    let join = |parts: &[&str]| {
        parts
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<&str>>()
            .join("/")
    };
    let stem = file_name.strip_suffix(".rs").unwrap_or(file_name);
    let source = format!("{name}.rs");
    let mut candidates = Vec::new();
    if !matches!(file_name, "lib.rs" | "main.rs" | "mod.rs") {
        candidates.push(join(&[directory, stem, &source]));
        candidates.push(join(&[directory, stem, name, "mod.rs"]));
    }
    candidates.push(join(&[directory, &source]));
    candidates.push(join(&[directory, name, "mod.rs"]));
    candidates.iter().find_map(|path| find(path))
}

/// Completes the facts that need a different file.
///
/// A Rust module file has no mark of its own when `#[cfg(test)] mod name;`
/// declares it in a different file. This step makes such a file, and each
/// module file that it declares, test code. The result does not depend on
/// the order of `files`.
pub fn link_files(files: &mut [FileSymbols]) {
    let positions: HashMap<String, usize> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| file.language == Language::Rust)
        .map(|(position, file)| (file.file.clone(), position))
        .collect();
    let declared_modules = |file: &FileSymbols, only_in_test_code: bool| -> Vec<usize> {
        file.imports
            .iter()
            .filter(|binding| binding.kind == ImportKind::ModuleDeclaration)
            .filter(|binding| !only_in_test_code || file.is_test_at(binding.range.start_byte))
            .filter_map(|binding| {
                rust_module_file(&file.file, &binding.module_path, |path| {
                    positions.get(path).copied()
                })
            })
            .collect()
    };
    let mut queue: Vec<usize> = positions
        .values()
        .flat_map(|position| declared_modules(&files[*position], true))
        .collect();
    queue.sort_unstable();
    let mut found = Vec::new();
    while let Some(position) = queue.pop() {
        if files[position].test_file {
            continue;
        }
        files[position].test_file = true;
        found.push(position);
        queue.extend(declared_modules(&files[position], false));
    }
    for position in found {
        let file = &mut files[position];
        for (symbol, facts) in file.symbols.iter_mut().zip(&file.link_facts) {
            if facts.test_entry_in_test_code {
                symbol.test_role = TestRole::Entry;
            } else if symbol.test_role == TestRole::None {
                symbol.test_role = TestRole::Support;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;
    use crate::index::extract::extract_file;
    use crate::index::model::SymbolKind;
    use crate::parser::Parser;

    fn facts(language: Language, file: &str, source: &str) -> FileSymbols {
        let parsed = Parser::new()
            .parse(source.as_bytes(), language, Path::new(file))
            .unwrap();
        extract_file(&parsed, file)
    }

    /// The role of each symbol that is not a field, by name.
    fn roles(language: Language, file: &str, source: &str) -> BTreeMap<String, TestRole> {
        facts(language, file, source)
            .symbols
            .into_iter()
            .filter(|symbol| symbol.kind != SymbolKind::Field)
            .map(|symbol| (symbol.name, symbol.test_role))
            .collect()
    }

    fn assert_roles(found: &BTreeMap<String, TestRole>, expected: &[(&str, TestRole)]) {
        for (name, role) in expected {
            assert_eq!(
                found.get(*name),
                Some(role),
                "the role of `{name}` in {found:?}"
            );
        }
    }

    use TestRole::{Entry, None as NotTest, Support};

    #[test]
    fn test_cfg_predicate_requires_test_only_when_each_build_with_it_is_a_test_build() {
        assert!(cfg_requires_test("test"));
        assert!(cfg_requires_test("all(test, feature = \"a,b\")"));
        assert!(cfg_requires_test("all(unix, all(test))"));
        assert!(cfg_requires_test("any(test)"));
        assert!(!cfg_requires_test("any(test, feature = \"test-support\")"));
        assert!(!cfg_requires_test("not(test)"));
        assert!(!cfg_requires_test("feature = \"test\""));
        assert!(!cfg_requires_test("all(not(test), unix)"));
    }

    /// The first version of `cfg_requires_test`. It is correct, but it
    /// recurses once for each level and scans the text again at each level.
    fn cfg_requires_test_by_recursion(predicate: &str) -> bool {
        let predicate = predicate.trim();
        if predicate == "test" {
            return true;
        }
        let Some(open) = predicate.find('(') else {
            return false;
        };
        if !predicate.ends_with(')') {
            return false;
        }
        let members = split_arguments_for_reference(&predicate[open + 1..predicate.len() - 1]);
        match predicate[..open].trim() {
            "all" => members
                .iter()
                .any(|member| cfg_requires_test_by_recursion(member)),
            "any" => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| cfg_requires_test_by_recursion(member))
            }
            _ => false,
        }
    }

    fn split_arguments_for_reference(text: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut start = 0;
        for (position, character) in text.char_indices() {
            if in_string {
                match character {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match character {
                '"' => in_string = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(text[start..position].trim());
                    start = position + 1;
                }
                _ => {}
            }
        }
        let last = text[start..].trim();
        if !last.is_empty() {
            parts.push(last);
        }
        parts
    }

    #[test]
    fn test_cfg_predicate_has_the_answer_of_the_recursive_reference() {
        let leaves = [
            "test",
            " test ",
            "unix",
            "feature = \"a,b)\"",
            "feature = \"test\"",
            "not(test)",
            "",
        ];
        let mut predicates: Vec<String> = leaves.iter().map(|leaf| leaf.to_string()).collect();
        for _ in 0..2 {
            let mut next = predicates.clone();
            for operator in ["all", "any", "not", "cfg", "all ", "x = all"] {
                for first in &predicates {
                    next.push(format!("{operator}({first})"));
                    for second in leaves {
                        next.push(format!("{operator}({first}, {second})"));
                        next.push(format!("{operator}({second},{first})"));
                    }
                }
            }
            predicates = next;
        }
        let malformed = [
            "all(test",
            "all(test))",
            "all(test) x",
            "all(test), all(test)",
            "any(test,)",
            "any(test,,test)",
            "all()",
            "any()",
            ")",
            "all(test)(test)",
            "all([test, unix])",
            "all({test}, test)",
            "all(\"a\\\"(\", test)",
        ];
        predicates.extend(malformed.iter().map(|text| text.to_string()));
        for predicate in &predicates {
            assert_eq!(
                cfg_requires_test(predicate),
                cfg_requires_test_by_recursion(predicate),
                "{predicate}"
            );
        }
    }

    fn nested_cfg(depth: usize) -> String {
        format!("{}test{}", "all(".repeat(depth), ")".repeat(depth))
    }

    #[test]
    fn test_deeply_nested_cfg_predicate_does_not_overflow_the_stack() {
        assert!(cfg_requires_test(&nested_cfg(200_000)));
        let mut negated = nested_cfg(200_000);
        negated.insert_str(0, "not(");
        negated.push(')');
        assert!(!cfg_requires_test(&negated));
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_deeply_nested_cfg_predicate_is_read_in_linear_time() {
        crate::index::timing::assert_scales_linearly("nested cfg", 400_000, |depth| {
            let predicate = nested_cfg(depth);
            let started = crate::index::timing::Stopwatch::start();
            assert!(cfg_requires_test(&predicate));
            started.elapsed()
        });
    }

    #[test]
    fn test_rust_test_attributes_make_an_entry() {
        let found = roles(
            Language::Rust,
            "src/lib.rs",
            r#"
#[test]
fn plain() {}

#[tokio::test]
async fn tokio_test() {}

#[gpui::test]
async fn gpui_test() {}

#[rstest]
fn with_rstest() {}

#[test_case(1; "one")]
fn with_test_case(value: u32) {}

#[testlog(tokio::test)]
async fn with_wrapped_path() {}

#[bench]
fn a_benchmark(b: &mut Bencher) {}

#[test]
/// The documentation is between the attribute and the function.
// A comment is there too.
#[ignore]
fn with_doc_comment_between() {}

#[doc = "names crate::test in a string"]
#[inline]
fn production() {}

#[perf]
fn unknown_attribute_outside_test_code() {}

fn test_only_by_name() {}
"#,
        );
        assert_roles(
            &found,
            &[
                ("plain", Entry),
                ("tokio_test", Entry),
                ("gpui_test", Entry),
                ("with_rstest", Entry),
                ("with_test_case", Entry),
                ("with_wrapped_path", Entry),
                ("a_benchmark", Entry),
                ("with_doc_comment_between", Entry),
                ("production", NotTest),
                ("unknown_attribute_outside_test_code", NotTest),
                ("test_only_by_name", NotTest),
            ],
        );
    }

    #[test]
    fn test_rust_cfg_test_module_is_test_code_with_other_attributes_and_comments_between() {
        let found = roles(
            Language::Rust,
            "src/lib.rs",
            r#"
fn production() {}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "tests run a process"
)]
// A comment.
mod tests {
    fn helper() {}

    #[perf]
    fn registered_by_an_unknown_attribute() {}

    #[inline]
    fn inert_attribute() {}

    mod nested {
        fn nested_helper() {}
    }
}

#[allow(dead_code)]
#[cfg(test)]
fn test_only_function() {}

#[cfg(not(test))]
mod not_tests {
    fn real() {}
}

#[cfg(any(test, feature = "test-support"))]
fn test_support_feature() {}

#[cfg(test)]
impl Thing {
    fn method_for_tests(&self) {}
}
"#,
        );
        assert_roles(
            &found,
            &[
                ("production", NotTest),
                ("tests", Support),
                ("helper", Support),
                ("registered_by_an_unknown_attribute", Entry),
                ("inert_attribute", Support),
                ("nested_helper", Support),
                ("test_only_function", Support),
                ("real", NotTest),
                ("test_support_feature", NotTest),
                ("method_for_tests", Support),
            ],
        );
    }

    #[test]
    fn test_rust_inner_cfg_test_and_test_directories() {
        let inner = roles(
            Language::Rust,
            "src/checks.rs",
            "#![cfg(test)]\n\nfn helper() {}\n",
        );
        assert_roles(&inner, &[("helper", Support)]);
        for file in ["tests/api.rs", "benches/speed.rs"] {
            let found = roles(
                Language::Rust,
                file,
                "fn helper() {}\n#[test]\nfn check() {}\n",
            );
            assert_roles(&found, &[("helper", Support), ("check", Entry)]);
        }
    }

    #[test]
    fn test_rust_file_name_with_tests_is_not_test_code_without_a_declaration() {
        // `xtask/src/tasks/sandbox_tests.rs` can be the command that runs tests.
        for file in ["src/tasks/sandbox_tests.rs", "src/tasks/tests.rs"] {
            let found = roles(Language::Rust, file, "pub fn run_sandbox_tests() {}\n");
            assert_roles(&found, &[("run_sandbox_tests", NotTest)]);
        }
    }

    #[test]
    fn test_rust_module_file_of_a_cfg_test_declaration_is_test_code() {
        let mut files = vec![
            facts(
                Language::Rust,
                "src/lib.rs",
                "#[cfg(test)]\nmod checks;\nmod real;\nfn production() {}\n",
            ),
            facts(
                Language::Rust,
                "src/checks.rs",
                "mod deep;\nfn helper() {}\n#[perf]\nfn measured() {}\n",
            ),
            facts(
                Language::Rust,
                "src/checks/deep.rs",
                "fn deep_helper() {}\n",
            ),
            facts(Language::Rust, "src/real.rs", "fn real() {}\n"),
        ];
        files.sort_by(|left, right| left.file.cmp(&right.file));
        crate::index::extract::link_files(&mut files);
        let manifests = crate::index::imports::Manifests::default();
        crate::index::imports::resolve_imports(&mut files, &manifests);
        link_files(&mut files);
        let role = |file: &str, name: &str| {
            files
                .iter()
                .find(|facts| facts.file == file)
                .and_then(|facts| facts.symbols.iter().find(|symbol| symbol.name == name))
                .map(|symbol| symbol.test_role)
        };
        assert_eq!(role("src/checks.rs", "helper"), Some(Support));
        assert_eq!(role("src/checks.rs", "measured"), Some(Entry));
        assert_eq!(role("src/checks/deep.rs", "deep_helper"), Some(Support));
        assert_eq!(role("src/real.rs", "real"), Some(NotTest));
        assert_eq!(role("src/lib.rs", "production"), Some(NotTest));
        let checks = files.iter().find(|facts| facts.file == "src/checks.rs");
        assert!(checks.is_some_and(|facts| facts.is_test_at(0)));
    }

    #[test]
    fn test_module_file_of_a_declaration_follows_the_path_rules_of_rust() {
        let files = [
            "crate/src/lib.rs",
            "crate/src/a.rs",
            "crate/src/a/b.rs",
            "crate/src/a/c/mod.rs",
            "crate/src/editor.rs",
            "crate/src/panel.rs",
            "crate/tests/api.rs",
            "crate/tests/common/mod.rs",
            "other/src/a.rs",
        ];
        let find = |declaring: &str, name: &str| {
            rust_module_file(declaring, name, |path| {
                files.iter().copied().find(|file| *file == path)
            })
        };
        assert_eq!(find("crate/src/lib.rs", "a"), Some("crate/src/a.rs"));
        assert_eq!(find("crate/src/a.rs", "b"), Some("crate/src/a/b.rs"));
        assert_eq!(find("crate/src/a.rs", "c"), Some("crate/src/a/c/mod.rs"));
        // A crate root can have a different name, and a test file is a crate root.
        assert_eq!(
            find("crate/src/editor.rs", "panel"),
            Some("crate/src/panel.rs")
        );
        assert_eq!(
            find("crate/tests/api.rs", "common"),
            Some("crate/tests/common/mod.rs")
        );
        assert_eq!(find("crate/src/lib.rs", "missing"), None);
        assert_eq!(find("crate/tests/common/mod.rs", "a"), None);
    }

    #[test]
    fn test_module_declaration_in_test_code_does_not_follow_a_wrong_import_target() {
        let mut files = vec![
            facts(
                Language::Rust,
                "service/tests/sim/mod.rs",
                "pub mod service;\npub mod helper;\n",
            ),
            facts(
                Language::Rust,
                "service/tests/sim/helper.rs",
                "pub fn helper() {}\n",
            ),
            facts(
                Language::Rust,
                "service/src/lib.rs",
                "pub mod manager;\npub fn production() {}\n",
            ),
            facts(
                Language::Rust,
                "service/src/manager.rs",
                "pub fn manage() {}\n",
            ),
        ];
        files.sort_by(|left, right| left.file.cmp(&right.file));
        // The import resolution can give the crate root for the name `service`.
        for file in &mut files {
            for binding in &mut file.imports {
                if binding.module_path == "service" {
                    binding.target_file = crate::index::model::ImportTarget::Resolved(
                        "service/src/lib.rs".to_string(),
                    );
                }
            }
        }
        link_files(&mut files);
        let is_test_file = |name: &str| {
            files
                .iter()
                .find(|facts| facts.file == name)
                .map(|facts| facts.test_file)
        };
        assert_eq!(is_test_file("service/src/lib.rs"), Some(false));
        assert_eq!(is_test_file("service/src/manager.rs"), Some(false));
        assert_eq!(is_test_file("service/tests/sim/helper.rs"), Some(true));
    }

    #[test]
    fn test_python_fixtures_test_cases_and_test_files() {
        let production = roles(
            Language::Python,
            "app/service.py",
            r#"
import unittest
import pytest

@pytest.fixture
def database():
    return 1

@pytest.fixture(scope="session")
def session_database():
    return 1

class ServiceCheck(unittest.TestCase):
    def setUp(self):
        pass

    def test_runs(self):
        self.helper()

    def helper(self):
        pass

class Service:
    def test_connection(self):
        pass

def test_like_name():
    pass
"#,
        );
        assert_roles(
            &production,
            &[
                ("database", Entry),
                ("session_database", Entry),
                ("ServiceCheck", Entry),
                ("setUp", Entry),
                ("test_runs", Entry),
                ("helper", Support),
                ("Service", NotTest),
                ("test_connection", NotTest),
                ("test_like_name", NotTest),
            ],
        );
        let test_file = roles(
            Language::Python,
            "tests/test_service.py",
            "def test_runs():\n    pass\n\ndef helper():\n    pass\n\nclass TestThing:\n    def test_method(self):\n        pass\n",
        );
        assert_roles(
            &test_file,
            &[
                ("test_runs", Entry),
                ("helper", Support),
                ("TestThing", Entry),
                ("test_method", Entry),
            ],
        );
        let conftest = roles(
            Language::Python,
            "conftest.py",
            "def pytest_configure(config):\n    pass\n",
        );
        assert_roles(&conftest, &[("pytest_configure", Entry)]);
    }

    #[test]
    fn test_ecma_callbacks_of_the_test_functions_are_test_code() {
        let source = r#"
function production() {}

describe("a suite", () => {
  function helper() {}
  const build = () => 1;
  beforeEach(function prepare() {});
  it("works", function named() { helper(); });
  test.each([1, 2])("case %s", (value) => {});
});

it.only("focused", async () => {
  const local = () => 2;
});

const it2 = [1].map((value) => value);
function iterate(it) { return it.next(); }
hooks.beforeEach(function ofAnObject() {});
test(function withoutTitle() {});
"#;
        for (language, file) in [
            (Language::JavaScript, "src/a.js"),
            (Language::Jsx, "src/a.jsx"),
            (Language::TypeScript, "src/a.ts"),
            (Language::Tsx, "src/a.tsx"),
        ] {
            let found = roles(language, file, source);
            assert_roles(
                &found,
                &[
                    ("production", NotTest),
                    ("helper", Support),
                    ("build", Support),
                    ("local", Support),
                    ("iterate", NotTest),
                ],
            );
            for name in ["ofAnObject", "withoutTitle"] {
                assert_ne!(found.get(name), Some(&Entry), "{name} in {language:?}");
                assert_ne!(found.get(name), Some(&Support), "{name} in {language:?}");
            }
            for name in ["prepare", "named"] {
                assert!(
                    found.get(name).is_none_or(|role| *role == Entry),
                    "{name} in {language:?}: {found:?}"
                );
            }
            let facts = facts(language, file, source);
            let focused = source.find("const local").unwrap();
            assert!(facts.is_test_at(focused));
            assert!(!facts.is_test_at(source.find("function production").unwrap()));
        }
        let by_path = roles(
            Language::TypeScript,
            "src/a.test.ts",
            "function helper() {}\n",
        );
        assert_roles(&by_path, &[("helper", Support)]);
    }

    #[test]
    fn test_ranges_of_a_tree_agree_with_the_index() {
        let source = "fn production() {}\n\n#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    fn helper() {}\n}\n\n#[cfg(not(test))]\nmod real {}\n\n#[test]\nfn check() {}\n";
        let parsed = Parser::new()
            .parse(source.as_bytes(), Language::Rust, Path::new("src/lib.rs"))
            .unwrap();
        let ranges = test_ranges_of_tree(parsed.root_node(), source.as_bytes(), Language::Rust);
        assert_eq!(
            ranges,
            facts(Language::Rust, "src/lib.rs", source).test_ranges
        );
        let starts: Vec<&str> = ranges
            .iter()
            .map(|range| source[range.clone()].lines().next().unwrap())
            .collect();
        assert_eq!(starts, ["mod tests {", "fn check() {}"]);
    }

    #[test]
    fn test_line_ranges_agree_with_the_byte_ranges() {
        let source = "fn production() {}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n\n#[test]\nfn check() {}\n";
        let found = facts(Language::Rust, "src/lib.rs", source);
        assert_eq!(found.test_lines, [(4, 6), (9, 9)]);
        for (line, expected) in [(1, false), (3, false), (4, true), (5, true), (6, true)] {
            assert_eq!(found.is_test_at_line(line), expected, "line {line}");
        }
        assert!(!found.is_test_at_line(7));
        assert!(found.is_test_at_line(9));
        let parsed = Parser::new()
            .parse(source.as_bytes(), Language::Rust, Path::new("src/lib.rs"))
            .unwrap();
        assert_eq!(
            test_lines_of_tree(parsed.root_node(), source.as_bytes(), Language::Rust),
            found.test_lines
        );
        let in_test_file = facts(Language::Rust, "tests/api.rs", "fn helper() {}\n");
        assert!(in_test_file.is_test_at_line(1));
    }

    #[test]
    fn test_go_test_functions_of_a_test_file() {
        let source = "package a\n\nfunc TestRun(t *testing.T) {}\nfunc BenchmarkRun(b *testing.B) {}\nfunc FuzzRun(f *testing.F) {}\nfunc ExampleRun() {}\nfunc Testify() {}\nfunc helper() {}\nfunc TestMain(m *testing.M) {}\n";
        let test_file = roles(Language::Go, "a_test.go", source);
        assert_roles(
            &test_file,
            &[
                ("TestRun", Entry),
                ("BenchmarkRun", Entry),
                ("FuzzRun", Entry),
                ("ExampleRun", Entry),
                ("TestMain", Entry),
                ("Testify", Support),
                ("helper", Support),
            ],
        );
        let production = roles(Language::Go, "a.go", source);
        assert_roles(&production, &[("TestRun", NotTest), ("helper", NotTest)]);
    }

    #[test]
    fn test_java_annotated_methods_and_their_class() {
        let found = roles(
            Language::Java,
            "src/main/java/Checks.java",
            r#"
class Checks {
    @Test
    void runs() {}

    @org.junit.jupiter.api.ParameterizedTest
    @ValueSource(ints = {1})
    void withValues(int value) {}

    @BeforeEach
    void prepare() {}

    void helper() {}
}

class Service {
    @Override
    public String toString() { return ""; }

    void testConnection() {}
}
"#,
        );
        assert_roles(
            &found,
            &[
                ("Checks", Support),
                ("runs", Entry),
                ("withValues", Entry),
                ("prepare", Entry),
                ("helper", Support),
                ("Service", NotTest),
                ("toString", NotTest),
                ("testConnection", NotTest),
            ],
        );
    }

    #[test]
    fn test_java_aspect_advice_is_not_a_test_hook() {
        // AspectJ has `@Before` and `@After` with a pointcut. JUnit 4 has
        // them without an argument.
        let found = roles(
            Language::Java,
            "src/main/java/Audit.java",
            r#"
@Aspect
class Audit {
    @Before("execution(* app.Service.*(..))")
    void record() {}

    @After(value = "execution(* app.Service.*(..))")
    void close() {}
}

class Legacy {
    @Before
    public void prepare() {}

    @After
    public void clean() {}
}
"#,
        );
        assert_roles(
            &found,
            &[
                ("Audit", NotTest),
                ("record", NotTest),
                ("close", NotTest),
                ("Legacy", Support),
                ("prepare", Entry),
                ("clean", Entry),
            ],
        );
    }

    #[test]
    fn test_csharp_attributed_methods_and_their_class() {
        let found = roles(
            Language::CSharp,
            "src/Checks.cs",
            r#"
[TestFixture]
public class Checks {
    [Test]
    public void Runs() {}

    [Xunit.Fact]
    public void IsFact() {}

    [Theory, InlineData(1)]
    public void IsTheory(int value) {}

    [TestMethodAttribute]
    public void IsTestMethod() {}

    public void Helper() {}
}

public class Unmarked {
    [Fact]
    public void Runs2() {}

    void Helper2() {}
}

public class Service {
    [Obsolete]
    public void TestConnection() {}
}
"#,
        );
        assert_roles(
            &found,
            &[
                ("Checks", Entry),
                ("Runs", Entry),
                ("IsFact", Entry),
                ("IsTheory", Entry),
                ("IsTestMethod", Entry),
                ("Helper", Support),
                ("Unmarked", Support),
                ("Runs2", Entry),
                ("Helper2", Support),
                ("Service", NotTest),
                ("TestConnection", NotTest),
            ],
        );
    }

    #[test]
    fn test_ruby_minitest_classes_and_rspec_blocks() {
        let found = roles(
            Language::Ruby,
            "lib/checks.rb",
            r#"
class ServiceTest < Minitest::Test
  def setup
  end

  def test_runs
    helper
  end

  def helper
  end
end

class UsersControllerTest < ActionDispatch::IntegrationTest
  def test_index
  end
end

class Service
  def test_connection
  end
end

RSpec.describe Service do
  def spec_helper
  end

  context "when ready" do
    it "runs" do
    end
  end
end

describe "plain" do
  def plain_helper
  end
end

context "of a different library" do
  def not_a_spec
  end
end
"#,
        );
        assert_roles(
            &found,
            &[
                ("ServiceTest", Entry),
                ("setup", Entry),
                ("test_runs", Entry),
                ("helper", Support),
                ("UsersControllerTest", Entry),
                ("test_index", Entry),
                ("Service", NotTest),
                ("test_connection", NotTest),
                ("spec_helper", Support),
                ("plain_helper", Support),
                ("not_a_spec", NotTest),
            ],
        );
        let by_path = roles(
            Language::Ruby,
            "spec/models/user_spec.rb",
            "def helper\nend\n",
        );
        assert_roles(&by_path, &[("helper", Support)]);
    }

    #[test]
    fn test_php_phpunit_classes_and_marked_methods() {
        let found = roles(
            Language::Php,
            "src/Checks.php",
            r#"<?php
use PHPUnit\Framework\TestCase;
use PHPUnit\Framework\Attributes\Test;

class ServiceTest extends TestCase {
    protected function setUp(): void {}

    public function testRuns(): void { $this->helper(); }

    private function helper(): void {}

    /**
     * @test
     */
    public function it_runs(): void {}

    #[Test]
    public function with_attribute(): void {}
}

class Marked {
    #[\PHPUnit\Framework\Attributes\Test]
    public function marked_method(): void {}

    public function other(): void {}
}

class Service {
    /** Runs the test of the connection. */
    public function testConnection(): void {}
}
"#,
        );
        assert_roles(
            &found,
            &[
                ("ServiceTest", Entry),
                ("setUp", Entry),
                ("testRuns", Entry),
                ("helper", Support),
                ("it_runs", Entry),
                ("with_attribute", Entry),
                ("Marked", Support),
                ("marked_method", Entry),
                ("other", Support),
                ("Service", NotTest),
                ("testConnection", NotTest),
            ],
        );
    }

    #[test]
    fn test_cpp_test_macros_define_test_functions() {
        let source = r#"
#include <gtest/gtest.h>

int helper() { return 1; }

TEST(Suite, Runs) {
    auto local = []() { return 2; };
    EXPECT_EQ(helper(), 1);
}

TEST_F(Fixture, WithFixture) {
    EXPECT_TRUE(true);
}

TEST_P(Parameters, WithParameter) {
    EXPECT_TRUE(GetParam());
}

int TEST_like(int value) { return value; }

class Test {
public:
    Test() { build(); }
};
"#;
        let facts = facts(Language::Cpp, "src/checks.cc", source);
        assert!(facts.is_test_at(source.find("auto local").unwrap()));
        assert!(facts.is_test_at(source.find("EXPECT_TRUE(true)").unwrap()));
        assert!(facts.is_test_at(source.find("EXPECT_TRUE(GetParam").unwrap()));
        assert!(!facts.is_test_at(source.find("return 1").unwrap()));
        assert!(!facts.is_test_at(source.find("return value").unwrap()));
        assert!(!facts.is_test_at(source.find("build();").unwrap()));
        let found = roles(Language::Cpp, "src/checks.cc", source);
        assert_roles(&found, &[("helper", NotTest), ("TEST_like", NotTest)]);
        for name in ["TEST", "TEST_F", "TEST_P"] {
            assert!(
                found.get(name).is_none_or(|role| *role == Entry),
                "{name}: {found:?}"
            );
        }
    }

    #[test]
    fn test_c_and_bash_use_the_path_and_the_test_macros() {
        let c_source = "int helper(void) { return 1; }\n\nTest(suite, runs) {\n    helper();\n}\n";
        let c_facts = facts(Language::C, "src/checks.c", c_source);
        assert!(c_facts.is_test_at(c_source.find("helper();").unwrap()));
        assert!(!c_facts.is_test_at(c_source.find("return 1").unwrap()));
        let c_by_path = roles(
            Language::C,
            "tests/checks.c",
            "int helper(void) { return 1; }\n",
        );
        assert_roles(&c_by_path, &[("helper", Support)]);
        let bash = roles(Language::Bash, "test/run.sh", "helper() {\n  true\n}\n");
        assert_roles(&bash, &[("helper", Support)]);
        let bash_production = roles(Language::Bash, "bin/run.sh", "test_run() {\n  true\n}\n");
        assert_roles(&bash_production, &[("test_run", NotTest)]);
    }

    #[test]
    fn test_test_role_is_absent_from_json_for_production_code() {
        let found = facts(
            Language::Rust,
            "src/lib.rs",
            "fn production() {}\n#[test]\nfn check() {}\n",
        );
        let production = serde_json::to_value(&found.symbols[0]).unwrap();
        assert!(production.get("test_role").is_none());
        let check = serde_json::to_value(&found.symbols[1]).unwrap();
        assert_eq!(check["test_role"], "entry");
    }
}

#[cfg(test)]
mod timing_tests {
    use std::path::Path;

    use super::*;
    use crate::index::timing::assert_scales_linearly;
    use crate::parser::Parser;

    /// The time of the walk that finds the test code, without the parse.
    fn time_of_marks(language: Language, file: &str, source: &str) -> std::time::Duration {
        let parsed = Parser::new()
            .parse(source.as_bytes(), language, Path::new(file))
            .unwrap();
        let started = crate::index::timing::Stopwatch::start();
        let ranges = test_ranges_of_tree(parsed.root_node(), source.as_bytes(), language);
        let elapsed = started.elapsed();
        assert!(!ranges.is_empty(), "{file}");
        elapsed
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_rust_attributes_before_one_test_are_read_in_linear_time() {
        assert_scales_linearly("attributes", 20_000, |size| {
            let source = format!("{}#[test]\nfn check() {{}}\n", "#[inline]\n".repeat(size));
            time_of_marks(Language::Rust, "src/lib.rs", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_rust_tests_in_one_module_are_read_in_linear_time() {
        // A step from a node to its sibling in tree-sitter reads the children of the parent from
        // the first child. The module is large, so that a step for each attribute fails the check.
        assert_scales_linearly("tests", 320_000, |size| {
            let source: String = (0..size)
                .map(|position| format!("#[test]\nfn check_{position}() {{}}\n"))
                .collect();
            time_of_marks(Language::Rust, "src/lib.rs", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_c_blocks_in_a_test_are_read_in_linear_time() {
        assert_scales_linearly("nested blocks", 5_000, |size| {
            let source = format!(
                "TEST(suite, name)\n{{\n{}{}}}\n",
                "if (x) {".repeat(size),
                "}".repeat(size)
            );
            time_of_marks(Language::C, "test.c", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_a_long_chain_of_calls_in_a_test_is_read_in_linear_time() {
        assert_scales_linearly("chain of calls", 5_000, |size| {
            let source = format!(
                "describe('suite', () => {{\n  a{};\n}});\n",
                ".b()".repeat(size)
            );
            time_of_marks(Language::TypeScript, "a.test.ts", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_c_test_macros_are_read_in_linear_time() {
        assert_scales_linearly("macros", 20_000, |size| {
            let source: String = (0..size)
                .map(|position| format!("TEST(Suite, Name{position}) {{\n}}\n"))
                .collect();
            time_of_marks(Language::C, "test/a_test.c", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tests_in_deeply_nested_anonymous_classes_are_read_in_linear_time() {
        assert_scales_linearly("java", 2_000, |depth| {
            let source = format!(
                "class A {{ void m() {{ {}{} }} }}\n",
                "new Object() { @Test void t() { ".repeat(depth),
                "} };".repeat(depth)
            );
            time_of_marks(Language::Java, "src/A.java", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tests_in_deeply_nested_ruby_blocks_are_read_in_linear_time() {
        assert_scales_linearly("ruby", 2_000, |depth| {
            let source = format!(
                "class ATest < Minitest::Test\n{}{}end\n",
                "run do\ndef test_a\nend\n".repeat(depth),
                "end\n".repeat(depth)
            );
            time_of_marks(Language::Ruby, "test/a_test.rb", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tests_in_deeply_nested_structs_are_read_in_linear_time() {
        assert_scales_linearly("csharp", 2_000, |depth| {
            let source = format!(
                "{}{}",
                "struct S { [Test] void M() {} ".repeat(depth),
                "}".repeat(depth)
            );
            time_of_marks(Language::CSharp, "Tests/A.cs", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_methods_of_a_class_with_many_bases_are_read_in_linear_time() {
        assert_scales_linearly("python", 10_000, |size| {
            let bases: Vec<String> = (0..size)
                .map(|position| format!("Base{position}"))
                .collect();
            let methods: String = (0..size)
                .map(|position| format!("    def test_{position}(self):\n        pass\n"))
                .collect();
            let source = format!("class ATest({}, TestCase):\n{methods}", bases.join(", "));
            time_of_marks(Language::Python, "src/a.py", &source)
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_methods_of_a_class_with_a_long_superclass_are_read_in_linear_time() {
        assert_scales_linearly("ruby", 20_000, |size| {
            let methods: String = (0..size)
                .map(|position| format!("def test_{position}\nend\n"))
                .collect();
            let source = format!(
                "class ATest < {}TestCase\n{methods}end\n",
                "A".repeat(size * 10)
            );
            time_of_marks(Language::Ruby, "lib/a.rb", &source)
        });
    }
}
