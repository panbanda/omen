//! Ground-truth comparison of omen's static understanding against reviewed labels.
//!
//! A fixture case is a directory of real source files plus a `labels.json`. An
//! adapter turns what omen reports for those files into [`Observed`];
//! [`compare`] turns labels and observations into one [`Assertion`] per checked
//! fact. Facts omen is known to get wrong are listed in an expected-failure
//! ledger, which [`check_ledger`] keeps honest in both directions: an unlisted
//! failure and a listed pass are both errors.
//!
//! Labels are author-reviewed development data. Passing them is not evidence of
//! held-out accuracy.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::invalid;
use crate::core::{Language, Result};

pub const LABELS_VERSION: u32 = 1;

/// Categories every language must label or declare not applicable.
pub const SYMBOL_CATEGORIES: [&str; 9] = [
    "free_function",
    "method",
    "constructor",
    "nested_function",
    "closure",
    "type",
    "same_name_methods",
    "overload",
    "visibility",
];
pub const CALL_CATEGORIES: [&str; 13] = [
    "bare_local",
    "cross_file_import",
    "alias",
    "self_receiver",
    "declared_type_receiver",
    "static_qualified",
    "constructor",
    "callback",
    "shadowing",
    "foreign_language",
    "ambiguous",
    "external",
    "wrong_receiver",
];
pub const IMPORT_CATEGORIES: [&str; 8] = [
    "relative",
    "package",
    "alias",
    "named",
    "wildcard",
    "grouped",
    "re_export",
    "external",
];

/// The sink categories. Each language labels each category in each scenario,
/// or declares the category or the scenario not applicable.
pub const SINK_CATEGORIES: [&str; 10] = [
    "command_execution",
    "code_evaluation",
    "unsafe_deserialization",
    "sql_string_construction",
    "weak_hash",
    "weak_cipher",
    "tls_verification_disabled",
    "hardcoded_credential",
    "memory_unsafe",
    "unsafe_html_output",
];
pub const SINK_SCENARIOS: [&str; 7] = [
    "real_call",
    "aliased_import",
    "safe_variant",
    "name_in_string",
    "name_in_comment",
    "same_name_symbol",
    "in_test",
];

/// The categories of the test-code labels. `entry_by_syntax` is a definition
/// or a block that a test runner calls because of an attribute, an
/// annotation, a decorator, a macro, a superclass, or a test function.
/// `entry_by_name` is a definition that a runner calls because of its name in
/// a test file. `support` is other code in test code. `not_test` is
/// production code that has a name or a shape of a test.
pub const TEST_CATEGORIES: [&str; 4] = ["entry_by_syntax", "entry_by_name", "support", "not_test"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Symbols,
    Calls,
    Imports,
    Sinks,
    Tests,
}

impl Dimension {
    pub const ALL: [Self; 5] = [
        Self::Symbols,
        Self::Calls,
        Self::Imports,
        Self::Sinks,
        Self::Tests,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symbols => "symbols",
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::Sinks => "sinks",
            Self::Tests => "tests",
        }
    }

    pub fn categories(self) -> &'static [&'static str] {
        match self {
            Self::Symbols => &SYMBOL_CATEGORIES,
            Self::Calls => &CALL_CATEGORIES,
            Self::Imports => &IMPORT_CATEGORIES,
            Self::Sinks => &SINK_CATEGORIES,
            Self::Tests => &TEST_CATEGORIES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Labels {
    pub version: u32,
    pub language: Language,
    pub split: String,
    pub oracle: String,
    #[serde(default)]
    pub not_applicable: Vec<NotApplicable>,
    /// When set, a symbol or import omen reports that no label names is a failure.
    pub closed_world: bool,
    #[serde(default)]
    pub symbols: Vec<SymbolLabel>,
    #[serde(default)]
    pub calls: Vec<CallLabel>,
    #[serde(default)]
    pub imports: Vec<ImportLabel>,
    /// When true, a sink that omen reports and that no label names is a failure. When absent, the value of `closed_world` applies.
    #[serde(default)]
    pub sinks_closed_world: Option<bool>,
    #[serde(default)]
    pub sinks: Vec<SinkLabel>,
    /// When true, an entry point that omen reports and that no label names is a failure.
    /// Without the setting, the entry points follow `closed_world` in a case that has an
    /// entry point label, and a case without an entry point label does not check them.
    #[serde(default)]
    pub entry_points_closed_world: Option<bool>,
    #[serde(default)]
    pub entry_points: Vec<EntryPointLabel>,
    #[serde(default)]
    pub paths: Vec<PathLabel>,
    #[serde(default)]
    pub tests: Vec<TestLabel>,
    #[serde(default)]
    pub references: Vec<ReferenceLabel>,
}

/// The relation of a definition or of a line to test code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestRoleLabel {
    /// A test runner calls the definition.
    Entry,
    /// The definition or the line is in test code, and no runner calls it by its syntax.
    Support,
    /// The definition or the line is not test code.
    None,
}

impl TestRoleLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Support => "support",
            Self::None => "none",
        }
    }
}

/// One definition or one line, with its relation to test code.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestLabel {
    pub label: String,
    /// One of `TEST_CATEGORIES`.
    pub category: String,
    pub file: String,
    /// The start line of the definition. Without a `name`, a line of code.
    pub line: u32,
    /// The name of the definition. Without it, the label is for the line: a
    /// block without a name, as the callback of `it` is, has no symbol.
    #[serde(default)]
    pub name: Option<String>,
    pub role: TestRoleLabel,
}

/// One use of a function as a value, as the argument `worker` in
/// `spawn(worker)` is. The use is not a call.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceLabel {
    pub label: String,
    /// The key of the symbol whose body has the use.
    pub user: String,
    pub line: u32,
    /// The name of the function as the source has it, without a receiver or a path.
    pub name: String,
    /// The keys of the functions to which the use has an edge. An empty list
    /// means that omen must not report an edge.
    pub expected: Vec<String>,
}

/// One shortest path from an entry point to a sink. An entry point and a sink are
/// given by the file and the line on which they start.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathLabel {
    pub label: String,
    pub entry_file: String,
    pub entry_line: u32,
    pub sink_file: String,
    pub sink_line: u32,
    /// `false` means that omen must not report a path between the two.
    #[serde(default = "flagged_by_default")]
    pub flagged: bool,
    /// The keys of the symbols on the path, from the handler to the caller of the sink.
    #[serde(default)]
    pub steps: Option<Vec<String>>,
    /// The weakest basis of an edge of the path. A path of one symbol has none.
    #[serde(default)]
    pub weakest_basis: Option<String>,
    /// The number of shortest paths.
    #[serde(default)]
    pub path_count: Option<u64>,
    /// The guards of the entry point of the path.
    #[serde(default)]
    pub guards: Option<Vec<String>>,
    /// The keys of symbols that each path to a sink goes through.
    #[serde(default)]
    pub choke_points: Vec<String>,
    /// The number of edges of the path that are function references.
    #[serde(default)]
    pub function_references: Option<usize>,
}

/// One entry point: a handler that the code declares or registers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPointLabel {
    pub label: String,
    pub file: String,
    /// The line on which the declaration, the registration call, or the file starts.
    pub line: u32,
    /// One of `http_handler`, `cli_main`, `message_consumer`, `scheduled_job`,
    /// `serverless_handler`, and `ffi_export`.
    pub kind: String,
    pub framework: String,
    #[serde(default)]
    pub method: Option<String>,
    /// Omitted: omen does not check the path. `null`: the entry point must have no resolved
    /// path. A string: the path must be equal to it.
    #[serde(default, deserialize_with = "present")]
    pub path: Option<Option<String>>,
    #[serde(default)]
    pub parameters: Vec<String>,
    /// The declared guards, in the order in which omen reports them.
    #[serde(default)]
    pub guards: Vec<String>,
    /// The key of the handler symbol. `null` states that the entry point has no symbol. Without
    /// the field, the symbol is not checked.
    #[serde(default, deserialize_with = "present")]
    pub symbol: Option<Option<String>>,
    /// Whether the guard list is complete. Without the field, the state is not checked.
    #[serde(default)]
    pub guards_complete: Option<bool>,
    /// The reason when the guard list is not complete. `null` states that omen gives no reason.
    /// Without the field, the reason is not checked.
    #[serde(default, deserialize_with = "present")]
    pub guards_unknown_reason: Option<Option<String>>,
}

/// Reads a field that is in the label, also when its value is `null`. Thus a label can tell
/// `null` from a field that it does not have.
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// One call, member access, or literal that is a sink or must not be one.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkLabel {
    pub label: String,
    /// One of `SINK_CATEGORIES`.
    pub category: String,
    /// One of `SINK_SCENARIOS`.
    pub scenario: String,
    pub file: String,
    /// The line on which the sink starts.
    pub line: u32,
    /// `false` means that omen must not report a sink of this category on this line.
    #[serde(default = "flagged_by_default")]
    pub flagged: bool,
    /// The evidence that omen gives, for example `imported_symbol`. Only a reported sink has it.
    #[serde(default)]
    pub basis: Option<String>,
    /// The sink is in test code.
    #[serde(default)]
    pub in_test: bool,
    /// The sink is in a file with the header of generated code.
    #[serde(default)]
    pub in_generated: bool,
    /// All arguments of the call are literals. Only a label that states it gets the check.
    #[serde(default)]
    pub constant_argument: Option<bool>,
}

fn flagged_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotApplicable {
    pub dimension: Dimension,
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolLabel {
    pub label: String,
    pub category: String,
    pub file: String,
    pub name: String,
    pub kind: Kind,
    pub container: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub exported: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallLabel {
    pub label: String,
    pub category: String,
    /// Label of the symbol whose body contains the call.
    pub caller: String,
    pub line: u32,
    pub callee_text: String,
    /// Labels of every definition this call binds to. Empty means the call must
    /// produce no edge (ambiguous, external, or dynamically bound).
    pub expected: Vec<String>,
    /// Labels that must at least be offered as candidates, whether or not an
    /// edge is asserted. Measured separately from edges.
    #[serde(default)]
    pub candidates_required: Vec<String>,
    /// The status that omen must give the call: `external` for a call of a
    /// definition outside the repository, and `not_external` for a call that
    /// a repository symbol can receive. Without the field, the status is not checked.
    #[serde(default)]
    pub status: Option<ExpectedStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedStatus {
    External,
    NotExternal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportLabel {
    pub label: String,
    pub category: String,
    pub file: String,
    pub line: u32,
    pub path: String,
    #[serde(default)]
    pub names: Vec<ImportedName>,
    /// Repository file the import resolves to; `None` for an external dependency.
    pub required_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedName {
    pub name: String,
    pub alias: Option<String>,
}

/// A definition identified the only way that is stable across id formats.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub file: String,
    pub name: String,
    pub start_line: u32,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.name, self.start_line)
    }
}

/// What omen reported for one fixture case. `None` means omen does not report
/// that fact at all, which is a failure rather than a match.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    pub symbols: Vec<ObservedSymbol>,
    pub calls: Vec<ObservedCall>,
    pub imports: Vec<ObservedImport>,
    pub sinks: Vec<ObservedSink>,
    pub entry_points: Vec<ObservedEntryPoint>,
    pub paths: Vec<ObservedPath>,
    pub test_symbols: Vec<ObservedTestSymbol>,
    pub test_lines: Vec<ObservedTestLines>,
    pub references: Vec<ObservedReference>,
}

/// The relation to test code that omen reports for one definition.
#[derive(Debug, Clone)]
pub struct ObservedTestSymbol {
    pub location: Location,
    /// `entry`, `support`, or `none`.
    pub role: String,
}

/// A range of lines that omen reports as test code.
#[derive(Debug, Clone)]
pub struct ObservedTestLines {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone)]
pub struct ObservedPath {
    pub entry_file: String,
    pub entry_line: u32,
    pub sink_file: String,
    pub sink_line: u32,
    pub steps: Vec<String>,
    pub weakest_basis: Option<String>,
    pub path_count: u64,
    pub guards: Vec<String>,
    /// The keys of the symbols on this path that are choke points for the sink of this path.
    pub choke_points: Vec<String>,
    /// The number of edges of the path that are function references.
    pub function_references: usize,
}

/// One use of a function as a value that omen reported.
#[derive(Debug, Clone)]
pub struct ObservedReference {
    /// The key of the symbol whose body has the use.
    pub user: String,
    pub line: u32,
    pub name: String,
    /// The keys of the functions to which the use has an edge.
    pub edges: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ObservedEntryPoint {
    pub file: String,
    pub line: u32,
    pub kind: String,
    pub framework: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub parameters: Vec<String>,
    pub guards: Vec<String>,
    pub symbol: Option<String>,
    pub guards_complete: bool,
    pub guards_unknown_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObservedSink {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub category: String,
    pub basis: String,
    pub in_test: bool,
    pub in_generated: bool,
    pub constant_argument: bool,
}

#[derive(Debug, Clone)]
pub struct ObservedSymbol {
    pub location: Location,
    pub kind: Option<Kind>,
    pub container: Option<String>,
    pub end_line: u32,
    pub exported: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ObservedCall {
    pub caller: Location,
    pub call: String,
    /// The line and the column at which the callee name starts.
    pub position: (u32, u32),
    pub edges: Vec<Location>,
    pub candidates: Vec<Location>,
    /// True for each report of the call that has the status `External`.
    pub external: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct ObservedImport {
    pub file: String,
    pub line: u32,
    pub path: String,
    pub names: Vec<ImportedName>,
    pub resolved_files: Vec<String>,
}

/// One checked fact. The id is `<language>/<case>/<dimension>/<label>/<check>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEntry {
    pub id: String,
    pub reason: String,
    pub tracking: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// An assertion failed and the ledger does not list it.
    UnexpectedFailure { id: String, detail: String },
    /// The ledger lists an assertion that now passes; remove the entry.
    UnexpectedPass { id: String },
    /// The ledger lists an id no assertion produced.
    UnknownEntry { id: String },
    /// Ledger ids must be strictly ascending, so duplicates and reordering show in review.
    Unsorted { id: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedFailure { id, detail } => write!(f, "FAIL {id}: {detail}"),
            Self::UnexpectedPass { id } => {
                write!(f, "FIXED {id}: passes now, remove it from the ledger")
            }
            Self::UnknownEntry { id } => write!(f, "STALE {id}: no such assertion"),
            Self::Unsorted { id } => write!(f, "ORDER {id}: ledger ids must be strictly ascending"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub passed: usize,
    pub expected_failures: usize,
}

fn assertion(id: String, failure: Option<String>) -> Assertion {
    Assertion {
        id,
        passed: failure.is_none(),
        detail: failure.unwrap_or_default(),
    }
}

/// The method or function name of a call expression, without receiver or path.
fn bare_name(call: &str) -> &str {
    call.rsplit(['.', ':', '>', '\\'])
        .next()
        .unwrap_or(call)
        .trim()
}

fn join<T: std::fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn unique_labels<'a>(
    case: &str,
    dimension: Dimension,
    labels: impl Iterator<Item = &'a str>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for label in labels {
        if label.is_empty() || label.contains('/') {
            return Err(invalid(format!(
                "{case}: {} label '{label}' must be non-empty without '/'",
                dimension.as_str()
            )));
        }
        if !seen.insert(label) {
            return Err(invalid(format!(
                "{case}: duplicate {} label '{label}'",
                dimension.as_str()
            )));
        }
    }
    Ok(())
}

fn symbol_locations(case: &str, labels: &Labels) -> Result<BTreeMap<String, Location>> {
    if labels.version != LABELS_VERSION {
        return Err(invalid(format!(
            "{case}: unsupported labels version {}",
            labels.version
        )));
    }
    unique_labels(
        case,
        Dimension::Symbols,
        labels.symbols.iter().map(|s| s.label.as_str()),
    )?;
    unique_labels(
        case,
        Dimension::Calls,
        labels.calls.iter().map(|c| c.label.as_str()),
    )?;
    unique_labels(
        case,
        Dimension::Imports,
        labels.imports.iter().map(|i| i.label.as_str()),
    )?;
    unique_labels(
        case,
        Dimension::Sinks,
        labels.sinks.iter().map(|s| s.label.as_str()),
    )?;
    unique_labels(
        case,
        Dimension::Tests,
        labels.tests.iter().map(|test| test.label.as_str()),
    )?;
    for test in &labels.tests {
        if !TEST_CATEGORIES.contains(&test.category.as_str()) {
            return Err(invalid(format!(
                "{case}: test label '{}' has the unknown category '{}'",
                test.label, test.category
            )));
        }
        if test.name.is_none() && test.role == TestRoleLabel::Entry {
            return Err(invalid(format!(
                "{case}: test label '{}' is for a line, thus its role is `support` or `none`",
                test.label
            )));
        }
    }
    for sink in &labels.sinks {
        if !SINK_CATEGORIES.contains(&sink.category.as_str()) {
            return Err(invalid(format!(
                "{case}: sink '{}' has the unknown category '{}'",
                sink.label, sink.category
            )));
        }
        if !SINK_SCENARIOS.contains(&sink.scenario.as_str()) {
            return Err(invalid(format!(
                "{case}: sink '{}' has the unknown scenario '{}'",
                sink.label, sink.scenario
            )));
        }
        if !sink.flagged
            && (sink.basis.is_some()
                || sink.in_test
                || sink.in_generated
                || sink.constant_argument.is_some())
        {
            return Err(invalid(format!(
                "{case}: sink '{}' is not flagged, thus it has no basis and no flags",
                sink.label
            )));
        }
    }
    Ok(labels
        .symbols
        .iter()
        .map(|symbol| {
            (
                symbol.label.clone(),
                Location {
                    file: symbol.file.clone(),
                    name: symbol.name.clone(),
                    start_line: symbol.start_line,
                },
            )
        })
        .collect())
}

fn compare_symbols(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    let mut labelled = BTreeSet::new();
    for symbol in &labels.symbols {
        let location = Location {
            file: symbol.file.clone(),
            name: symbol.name.clone(),
            start_line: symbol.start_line,
        };
        let found = observed
            .symbols
            .iter()
            .find(|candidate| candidate.location == location);
        let check = |name: &str, failure: Option<String>| {
            assertion(
                format!("{case}/symbols/{}/{name}", symbol.label),
                match found {
                    None => Some(format!("{location} was not extracted")),
                    Some(_) => failure,
                },
            )
        };
        let mismatch = |expected: String, actual: String| {
            (expected != actual).then(|| format!("expected {expected}, got {actual}"))
        };
        out.push(check("found", None));
        out.push(check(
            "kind",
            found.and_then(|o| {
                mismatch(format!("{:?}", Some(symbol.kind)), format!("{:?}", o.kind))
            }),
        ));
        out.push(check(
            "container",
            found.and_then(|o| {
                mismatch(
                    format!("{:?}", symbol.container),
                    format!("{:?}", o.container),
                )
            }),
        ));
        out.push(check(
            "range",
            found.and_then(|o| {
                mismatch(
                    format!("end line {}", symbol.end_line),
                    format!("end line {}", o.end_line),
                )
            }),
        ));
        out.push(check(
            "exported",
            found.and_then(|o| {
                mismatch(
                    format!("{:?}", Some(symbol.exported)),
                    format!("{:?}", o.exported),
                )
            }),
        ));
        labelled.insert(location);
    }
    if labels.closed_world {
        let unlabelled: BTreeSet<&Location> = observed
            .symbols
            .iter()
            .map(|symbol| &symbol.location)
            .filter(|location| !labelled.contains(*location))
            .collect();
        for location in unlabelled {
            out.push(assertion(
                format!("{case}/symbols/unlabelled:{location}/absent"),
                Some("extracted but not a labelled definition".to_string()),
            ));
        }
    }
}

fn compare_calls(
    case: &str,
    labels: &Labels,
    observed: &Observed,
    locations: &BTreeMap<String, Location>,
    out: &mut Vec<Assertion>,
) -> Result<()> {
    let lookup = |call: &CallLabel, label: &String| {
        locations.get(label).cloned().ok_or_else(|| {
            invalid(format!(
                "{case}: call '{}' names unknown symbol label '{label}'",
                call.label
            ))
        })
    };
    // Omen reports one result for each callee name in a caller. Two labels with
    // the same caller and callee name would each get the results of the other.
    let mut labelled: BTreeMap<(Location, &str), &str> = BTreeMap::new();
    for call in &labels.calls {
        let caller = lookup(call, &call.caller)?;
        let key = (caller.clone(), bare_name(&call.callee_text));
        if let Some(first) = labelled.insert(key, &call.label) {
            return Err(invalid(format!(
                "{case}: calls '{first}' and '{}' have the same caller and callee name",
                call.label
            )));
        }
        let expected = call
            .expected
            .iter()
            .map(|label| lookup(call, label))
            .collect::<Result<BTreeSet<_>>>()?;
        let candidates_required = call
            .candidates_required
            .iter()
            .map(|label| lookup(call, label))
            .collect::<Result<BTreeSet<_>>>()?;

        let matching: Vec<&ObservedCall> = observed
            .calls
            .iter()
            .filter(|o| o.caller == caller && bare_name(&o.call) == bare_name(&call.callee_text))
            .collect();
        // The same rule holds for a call without a label: the label would get the
        // results of each call of the name in the caller.
        let positions: BTreeSet<(u32, u32)> = matching.iter().map(|o| o.position).collect();
        if positions.len() > 1 {
            let places: Vec<String> = positions
                .iter()
                .map(|(line, column)| format!("{line}:{column}"))
                .collect();
            return Err(invalid(format!(
                "{case}: call '{}' has the callee name '{}' at {} in its caller. Omen reports \
                 one result for each callee name in a caller, thus the label gets the results \
                 of each of these calls. Move the labelled call to a caller of its own",
                call.label,
                bare_name(&call.callee_text),
                places.join(", ")
            )));
        }
        let edges: BTreeSet<Location> = matching
            .iter()
            .flat_map(|o| o.edges.iter().cloned())
            .collect();
        let offered: BTreeSet<Location> = matching
            .iter()
            .flat_map(|o| o.candidates.iter().chain(&o.edges).cloned())
            .collect();

        let false_edges: Vec<_> = edges.difference(&expected).collect();
        let missed_edges: Vec<_> = expected.difference(&edges).collect();
        let prefix = format!("{case}/calls/{}", call.label);
        out.push(assertion(
            format!("{prefix}/no_false_edge"),
            (!false_edges.is_empty()).then(|| format!("false edge to {}", join(false_edges))),
        ));
        out.push(assertion(
            format!("{prefix}/no_missed_edge"),
            (!missed_edges.is_empty()).then(|| format!("no edge to {}", join(missed_edges))),
        ));
        if let Some(status) = call.status {
            let reported = matching.iter().flat_map(|o| o.external.iter().copied());
            let wrong =
                reported.filter(|external| *external != (status == ExpectedStatus::External));
            let count = wrong.count();
            out.push(assertion(
                format!("{prefix}/status"),
                (matching.is_empty() || count > 0).then(|| match status {
                    ExpectedStatus::External => "the call is not reported as external".to_string(),
                    ExpectedStatus::NotExternal => "the call is reported as external".to_string(),
                }),
            ));
        }
        if !candidates_required.is_empty() {
            let missing: Vec<_> = candidates_required.difference(&offered).collect();
            out.push(assertion(
                format!("{prefix}/candidates"),
                (!missing.is_empty()).then(|| format!("not offered: {}", join(missing))),
            ));
        }
    }
    Ok(())
}

fn compare_imports(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    let mut labelled = BTreeSet::new();
    for import in &labels.imports {
        let key = (import.file.as_str(), import.line, import.path.as_str());
        let matching: Vec<&ObservedImport> = observed
            .imports
            .iter()
            .filter(|o| (o.file.as_str(), o.line, o.path.as_str()) == key)
            .collect();
        let found = matching.first().copied();
        let check = |name: &str, failure: Option<String>| {
            assertion(
                format!("{case}/imports/{}/{name}", import.label),
                match found {
                    None => {
                        let same_line = observed
                            .imports
                            .iter()
                            .filter(|o| o.file == import.file && o.line == import.line)
                            .map(|o| format!("'{}'", o.path));
                        Some(format!(
                            "'{}' not extracted at {}:{}; extracted there: [{}]",
                            import.path,
                            import.file,
                            import.line,
                            join(same_line)
                        ))
                    }
                    Some(_) => failure,
                },
            )
        };
        out.push(check(
            "found",
            (matching.len() > 1).then(|| format!("extracted {} times", matching.len())),
        ));
        out.push(check(
            "names",
            found.and_then(|o| {
                let expected: BTreeSet<_> = import.names.iter().collect();
                let actual: BTreeSet<_> = o.names.iter().collect();
                (expected != actual).then(|| format!("expected {expected:?}, got {actual:?}"))
            }),
        ));
        out.push(check(
            "target",
            found.and_then(|o| {
                let expected: BTreeSet<_> = import.required_file.iter().collect();
                let actual: BTreeSet<_> = o.resolved_files.iter().collect();
                (expected != actual).then(|| format!("expected {expected:?}, got {actual:?}"))
            }),
        ));
        labelled.insert(key);
    }
    if labels.closed_world {
        let unlabelled: BTreeSet<_> = observed
            .imports
            .iter()
            .map(|o| (o.file.as_str(), o.line, o.path.as_str()))
            .filter(|key| !labelled.contains(key))
            .collect();
        for (file, line, path) in unlabelled {
            out.push(assertion(
                // An incorrect path can contain a full statement. Keep the ID on one line.
                format!(
                    "{case}/imports/unlabelled:{file}:{line}:{}/absent",
                    path.escape_debug()
                ),
                Some("extracted but not a labelled import".to_string()),
            ));
        }
    }
}

/// A label takes one reported entry point, as a sink label does.
fn compare_entry_points(
    case: &str,
    labels: &Labels,
    observed: &Observed,
    out: &mut Vec<Assertion>,
) {
    let mut entries: Vec<&ObservedEntryPoint> = observed.entry_points.iter().collect();
    entries.sort_by(|left, right| {
        (&left.file, left.line, &left.kind, &left.path).cmp(&(
            &right.file,
            right.line,
            &right.kind,
            &right.path,
        ))
    });
    let mut taken = vec![false; entries.len()];
    for label in &labels.entry_points {
        let found = entries
            .iter()
            .enumerate()
            .find(|(position, entry)| {
                !taken[*position]
                    && entry.file == label.file
                    && entry.line == label.line
                    && entry.kind == label.kind
                    && label.path.as_ref().is_none_or(|path| entry.path == *path)
                    && (label.method.is_none() || entry.method == label.method)
            })
            .map(|(position, entry)| {
                taken[position] = true;
                *entry
            });
        let prefix = format!("{case}/entry_points/{}", label.label);
        let check = |name: &str, failure: Option<String>| {
            assertion(
                format!("{prefix}/{name}"),
                match found {
                    None => Some(format!(
                        "no {} entry point at {}:{}",
                        label.kind, label.file, label.line
                    )),
                    Some(_) => failure,
                },
            )
        };
        let mismatch = |expected: String, actual: String| {
            (expected != actual).then(|| format!("expected {expected}, got {actual}"))
        };
        out.push(check("found", None));
        out.push(check(
            "framework",
            found.and_then(|entry| mismatch(label.framework.clone(), entry.framework.clone())),
        ));
        if label.path.is_some() {
            out.push(check(
                "parameters",
                found.and_then(|entry| {
                    mismatch(
                        format!("{:?}", label.parameters),
                        format!("{:?}", entry.parameters),
                    )
                }),
            ));
        }
        out.push(check(
            "guards",
            found.and_then(|entry| {
                mismatch(format!("{:?}", label.guards), format!("{:?}", entry.guards))
            }),
        ));
        if let Some(symbol) = &label.symbol {
            out.push(check(
                "symbol",
                found.and_then(|entry| {
                    mismatch(format!("{symbol:?}"), format!("{:?}", entry.symbol))
                }),
            ));
        }
        if let Some(complete) = label.guards_complete {
            out.push(check(
                "guards_complete",
                found.and_then(|entry| {
                    mismatch(complete.to_string(), entry.guards_complete.to_string())
                }),
            ));
        }
        if let Some(reason) = &label.guards_unknown_reason {
            out.push(check(
                "guards_unknown_reason",
                found.and_then(|entry| {
                    mismatch(
                        format!("{reason:?}"),
                        format!("{:?}", entry.guards_unknown_reason),
                    )
                }),
            ));
        }
    }
    let closed = labels
        .entry_points_closed_world
        .unwrap_or(labels.closed_world && !labels.entry_points.is_empty());
    if closed {
        for (position, entry) in entries.iter().enumerate() {
            if !taken[position] {
                out.push(assertion(
                    format!(
                        "{case}/entry_points/unlabelled:{}:{}:{}/absent",
                        entry.file, entry.line, entry.kind
                    ),
                    Some("reported but not a labelled entry point".to_string()),
                ));
            }
        }
    }
}

/// A label names one path. A path that the label says must not exist fails when omen reports it.
fn compare_paths(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    for label in &labels.paths {
        let found = observed.paths.iter().find(|path| {
            path.entry_file == label.entry_file
                && path.entry_line == label.entry_line
                && path.sink_file == label.sink_file
                && path.sink_line == label.sink_line
        });
        let prefix = format!("{case}/paths/{}", label.label);
        if !label.flagged {
            out.push(assertion(
                format!("{prefix}/absent"),
                found.map(|_| "a path is reported".to_string()),
            ));
            continue;
        }
        let check = |name: &str, failure: Option<String>| {
            assertion(
                format!("{prefix}/{name}"),
                match found {
                    None => Some(format!(
                        "no path from {}:{} to {}:{}",
                        label.entry_file, label.entry_line, label.sink_file, label.sink_line
                    )),
                    Some(_) => failure,
                },
            )
        };
        let mismatch = |expected: String, actual: String| {
            (expected != actual).then(|| format!("expected {expected}, got {actual}"))
        };
        out.push(check("found", None));
        if let Some(steps) = &label.steps {
            out.push(check(
                "steps",
                found.and_then(|path| mismatch(format!("{steps:?}"), format!("{:?}", path.steps))),
            ));
        }
        if let Some(basis) = &label.weakest_basis {
            out.push(check(
                "weakest_basis",
                found.and_then(|path| {
                    mismatch(
                        basis.clone(),
                        path.weakest_basis.clone().unwrap_or_default(),
                    )
                }),
            ));
        }
        if let Some(count) = label.path_count {
            out.push(check(
                "path_count",
                found.and_then(|path| mismatch(count.to_string(), path.path_count.to_string())),
            ));
        }
        if let Some(count) = label.function_references {
            out.push(check(
                "function_references",
                found.and_then(|path| {
                    mismatch(count.to_string(), path.function_references.to_string())
                }),
            ));
        }
        if let Some(guards) = &label.guards {
            out.push(check(
                "guards",
                found
                    .and_then(|path| mismatch(format!("{guards:?}"), format!("{:?}", path.guards))),
            ));
        }
        if !label.choke_points.is_empty() {
            out.push(check(
                "choke_points",
                found.and_then(|path| {
                    let missing: Vec<&String> = label
                        .choke_points
                        .iter()
                        .filter(|symbol| !path.choke_points.contains(symbol))
                        .collect();
                    (!missing.is_empty())
                        .then(|| format!("not choke points of the sink: {missing:?}"))
                }),
            ));
        }
    }
}

/// A label names one use: a symbol, a line, and a name. The edges of the use must be the
/// expected functions. A use that omen does not report has no edge.
fn compare_references(
    case: &str,
    labels: &Labels,
    observed: &Observed,
    out: &mut Vec<Assertion>,
) -> Result<()> {
    let names = labels.references.iter().map(|label| label.label.as_str());
    let mut seen = BTreeSet::new();
    for name in names {
        if name.is_empty() || name.contains('/') || !seen.insert(name) {
            return Err(invalid(format!(
                "{case}: references label '{name}' must be unique and non-empty without '/'"
            )));
        }
    }
    for label in &labels.references {
        let expected: BTreeSet<&String> = label.expected.iter().collect();
        let edges: BTreeSet<&String> = observed
            .references
            .iter()
            .filter(|found| {
                found.user == label.user && found.line == label.line && found.name == label.name
            })
            .flat_map(|found| &found.edges)
            .collect();
        let false_edges: Vec<_> = edges.difference(&expected).collect();
        let missed_edges: Vec<_> = expected.difference(&edges).collect();
        let prefix = format!("{case}/references/{}", label.label);
        out.push(assertion(
            format!("{prefix}/no_false_edge"),
            (!false_edges.is_empty()).then(|| format!("false edge to {}", join(false_edges))),
        ));
        out.push(assertion(
            format!("{prefix}/no_missed_edge"),
            (!missed_edges.is_empty()).then(|| format!("no edge to {}", join(missed_edges))),
        ));
    }
    Ok(())
}

/// A label takes one reported sink. A second label on the same line and category needs a
/// second sink. A sink that no label takes is unlabelled.
fn compare_sinks(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    let mut sinks: Vec<&ObservedSink> = observed.sinks.iter().collect();
    sinks.sort_by(|left, right| {
        (&left.file, left.line, left.column, &left.category).cmp(&(
            &right.file,
            right.line,
            right.column,
            &right.category,
        ))
    });
    let mut taken = vec![false; sinks.len()];
    let same_place = |sink: &ObservedSink, label: &SinkLabel| {
        sink.file == label.file && sink.line == label.line && sink.category == label.category
    };
    for label in labels.sinks.iter().filter(|label| label.flagged) {
        let found = sinks
            .iter()
            .enumerate()
            .find(|(position, sink)| !taken[*position] && same_place(sink, label))
            .map(|(position, sink)| {
                taken[position] = true;
                *sink
            });
        let prefix = format!("{case}/sinks/{}", label.label);
        let check = |name: &str, failure: Option<String>| {
            assertion(
                format!("{prefix}/{name}"),
                match found {
                    None => Some(format!(
                        "no {} sink at {}:{}",
                        label.category, label.file, label.line
                    )),
                    Some(_) => failure,
                },
            )
        };
        let mismatch = |expected: String, actual: String| {
            (expected != actual).then(|| format!("expected {expected}, got {actual}"))
        };
        out.push(check("found", None));
        if let Some(basis) = &label.basis {
            out.push(check(
                "basis",
                found.and_then(|sink| mismatch(basis.clone(), sink.basis.clone())),
            ));
        }
        out.push(check(
            "in_test",
            found.and_then(|sink| mismatch(label.in_test.to_string(), sink.in_test.to_string())),
        ));
        out.push(check(
            "in_generated",
            found.and_then(|sink| {
                mismatch(
                    label.in_generated.to_string(),
                    sink.in_generated.to_string(),
                )
            }),
        ));
        if let Some(constant) = label.constant_argument {
            out.push(check(
                "constant_argument",
                found.and_then(|sink| {
                    mismatch(constant.to_string(), sink.constant_argument.to_string())
                }),
            ));
        }
    }
    let mut forbidden = Vec::new();
    for label in labels.sinks.iter().filter(|label| !label.flagged) {
        let found = sinks.iter().find(|sink| same_place(sink, label));
        out.push(assertion(
            format!("{case}/sinks/{}/absent", label.label),
            found.map(|sink| format!("reported with the basis {}", sink.basis)),
        ));
        forbidden.push(label);
    }
    if sinks_closed_world(labels) {
        for (position, sink) in sinks.iter().enumerate() {
            let covered = taken[position] || forbidden.iter().any(|label| same_place(sink, label));
            if !covered {
                out.push(assertion(
                    format!(
                        "{case}/sinks/unlabelled:{}:{}:{}:{}/absent",
                        sink.file, sink.line, sink.column, sink.category
                    ),
                    Some("reported but not a labelled sink".to_string()),
                ));
            }
        }
    }
}

fn compare_tests(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    for test in &labels.tests {
        let actual = match &test.name {
            Some(name) => {
                let location = Location {
                    file: test.file.clone(),
                    name: name.clone(),
                    start_line: test.line,
                };
                observed
                    .test_symbols
                    .iter()
                    .find(|candidate| candidate.location == location)
                    .map(|found| found.role.clone())
                    .ok_or_else(|| format!("{location} was not extracted"))
            }
            None => {
                let in_test_code = observed.test_lines.iter().any(|lines| {
                    lines.file == test.file
                        && lines.start_line <= test.line
                        && test.line <= lines.end_line
                });
                Ok(if in_test_code { "support" } else { "none" }.to_string())
            }
        };
        let failure = match actual {
            Ok(actual) if actual == test.role.as_str() => None,
            Ok(actual) => Some(format!("expected {}, got {actual}", test.role.as_str())),
            Err(missing) => Some(missing),
        };
        out.push(assertion(
            format!("{case}/tests/{}/role", test.label),
            failure,
        ));
    }
}

/// Without a setting, the sinks follow `closed_world`. Thus an old case does not ignore a sink.
fn sinks_closed_world(labels: &Labels) -> bool {
    labels.sinks_closed_world.unwrap_or(labels.closed_world)
}

/// Check every label of one case against what omen reported.
///
/// `case` is `<language>/<case directory>`. Errors mean the labels themselves
/// are malformed, never that omen was wrong.
pub fn compare(case: &str, labels: &Labels, observed: &Observed) -> Result<Vec<Assertion>> {
    let locations = symbol_locations(case, labels)?;
    let mut assertions = Vec::new();
    compare_symbols(case, labels, observed, &mut assertions);
    compare_calls(case, labels, observed, &locations, &mut assertions)?;
    compare_imports(case, labels, observed, &mut assertions);
    compare_sinks(case, labels, observed, &mut assertions);
    compare_entry_points(case, labels, observed, &mut assertions);
    compare_paths(case, labels, observed, &mut assertions);
    compare_tests(case, labels, observed, &mut assertions);
    compare_references(case, labels, observed, &mut assertions)?;
    Ok(assertions)
}

/// Assertion ids that follow from the labels alone, whatever omen reports.
pub fn label_assertion_ids(case: &str, labels: &Labels) -> Result<BTreeSet<String>> {
    let open_world = Labels {
        closed_world: false,
        ..labels.clone()
    };
    Ok(compare(case, &open_world, &Observed::default())?
        .into_iter()
        .map(|assertion| assertion.id)
        .collect())
}

/// Enforce the ledger in both directions.
pub fn check_ledger(assertions: &[Assertion], ledger: &[LedgerEntry]) -> Vec<Violation> {
    let mut violations = Vec::new();
    for pair in ledger.windows(2) {
        if pair[0].id >= pair[1].id {
            violations.push(Violation::Unsorted {
                id: pair[1].id.clone(),
            });
        }
    }
    let listed: BTreeSet<&str> = ledger.iter().map(|entry| entry.id.as_str()).collect();
    let mut produced = BTreeSet::new();
    for assertion in assertions {
        produced.insert(assertion.id.as_str());
        match (assertion.passed, listed.contains(assertion.id.as_str())) {
            (false, false) => violations.push(Violation::UnexpectedFailure {
                id: assertion.id.clone(),
                detail: assertion.detail.clone(),
            }),
            (true, true) => violations.push(Violation::UnexpectedPass {
                id: assertion.id.clone(),
            }),
            _ => {}
        }
    }
    for id in listed.difference(&produced) {
        violations.push(Violation::UnknownEntry { id: id.to_string() });
    }
    violations
}

fn case_of(id: &str) -> &str {
    match id.match_indices('/').nth(1) {
        Some((end, _)) => &id[..end],
        None => id,
    }
}

/// Ledger ids added since the base revision for facts the base already checked.
///
/// A new expected failure is legitimate only when its label is new too.
/// `base_ids` comes from [`label_assertion_ids`] over the base fixtures and
/// `base_cases` lists the base's `<language>/<case>` directories, which is how
/// closed-world ids (not derivable from labels) are recognised.
pub fn ledger_growth(
    base_ids: &BTreeSet<String>,
    base_cases: &BTreeSet<String>,
    base_ledger: &[LedgerEntry],
    ledger: &[LedgerEntry],
) -> Vec<String> {
    let before: BTreeSet<&str> = base_ledger.iter().map(|entry| entry.id.as_str()).collect();
    ledger
        .iter()
        .map(|entry| entry.id.as_str())
        .filter(|id| !before.contains(id))
        .filter(|id| {
            base_ids.contains(*id)
                || (id.contains("/unlabelled:") && base_cases.contains(case_of(id)))
        })
        .map(str::to_string)
        .collect()
}

/// Required categories a language neither labels nor declares not applicable.
pub fn coverage_gaps(cases: &[&Labels]) -> Vec<String> {
    let mut gaps = Vec::new();
    for dimension in Dimension::ALL {
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for labels in cases {
            match dimension {
                Dimension::Symbols => {
                    covered.extend(labels.symbols.iter().map(|s| s.category.clone()))
                }
                Dimension::Calls => covered.extend(labels.calls.iter().map(|c| c.category.clone())),
                Dimension::Imports => {
                    covered.extend(labels.imports.iter().map(|i| i.category.clone()))
                }
                Dimension::Sinks => covered.extend(
                    labels
                        .sinks
                        .iter()
                        .map(|sink| format!("{}/{}", sink.category, sink.scenario)),
                ),
                Dimension::Tests => {
                    covered.extend(labels.tests.iter().map(|test| test.category.clone()))
                }
            }
            covered.extend(
                labels
                    .not_applicable
                    .iter()
                    .filter(|entry| entry.dimension == dimension && !entry.reason.trim().is_empty())
                    .map(|entry| entry.category.clone()),
            );
        }
        if dimension == Dimension::Sinks {
            // An entry for a whole sink category covers each scenario of the category.
            for category in SINK_CATEGORIES {
                for scenario in SINK_SCENARIOS {
                    let key = format!("{category}/{scenario}");
                    if !covered.contains(&key) && !covered.contains(category) {
                        gaps.push(format!("sinks/{key}"));
                    }
                }
            }
        } else {
            gaps.extend(
                dimension
                    .categories()
                    .iter()
                    .filter(|category| !covered.contains(**category))
                    .map(|category| format!("{}/{category}", dimension.as_str())),
            );
        }
    }
    gaps
}

/// Per-language, per-sink-category counts. Never pooled into one figure.
pub fn summarize_sinks(assertions: &[Assertion]) -> BTreeMap<(String, String), Tally> {
    let mut table: BTreeMap<(String, String), Tally> = BTreeMap::new();
    for assertion in assertions {
        let Some((head, rest)) = assertion.id.split_once("/sinks/") else {
            continue;
        };
        let language = head.split('/').next().unwrap_or_default();
        // A label is `<category>.<scenario>`. An unlabelled sink is
        // `unlabelled:<file>:<line>:<category>`, and a file path can contain `/`.
        let category = match rest.strip_prefix("unlabelled:") {
            Some(unlabelled) => unlabelled
                .trim_end_matches("/absent")
                .rsplit(':')
                .next()
                .unwrap_or_default(),
            None => rest.split(['.', '/']).next().unwrap_or_default(),
        };
        let tally = table
            .entry((language.to_string(), category.to_string()))
            .or_default();
        if assertion.passed {
            tally.passed += 1;
        } else {
            tally.expected_failures += 1;
        }
    }
    table
}

pub fn render_sink_table(table: &BTreeMap<(String, String), Tally>) -> String {
    let mut out = format!(
        "{:<12} {:<26} {:>6} {:>18}\n",
        "language", "sink category", "pass", "expected failures"
    );
    for ((language, category), tally) in table {
        out.push_str(&format!(
            "{language:<12} {category:<26} {:>6} {:>18}\n",
            tally.passed, tally.expected_failures
        ));
    }
    out
}

/// Per-language, per-dimension counts. Never pooled into one figure.
pub fn summarize(assertions: &[Assertion]) -> BTreeMap<(String, String), Tally> {
    let mut table: BTreeMap<(String, String), Tally> = BTreeMap::new();
    for assertion in assertions {
        let mut parts = assertion.id.split('/');
        let language = parts.next().unwrap_or_default().to_string();
        let dimension = parts.nth(1).unwrap_or_default().to_string();
        let tally = table.entry((language, dimension)).or_default();
        if assertion.passed {
            tally.passed += 1;
        } else {
            tally.expected_failures += 1;
        }
    }
    table
}

pub fn render_table(table: &BTreeMap<(String, String), Tally>) -> String {
    let mut out = format!(
        "{:<12} {:<8} {:>6} {:>18}\n",
        "language", "dim", "pass", "expected failures"
    );
    for ((language, dimension), tally) in table {
        out.push_str(&format!(
            "{language:<12} {dimension:<8} {:>6} {:>18}\n",
            tally.passed, tally.expected_failures
        ));
    }
    out
}

/// Ledger matching the current failures, keeping reasons already written.
pub fn bless(assertions: &[Assertion], ledger: &[LedgerEntry], tracking: &str) -> Vec<LedgerEntry> {
    let existing: BTreeMap<&str, &LedgerEntry> = ledger
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let blessed: BTreeMap<&str, LedgerEntry> = assertions
        .iter()
        .filter(|assertion| !assertion.passed)
        .map(|assertion| {
            let entry = existing.get(assertion.id.as_str()).map_or_else(
                || LedgerEntry {
                    id: assertion.id.clone(),
                    reason: assertion.detail.clone(),
                    tracking: tracking.to_string(),
                },
                |entry| (*entry).clone(),
            );
            (assertion.id.as_str(), entry)
        })
        .collect();
    blessed.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(file: &str, name: &str, start_line: u32) -> Location {
        Location {
            file: file.to_string(),
            name: name.to_string(),
            start_line,
        }
    }

    fn labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "language": "rust",
            "split": "development",
            "oracle": "author-reviewed, not executed",
            "closed_world": true,
            "symbols": [
                {"label": "helper", "category": "free_function", "file": "a.rs", "name": "helper",
                 "kind": "function", "container": null, "start_line": 1, "end_line": 1, "exported": true},
                {"label": "area", "category": "method", "file": "a.rs", "name": "area",
                 "kind": "method", "container": "Circle", "start_line": 5, "end_line": 7, "exported": false},
                {"label": "caller", "category": "free_function", "file": "main.rs", "name": "caller",
                 "kind": "function", "container": null, "start_line": 3, "end_line": 6, "exported": false}
            ],
            "calls": [
                {"label": "bare", "category": "cross_file_import", "caller": "caller", "line": 4,
                 "callee_text": "helper", "expected": ["helper"]},
                {"label": "receiver", "category": "declared_type_receiver", "caller": "caller", "line": 5,
                 "callee_text": "c.area", "expected": [], "candidates_required": ["area"]}
            ],
            "imports": [
                {"label": "use_helper", "category": "named", "file": "main.rs", "line": 1,
                 "path": "crate::a::helper", "names": [{"name": "helper", "alias": null}],
                 "required_file": "a.rs"}
            ]
        }))
        .unwrap()
    }

    fn symbol(
        location: Location,
        kind: Kind,
        container: Option<&str>,
        end_line: u32,
        exported: bool,
    ) -> ObservedSymbol {
        ObservedSymbol {
            location,
            kind: Some(kind),
            container: container.map(str::to_string),
            end_line,
            exported: Some(exported),
        }
    }

    fn correct() -> Observed {
        Observed {
            symbols: vec![
                symbol(location("a.rs", "helper", 1), Kind::Function, None, 1, true),
                symbol(
                    location("a.rs", "area", 5),
                    Kind::Method,
                    Some("Circle"),
                    7,
                    false,
                ),
                symbol(
                    location("main.rs", "caller", 3),
                    Kind::Function,
                    None,
                    6,
                    false,
                ),
            ],
            calls: vec![
                ObservedCall {
                    caller: location("main.rs", "caller", 3),
                    call: "helper".to_string(),
                    position: (4, 4),
                    edges: vec![location("a.rs", "helper", 1)],
                    candidates: vec![location("a.rs", "helper", 1)],
                    external: vec![false],
                },
                ObservedCall {
                    caller: location("main.rs", "caller", 3),
                    call: "area".to_string(),
                    position: (5, 6),
                    edges: vec![],
                    candidates: vec![location("a.rs", "area", 5)],
                    external: vec![false],
                },
            ],
            sinks: vec![],
            entry_points: vec![],
            paths: vec![],
            references: vec![],
            imports: vec![ObservedImport {
                file: "main.rs".to_string(),
                line: 1,
                path: "crate::a::helper".to_string(),
                names: vec![ImportedName {
                    name: "helper".to_string(),
                    alias: None,
                }],
                resolved_files: vec!["a.rs".to_string()],
            }],
            ..Observed::default()
        }
    }

    fn failures(observed: &Observed) -> Vec<String> {
        compare("rust/basics", &labels(), observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    fn entry(id: &str) -> LedgerEntry {
        LedgerEntry {
            id: id.to_string(),
            reason: "known".to_string(),
            tracking: "t".to_string(),
        }
    }

    fn result(id: &str, passed: bool) -> Assertion {
        Assertion {
            id: id.to_string(),
            passed,
            detail: if passed {
                String::new()
            } else {
                "why".to_string()
            },
        }
    }

    #[test]
    fn correct_observation_passes_every_assertion() {
        let assertions = compare("rust/basics", &labels(), &correct()).unwrap();
        assert_eq!(failures(&correct()), Vec::<String>::new());
        // 3 symbols x 5 checks, bare call x 2, receiver call x 3, import x 3.
        assert_eq!(assertions.len(), 15 + 2 + 3 + 3);
    }

    #[test]
    fn missing_symbol_fails_all_of_its_checks() {
        let mut observed = correct();
        observed.symbols.remove(1);
        assert_eq!(
            failures(&observed),
            [
                "rust/basics/symbols/area/found",
                "rust/basics/symbols/area/kind",
                "rust/basics/symbols/area/container",
                "rust/basics/symbols/area/range",
                "rust/basics/symbols/area/exported",
            ]
        );
    }

    #[test]
    fn unreported_kind_and_export_are_failures_not_matches() {
        let mut observed = correct();
        observed.symbols[1].kind = None;
        observed.symbols[1].exported = None;
        observed.symbols[1].container = None;
        observed.symbols[1].end_line = 6;
        assert_eq!(
            failures(&observed),
            [
                "rust/basics/symbols/area/kind",
                "rust/basics/symbols/area/container",
                "rust/basics/symbols/area/range",
                "rust/basics/symbols/area/exported",
            ]
        );
    }

    #[test]
    fn closed_world_rejects_unlabelled_symbols_and_imports() {
        let mut observed = correct();
        observed.symbols.push(symbol(
            location("a.rs", "ghost", 9),
            Kind::Function,
            None,
            9,
            true,
        ));
        let mut duplicate = observed.imports[0].clone();
        duplicate.path = "\"two\nlines\"".to_string();
        observed.imports.push(duplicate);
        assert_eq!(
            failures(&observed),
            [
                "rust/basics/symbols/unlabelled:a.rs:ghost:9/absent",
                r#"rust/basics/imports/unlabelled:main.rs:1:\"two\nlines\"/absent"#,
            ]
        );

        let mut open = labels();
        open.closed_world = false;
        let open_failures = compare("rust/basics", &open, &observed)
            .unwrap()
            .iter()
            .filter(|assertion| !assertion.passed)
            .count();
        assert_eq!(open_failures, 0);
    }

    #[test]
    fn wrong_target_is_both_a_false_and_a_missed_edge() {
        let mut observed = correct();
        observed.calls[0].edges = vec![location("a.rs", "area", 5)];
        assert_eq!(
            failures(&observed),
            [
                "rust/basics/calls/bare/no_false_edge",
                "rust/basics/calls/bare/no_missed_edge",
            ]
        );
    }

    #[test]
    fn call_expected_to_abstain_fails_when_an_edge_is_asserted() {
        let mut observed = correct();
        observed.calls[1].edges = vec![location("a.rs", "area", 5)];
        assert_eq!(
            failures(&observed),
            ["rust/basics/calls/receiver/no_false_edge"]
        );
    }

    #[test]
    fn status_label_checks_each_report_of_the_call() {
        let mut open = labels();
        open.calls[1].status = Some(ExpectedStatus::External);
        let mut observed = correct();
        let failing = |observed: &Observed, labels: &Labels| -> Vec<String> {
            compare("rust/basics", labels, observed)
                .unwrap()
                .into_iter()
                .filter(|assertion| !assertion.passed)
                .map(|assertion| assertion.id)
                .collect()
        };
        assert_eq!(
            failing(&observed, &open),
            ["rust/basics/calls/receiver/status"]
        );
        observed.calls[1].external = vec![true];
        assert!(failing(&observed, &open).is_empty());
        open.calls[1].status = Some(ExpectedStatus::NotExternal);
        assert_eq!(
            failing(&observed, &open),
            ["rust/basics/calls/receiver/status"]
        );
    }

    #[test]
    fn status_label_fails_when_the_call_is_not_reported() {
        let mut open = labels();
        open.calls[1].status = Some(ExpectedStatus::NotExternal);
        let mut observed = correct();
        observed.calls.remove(1);
        let ids: Vec<String> = compare("rust/basics", &open, &observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect();
        assert!(ids.contains(&"rust/basics/calls/receiver/status".to_string()));
    }

    #[test]
    fn candidate_coverage_is_separate_from_edges() {
        let mut observed = correct();
        observed.calls[1].candidates.clear();
        assert_eq!(
            failures(&observed),
            ["rust/basics/calls/receiver/candidates"]
        );
    }

    #[test]
    fn calls_match_on_method_name_whatever_the_receiver_syntax() {
        for call in ["c.area", "Circle::area", "$c->area", "area"] {
            let mut observed = correct();
            observed.calls[1].call = call.to_string();
            observed.calls[1].candidates.clear();
            assert_eq!(
                failures(&observed),
                ["rust/basics/calls/receiver/candidates"],
                "{call}"
            );
        }
        assert_eq!(bare_name("ns\\helper"), "helper");
    }

    #[test]
    fn unextracted_call_is_a_missed_edge() {
        let mut observed = correct();
        observed.calls.remove(0);
        assert_eq!(
            failures(&observed),
            ["rust/basics/calls/bare/no_missed_edge"]
        );
    }

    #[test]
    fn a_call_label_is_rejected_when_its_caller_has_another_call_of_the_name() {
        let mut observed = correct();
        observed.calls.push(ObservedCall {
            caller: location("main.rs", "caller", 3),
            call: "helper".to_string(),
            position: (6, 4),
            edges: vec![location("a.rs", "helper", 1)],
            candidates: vec![location("a.rs", "helper", 1)],
            external: vec![false],
        });
        let error = compare("rust/basics", &labels(), &observed)
            .expect_err("two calls of one callee name in a caller")
            .to_string();
        assert!(
            error.contains("call 'bare'")
                && error.contains("'helper'")
                && error.contains("4:4, 6:4"),
            "{error}"
        );
    }

    #[test]
    fn import_checks_cover_duplicates_names_and_target() {
        let mut observed = correct();
        observed.imports[0].names.clear();
        observed.imports[0].resolved_files.clear();
        observed.imports.push(observed.imports[0].clone());
        assert_eq!(
            failures(&observed),
            [
                "rust/basics/imports/use_helper/found",
                "rust/basics/imports/use_helper/names",
                "rust/basics/imports/use_helper/target",
            ]
        );
    }

    #[test]
    fn import_with_a_different_path_is_not_found_and_reports_what_was() {
        let mut observed = correct();
        observed.imports[0].path = "use crate::a::helper;".to_string();
        let mut open = labels();
        open.closed_world = false;
        let assertions = compare("rust/basics", &open, &observed).unwrap();
        let found = assertions
            .iter()
            .find(|assertion| assertion.id == "rust/basics/imports/use_helper/found")
            .unwrap();
        assert!(!found.passed);
        assert!(
            found.detail.contains("'use crate::a::helper;'"),
            "{}",
            found.detail
        );
    }

    #[test]
    fn external_import_must_not_resolve() {
        let mut external = labels();
        external.imports[0].required_file = None;
        let failing: Vec<_> = compare("rust/basics", &external, &correct())
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect();
        assert_eq!(failing, ["rust/basics/imports/use_helper/target"]);
    }

    #[test]
    fn malformed_labels_are_errors() {
        let mut unknown_caller = labels();
        unknown_caller.calls[0].caller = "nobody".to_string();
        assert!(compare("rust/basics", &unknown_caller, &correct()).is_err());

        let mut unknown_target = labels();
        unknown_target.calls[0].expected = vec!["nobody".to_string()];
        assert!(compare("rust/basics", &unknown_target, &correct()).is_err());

        let mut duplicate = labels();
        duplicate.symbols[1].label = "helper".to_string();
        assert!(compare("rust/basics", &duplicate, &correct()).is_err());

        let mut slash = labels();
        slash.imports[0].label = "a/b".to_string();
        assert!(compare("rust/basics", &slash, &correct()).is_err());

        let mut version = labels();
        version.version = 2;
        assert!(compare("rust/basics", &version, &correct()).is_err());

        let unknown_field = serde_json::json!({
            "version": 1, "language": "rust", "split": "development", "oracle": "x",
            "closed_world": true, "surprise": 1
        });
        assert!(serde_json::from_value::<Labels>(unknown_field).is_err());
    }

    #[test]
    fn two_call_labels_that_omen_cannot_tell_apart_are_an_error() {
        let mut twin = labels();
        twin.calls[1].callee_text = "other.helper".to_string();
        let error = compare("rust/basics", &twin, &correct()).unwrap_err();
        assert!(
            error.to_string().contains("'bare' and 'receiver'"),
            "{error}"
        );
    }

    #[test]
    fn ledger_rejects_unlisted_failure() {
        let violations = check_ledger(&[result("a", false), result("b", true)], &[]);
        assert_eq!(
            violations,
            [Violation::UnexpectedFailure {
                id: "a".to_string(),
                detail: "why".to_string()
            }]
        );
    }

    #[test]
    fn ledger_rejects_listed_assertion_that_passes() {
        let violations = check_ledger(&[result("a", true)], &[entry("a")]);
        assert_eq!(
            violations,
            [Violation::UnexpectedPass {
                id: "a".to_string()
            }]
        );
    }

    #[test]
    fn ledger_rejects_entry_without_an_assertion() {
        let violations = check_ledger(&[result("a", false)], &[entry("a"), entry("gone")]);
        assert_eq!(
            violations,
            [Violation::UnknownEntry {
                id: "gone".to_string()
            }]
        );
    }

    #[test]
    fn ledger_accepts_exactly_the_listed_failures() {
        let assertions = [result("a", false), result("b", true), result("c", false)];
        assert_eq!(check_ledger(&assertions, &[entry("a"), entry("c")]), []);
    }

    #[test]
    fn ledger_must_be_strictly_ascending() {
        let assertions = [result("a", false), result("b", false)];
        assert_eq!(
            check_ledger(&assertions, &[entry("b"), entry("a")]),
            [Violation::Unsorted {
                id: "a".to_string()
            }]
        );
        assert_eq!(
            check_ledger(&assertions, &[entry("a"), entry("a"), entry("b")]),
            [Violation::Unsorted {
                id: "a".to_string()
            }]
        );
    }

    #[test]
    fn violations_render_with_their_action() {
        let rendered: Vec<String> = [
            Violation::UnexpectedFailure {
                id: "a".to_string(),
                detail: "why".to_string(),
            },
            Violation::UnexpectedPass {
                id: "b".to_string(),
            },
            Violation::UnknownEntry {
                id: "c".to_string(),
            },
            Violation::Unsorted {
                id: "d".to_string(),
            },
        ]
        .iter()
        .map(Violation::to_string)
        .collect();
        assert_eq!(rendered[0], "FAIL a: why");
        assert!(rendered[1].starts_with("FIXED b"));
        assert!(rendered[2].starts_with("STALE c"));
        assert!(rendered[3].starts_with("ORDER d"));
    }

    #[test]
    fn ledger_may_grow_only_with_new_labels() {
        let base_ids = label_assertion_ids("rust/basics", &labels()).unwrap();
        assert!(base_ids.contains("rust/basics/calls/bare/no_missed_edge"));
        assert!(base_ids.contains("rust/basics/calls/receiver/candidates"));
        assert!(base_ids.iter().all(|id| !id.contains("unlabelled")));
        let base_cases = BTreeSet::from(["rust/basics".to_string()]);
        let base_ledger = [entry("rust/basics/symbols/area/kind")];

        let grown = [
            entry("go/new/calls/fresh/no_missed_edge"),
            entry("rust/basics/calls/bare/no_missed_edge"),
            entry("rust/basics/calls/brand_new/no_missed_edge"),
            entry("rust/basics/symbols/area/kind"),
            entry("rust/basics/symbols/unlabelled:a.rs:ghost:9/absent"),
            entry("rust/other/symbols/unlabelled:a.rs:ghost:9/absent"),
        ];
        assert_eq!(
            ledger_growth(&base_ids, &base_cases, &base_ledger, &grown),
            [
                "rust/basics/calls/bare/no_missed_edge",
                "rust/basics/symbols/unlabelled:a.rs:ghost:9/absent",
            ]
        );
    }

    #[test]
    fn coverage_requires_every_category_or_a_reasoned_exemption() {
        let mut one = labels();
        let all_gaps = coverage_gaps(&[&one]);
        assert!(all_gaps.contains(&"symbols/constructor".to_string()));
        assert!(all_gaps.contains(&"calls/alias".to_string()));
        assert!(all_gaps.contains(&"imports/wildcard".to_string()));
        assert!(!all_gaps.contains(&"symbols/method".to_string()));
        assert!(!all_gaps.contains(&"calls/cross_file_import".to_string()));
        assert!(!all_gaps.contains(&"imports/named".to_string()));

        one.not_applicable = vec![
            NotApplicable {
                dimension: Dimension::Symbols,
                category: "constructor".to_string(),
                reason: "no constructor syntax".to_string(),
            },
            NotApplicable {
                dimension: Dimension::Calls,
                category: "alias".to_string(),
                reason: "  ".to_string(),
            },
        ];
        let gaps = coverage_gaps(&[&one]);
        assert!(!gaps.contains(&"symbols/constructor".to_string()));
        assert!(
            gaps.contains(&"calls/alias".to_string()),
            "blank reason is no exemption"
        );

        let mut two = labels();
        two.imports[0].category = "wildcard".to_string();
        assert!(!coverage_gaps(&[&one, &two]).contains(&"imports/wildcard".to_string()));
    }

    #[test]
    fn summary_is_per_language_and_dimension() {
        let assertions = [
            result("rust/a/symbols/x/found", true),
            result("rust/b/symbols/y/found", false),
            result("rust/a/calls/z/no_missed_edge", false),
            result("go/a/calls/z/no_missed_edge", true),
        ];
        let table = summarize(&assertions);
        assert_eq!(
            table[&("rust".to_string(), "symbols".to_string())],
            Tally {
                passed: 1,
                expected_failures: 1
            }
        );
        assert_eq!(
            table[&("go".to_string(), "calls".to_string())],
            Tally {
                passed: 1,
                expected_failures: 0
            }
        );
        let rendered = render_table(&table);
        assert_eq!(rendered.lines().count(), 4);
        assert!(rendered.lines().nth(1).unwrap().starts_with("go"));
    }

    #[test]
    fn bless_lists_current_failures_and_keeps_written_reasons() {
        let assertions = [result("b", false), result("a", false), result("c", true)];
        let existing = [
            LedgerEntry {
                id: "a".to_string(),
                reason: "hand-written".to_string(),
                tracking: "issue".to_string(),
            },
            entry("c"),
        ];
        let blessed = bless(&assertions, &existing, "phase-2");
        assert_eq!(blessed.len(), 2);
        assert_eq!(blessed[0], existing[0]);
        assert_eq!(
            blessed[1],
            LedgerEntry {
                id: "b".to_string(),
                reason: "why".to_string(),
                tracking: "phase-2".to_string(),
            }
        );
        assert_eq!(check_ledger(&assertions, &blessed), []);
    }

    fn sink_labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "language": "python",
            "split": "development",
            "oracle": "author-reviewed, not executed",
            "closed_world": false,
            "sinks_closed_world": true,
            "sinks": [
                {"label": "command_execution.real_call", "category": "command_execution",
                 "scenario": "real_call", "file": "sinks.py", "line": 4,
                 "basis": "imported_symbol", "constant_argument": false},
                {"label": "command_execution.safe_variant", "category": "command_execution",
                 "scenario": "safe_variant", "file": "sinks.py", "line": 5, "flagged": false},
                {"label": "command_execution.in_test", "category": "command_execution",
                 "scenario": "in_test", "file": "tests/test_sinks.py", "line": 3, "in_test": true}
            ]
        }))
        .unwrap()
    }

    fn sink(file: &str, line: u32, category: &str) -> ObservedSink {
        ObservedSink {
            file: file.to_string(),
            line,
            column: 4,
            category: category.to_string(),
            basis: "imported_symbol".to_string(),
            in_test: file.starts_with("tests/"),
            in_generated: false,
            constant_argument: false,
        }
    }

    fn correct_sinks() -> Observed {
        Observed {
            sinks: vec![
                sink("sinks.py", 4, "command_execution"),
                sink("tests/test_sinks.py", 3, "command_execution"),
            ],
            ..Observed::default()
        }
    }

    fn sink_failures(observed: &Observed) -> Vec<String> {
        compare("python/security", &sink_labels(), observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    #[test]
    fn correct_sinks_pass_every_assertion() {
        let assertions = compare("python/security", &sink_labels(), &correct_sinks()).unwrap();
        assert_eq!(sink_failures(&correct_sinks()), Vec::<String>::new());
        // The real call has five checks, the safe variant one, and the test call three.
        assert_eq!(assertions.len(), 5 + 1 + 3);
    }

    #[test]
    fn missing_sink_fails_each_of_its_checks() {
        let mut observed = correct_sinks();
        observed.sinks.remove(0);
        assert_eq!(
            sink_failures(&observed),
            [
                "python/security/sinks/command_execution.real_call/found",
                "python/security/sinks/command_execution.real_call/basis",
                "python/security/sinks/command_execution.real_call/in_test",
                "python/security/sinks/command_execution.real_call/in_generated",
                "python/security/sinks/command_execution.real_call/constant_argument",
            ]
        );
    }

    #[test]
    fn sink_that_must_not_be_reported_fails_when_reported() {
        let mut observed = correct_sinks();
        observed
            .sinks
            .push(sink("sinks.py", 5, "command_execution"));
        assert_eq!(
            sink_failures(&observed),
            ["python/security/sinks/command_execution.safe_variant/absent"]
        );
        let mut other_category = correct_sinks();
        other_category.sinks.push(sink("sinks.py", 5, "weak_hash"));
        assert_eq!(
            sink_failures(&other_category),
            ["python/security/sinks/unlabelled:sinks.py:5:4:weak_hash/absent"]
        );
    }

    #[test]
    fn sink_basis_and_flags_are_checked_when_the_label_gives_them() {
        let mut observed = correct_sinks();
        observed.sinks[0].basis = "name_only".to_string();
        observed.sinks[0].in_test = true;
        observed.sinks[1].in_test = false;
        observed.sinks[0].constant_argument = true;
        observed.sinks[1].in_generated = true;
        assert_eq!(
            sink_failures(&observed),
            [
                "python/security/sinks/command_execution.real_call/basis",
                "python/security/sinks/command_execution.real_call/in_test",
                "python/security/sinks/command_execution.real_call/constant_argument",
                "python/security/sinks/command_execution.in_test/in_test",
                "python/security/sinks/command_execution.in_test/in_generated",
            ]
        );
    }

    #[test]
    fn sinks_closed_world_rejects_unlabelled_sinks_only_when_set() {
        let mut observed = correct_sinks();
        observed.sinks.push(sink("sinks.py", 9, "weak_hash"));
        assert_eq!(
            sink_failures(&observed),
            ["python/security/sinks/unlabelled:sinks.py:9:4:weak_hash/absent"]
        );
        let mut open = sink_labels();
        open.sinks_closed_world = Some(false);
        let failing = compare("python/security", &open, &observed)
            .unwrap()
            .iter()
            .filter(|assertion| !assertion.passed)
            .count();
        assert_eq!(failing, 0);
    }

    fn entry_labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1, "language": "python", "split": "development", "oracle": "x",
            "closed_world": true,
            "entry_points": [
                {"label": "update", "file": "app.py", "line": 7, "kind": "http_handler",
                 "framework": "flask", "method": "POST", "path": "/u/<id>",
                 "parameters": ["id"], "guards": ["login_required"]}
            ]
        }))
        .unwrap()
    }

    fn observed_entry() -> ObservedEntryPoint {
        ObservedEntryPoint {
            file: "app.py".to_string(),
            line: 7,
            kind: "http_handler".to_string(),
            framework: "flask".to_string(),
            method: Some("POST".to_string()),
            path: Some("/u/<id>".to_string()),
            parameters: vec!["id".to_string()],
            guards: vec!["login_required".to_string()],
            symbol: Some("app.py#update".to_string()),
            guards_complete: false,
            guards_unknown_reason: Some("layer_not_read".to_string()),
        }
    }

    fn entry_failures(observed: &Observed) -> Vec<String> {
        compare("python/entry_points", &entry_labels(), observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    #[test]
    fn a_correct_entry_point_passes_every_check() {
        let observed = Observed {
            entry_points: vec![observed_entry()],
            ..Observed::default()
        };
        assert_eq!(entry_failures(&observed), Vec::<String>::new());
    }

    #[test]
    fn a_label_checks_the_symbol_and_the_guard_state_only_when_it_has_the_fields() {
        let mut labels = entry_labels();
        let mut wrong = observed_entry();
        wrong.symbol = None;
        wrong.guards_complete = true;
        wrong.guards_unknown_reason = None;
        let observed = Observed {
            entry_points: vec![wrong],
            ..Observed::default()
        };
        let failures = |labels: &Labels| -> Vec<String> {
            compare("python/entry_points", labels, &observed)
                .unwrap()
                .into_iter()
                .filter(|assertion| !assertion.passed)
                .map(|assertion| assertion.id)
                .collect()
        };
        assert_eq!(failures(&labels), Vec::<String>::new());
        labels.entry_points[0].symbol = Some(Some("app.py#update".to_string()));
        labels.entry_points[0].guards_complete = Some(false);
        labels.entry_points[0].guards_unknown_reason = Some(Some("layer_not_read".to_string()));
        assert_eq!(
            failures(&labels),
            [
                "python/entry_points/entry_points/update/symbol",
                "python/entry_points/entry_points/update/guards_complete",
                "python/entry_points/entry_points/update/guards_unknown_reason",
            ]
        );
        let stated: Labels = serde_json::from_value(serde_json::json!({
            "version": 1, "language": "python", "split": "development", "oracle": "x",
            "closed_world": true,
            "entry_points": [
                {"label": "update", "file": "app.py", "line": 7, "kind": "http_handler",
                 "framework": "flask", "method": "POST", "path": "/u/<id>",
                 "parameters": ["id"], "guards": ["login_required"], "symbol": null,
                 "guards_complete": true, "guards_unknown_reason": null}
            ]
        }))
        .unwrap();
        assert_eq!(stated.entry_points[0].symbol, Some(None));
        assert_eq!(failures(&stated), Vec::<String>::new());
    }

    #[test]
    fn a_wrong_framework_parameter_or_guard_is_a_failure() {
        let mut wrong = observed_entry();
        wrong.framework = "fastapi".to_string();
        wrong.parameters.clear();
        wrong.guards.clear();
        let observed = Observed {
            entry_points: vec![wrong],
            ..Observed::default()
        };
        assert_eq!(
            entry_failures(&observed),
            [
                "python/entry_points/entry_points/update/framework",
                "python/entry_points/entry_points/update/parameters",
                "python/entry_points/entry_points/update/guards",
            ]
        );
    }

    #[test]
    fn a_missing_entry_point_fails_and_an_unlabelled_one_fails_when_closed() {
        assert_eq!(
            entry_failures(&Observed::default()).len(),
            4,
            "found, framework, parameters, guards"
        );
        let mut extra = observed_entry();
        extra.line = 20;
        let observed = Observed {
            entry_points: vec![observed_entry(), extra],
            ..Observed::default()
        };
        assert_eq!(
            entry_failures(&observed),
            ["python/entry_points/entry_points/unlabelled:app.py:20:http_handler/absent"]
        );
    }

    #[test]
    fn a_null_path_requires_that_the_entry_point_has_no_path() {
        let labels: Labels = serde_json::from_value(serde_json::json!({
            "version": 1, "language": "typescript", "split": "development", "oracle": "x",
            "closed_world": false,
            "entry_points": [
                {"label": "unresolved", "file": "app.ts", "line": 7, "kind": "http_handler",
                 "framework": "express", "method": "GET", "path": null}
            ]
        }))
        .unwrap();
        let mut entry = observed_entry();
        entry.framework = "express".to_string();
        entry.method = Some("GET".to_string());
        entry.parameters.clear();
        entry.guards.clear();
        let failures = |path: Option<&str>| -> Vec<String> {
            let mut entry = entry.clone();
            entry.file = "app.ts".to_string();
            entry.path = path.map(str::to_string);
            let observed = Observed {
                entry_points: vec![entry],
                ..Observed::default()
            };
            compare("typescript/routes", &labels, &observed)
                .unwrap()
                .into_iter()
                .filter(|assertion| !assertion.passed)
                .map(|assertion| assertion.id)
                .collect()
        };
        assert_eq!(failures(None), Vec::<String>::new());
        assert!(!failures(Some("/resolved")).is_empty());
    }

    fn path_labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1, "language": "python", "split": "development", "oracle": "x",
            "closed_world": false,
            "paths": [
                {"label": "update_to_system", "entry_file": "app.py", "entry_line": 7,
                 "sink_file": "app.py", "sink_line": 12,
                 "steps": ["app.py#update", "app.py#helper"],
                 "weakest_basis": "same_file", "path_count": 1,
                 "guards": ["login_required"], "choke_points": ["app.py#helper"]},
                {"label": "orphan_is_not_reached", "entry_file": "app.py", "entry_line": 7,
                 "sink_file": "app.py", "sink_line": 30, "flagged": false}
            ]
        }))
        .unwrap()
    }

    fn observed_path() -> ObservedPath {
        ObservedPath {
            entry_file: "app.py".to_string(),
            entry_line: 7,
            sink_file: "app.py".to_string(),
            sink_line: 12,
            steps: vec!["app.py#update".to_string(), "app.py#helper".to_string()],
            weakest_basis: Some("same_file".to_string()),
            path_count: 1,
            guards: vec!["login_required".to_string()],
            choke_points: vec!["app.py#helper".to_string()],
            function_references: 0,
        }
    }

    fn path_failures(observed: &Observed) -> Vec<String> {
        compare("python/paths", &path_labels(), observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    #[test]
    fn a_correct_path_passes_every_check() {
        let observed = Observed {
            paths: vec![observed_path()],
            ..Observed::default()
        };
        assert_eq!(path_failures(&observed), Vec::<String>::new());
    }

    #[test]
    fn a_wrong_step_count_basis_guard_or_choke_point_is_a_failure() {
        let mut wrong = observed_path();
        wrong.steps = vec!["app.py#update".to_string()];
        wrong.weakest_basis = Some("imported_name".to_string());
        wrong.path_count = 2;
        wrong.guards.clear();
        wrong.choke_points.clear();
        let observed = Observed {
            paths: vec![wrong],
            ..Observed::default()
        };
        assert_eq!(
            path_failures(&observed),
            [
                "python/paths/paths/update_to_system/steps",
                "python/paths/paths/update_to_system/weakest_basis",
                "python/paths/paths/update_to_system/path_count",
                "python/paths/paths/update_to_system/guards",
                "python/paths/paths/update_to_system/choke_points",
            ]
        );
    }

    #[test]
    fn a_missing_path_fails_and_a_path_that_must_not_exist_fails_when_it_does() {
        assert_eq!(path_failures(&Observed::default()).len(), 6);
        let mut extra = observed_path();
        extra.sink_line = 30;
        let observed = Observed {
            paths: vec![observed_path(), extra],
            ..Observed::default()
        };
        assert_eq!(
            path_failures(&observed),
            ["python/paths/paths/orphan_is_not_reached/absent"]
        );
    }

    #[test]
    fn a_choke_point_of_another_sink_does_not_satisfy_a_label() {
        let mut without = observed_path();
        without.choke_points.clear();
        let mut other = observed_path();
        other.sink_line = 30;
        let observed = Observed {
            paths: vec![without, other],
            ..Observed::default()
        };
        assert_eq!(
            path_failures(&observed),
            [
                "python/paths/paths/update_to_system/choke_points",
                "python/paths/paths/orphan_is_not_reached/absent"
            ]
        );
    }

    #[test]
    fn one_reported_sink_cannot_satisfy_two_labels() {
        let mut labels = sink_labels();
        let mut twin = labels.sinks[0].clone();
        twin.label = "command_execution.twin".to_string();
        labels.sinks.push(twin);
        let failing: Vec<String> = compare("python/security", &labels, &correct_sinks())
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect();
        assert_eq!(
            failing,
            [
                "python/security/sinks/command_execution.twin/found",
                "python/security/sinks/command_execution.twin/basis",
                "python/security/sinks/command_execution.twin/in_test",
                "python/security/sinks/command_execution.twin/in_generated",
                "python/security/sinks/command_execution.twin/constant_argument",
            ]
        );
    }

    #[test]
    fn a_second_sink_of_the_same_category_on_a_labelled_line_is_unlabelled() {
        let mut observed = correct_sinks();
        let mut extra = sink("sinks.py", 4, "command_execution");
        extra.column = 20;
        observed.sinks.push(extra);
        assert_eq!(
            sink_failures(&observed),
            ["python/security/sinks/unlabelled:sinks.py:4:20:command_execution/absent"]
        );
    }

    #[test]
    fn the_sinks_follow_closed_world_when_the_case_does_not_say() {
        let mut labels = sink_labels();
        labels.sinks_closed_world = None;
        labels.closed_world = true;
        let mut observed = correct_sinks();
        observed.sinks.push(sink("sinks.py", 9, "weak_hash"));
        let failing = compare("python/security", &labels, &observed)
            .unwrap()
            .iter()
            .filter(|assertion| !assertion.passed)
            .count();
        assert_eq!(failing, 1);
        labels.closed_world = false;
        let failing = compare("python/security", &labels, &observed)
            .unwrap()
            .iter()
            .filter(|assertion| !assertion.passed)
            .count();
        assert_eq!(failing, 0);
    }

    #[test]
    fn malformed_sink_labels_are_errors() {
        let mut category = sink_labels();
        category.sinks[0].category = "weak_everything".to_string();
        assert!(compare("python/security", &category, &correct_sinks()).is_err());

        let mut scenario = sink_labels();
        scenario.sinks[0].scenario = "somehow".to_string();
        assert!(compare("python/security", &scenario, &correct_sinks()).is_err());

        let mut duplicate = sink_labels();
        duplicate.sinks[1].label = duplicate.sinks[0].label.clone();
        assert!(compare("python/security", &duplicate, &correct_sinks()).is_err());

        let mut unflagged_with_basis = sink_labels();
        unflagged_with_basis.sinks[1].basis = Some("imported_symbol".to_string());
        assert!(compare("python/security", &unflagged_with_basis, &correct_sinks()).is_err());
    }

    #[test]
    fn sink_coverage_requires_each_category_and_scenario() {
        let mut one = sink_labels();
        let gaps = coverage_gaps(&[&one]);
        assert!(!gaps.contains(&"sinks/command_execution/real_call".to_string()));
        assert!(gaps.contains(&"sinks/command_execution/aliased_import".to_string()));
        assert!(gaps.contains(&"sinks/memory_unsafe/real_call".to_string()));

        one.not_applicable = vec![
            NotApplicable {
                dimension: Dimension::Sinks,
                category: "command_execution/aliased_import".to_string(),
                reason: "no alias syntax".to_string(),
            },
            NotApplicable {
                dimension: Dimension::Sinks,
                category: "memory_unsafe".to_string(),
                reason: "memory-safe language".to_string(),
            },
            NotApplicable {
                dimension: Dimension::Sinks,
                category: "weak_hash".to_string(),
                reason: " ".to_string(),
            },
        ];
        let gaps = coverage_gaps(&[&one]);
        assert!(!gaps.contains(&"sinks/command_execution/aliased_import".to_string()));
        assert!(!gaps.contains(&"sinks/memory_unsafe/real_call".to_string()));
        assert!(
            gaps.contains(&"sinks/weak_hash/real_call".to_string()),
            "blank reason is no exemption"
        );
    }

    fn test_labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1, "language": "rust", "split": "development", "oracle": "rustc",
            "closed_world": false,
            "tests": [
                {"label": "attribute", "category": "entry_by_syntax", "file": "a.rs",
                 "line": 4, "name": "checks", "role": "entry"},
                {"label": "helper", "category": "support", "file": "a.rs",
                 "line": 8, "name": "helper", "role": "support"},
                {"label": "block", "category": "support", "file": "a.rs", "line": 9,
                 "role": "support"},
                {"label": "production", "category": "not_test", "file": "a.rs",
                 "line": 1, "name": "test_like", "role": "none"},
                {"label": "production_line", "category": "not_test", "file": "a.rs",
                 "line": 2, "role": "none"}
            ]
        }))
        .unwrap()
    }

    fn observed_tests() -> Observed {
        let symbol = |name: &str, line: u32, role: &str| ObservedTestSymbol {
            location: location("a.rs", name, line),
            role: role.to_string(),
        };
        Observed {
            test_symbols: vec![
                symbol("checks", 4, "entry"),
                symbol("helper", 8, "support"),
                symbol("test_like", 1, "none"),
            ],
            test_lines: vec![ObservedTestLines {
                file: "a.rs".to_string(),
                start_line: 4,
                end_line: 10,
            }],
            ..Observed::default()
        }
    }

    fn test_failures(observed: &Observed) -> Vec<String> {
        compare("rust/test_code", &test_labels(), observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    #[test]
    fn correct_test_roles_pass_every_assertion() {
        assert!(test_failures(&observed_tests()).is_empty());
        assert_eq!(
            compare("rust/test_code", &test_labels(), &observed_tests())
                .unwrap()
                .len(),
            5
        );
    }

    #[test]
    fn a_wrong_or_missing_test_role_is_a_failure() {
        let mut observed = observed_tests();
        observed.test_symbols[0].role = "support".to_string();
        observed.test_symbols.remove(1);
        observed.test_lines[0].start_line = 1;
        observed.test_lines[0].end_line = 5;
        assert_eq!(
            test_failures(&observed),
            [
                "rust/test_code/tests/attribute/role",
                "rust/test_code/tests/helper/role",
                "rust/test_code/tests/block/role",
                "rust/test_code/tests/production_line/role",
            ]
        );
    }

    #[test]
    fn malformed_test_labels_are_errors() {
        let mut unknown = test_labels();
        unknown.tests[0].category = "other".to_string();
        assert!(compare("rust/test_code", &unknown, &observed_tests()).is_err());
        let mut line_entry = test_labels();
        line_entry.tests[2].role = TestRoleLabel::Entry;
        assert!(compare("rust/test_code", &line_entry, &observed_tests()).is_err());
    }

    #[test]
    fn test_coverage_requires_each_category() {
        let mut labels = test_labels();
        labels.tests.retain(|test| test.category != "support");
        let gaps = coverage_gaps(&[&labels]);
        assert!(gaps.contains(&"tests/support".to_string()), "{gaps:?}");
        assert!(
            gaps.contains(&"tests/entry_by_name".to_string()),
            "{gaps:?}"
        );
        assert!(
            !gaps.contains(&"tests/entry_by_syntax".to_string()),
            "{gaps:?}"
        );
    }

    fn reference_labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1, "language": "python", "split": "development", "oracle": "x",
            "closed_world": false,
            "references": [
                {"label": "callback", "user": "app.py#run", "line": 6, "name": "worker",
                 "expected": ["app.py#worker"]},
                {"label": "shadowed", "user": "app.py#other", "line": 9, "name": "worker",
                 "expected": []}
            ]
        }))
        .unwrap()
    }

    fn reference_failures(references: Vec<ObservedReference>) -> Vec<String> {
        let observed = Observed {
            references,
            ..Observed::default()
        };
        compare("python/references", &reference_labels(), &observed)
            .unwrap()
            .into_iter()
            .filter(|assertion| !assertion.passed)
            .map(|assertion| assertion.id)
            .collect()
    }

    fn observed_reference(user: &str, line: u32, edges: &[&str]) -> ObservedReference {
        ObservedReference {
            user: user.to_string(),
            line,
            name: "worker".to_string(),
            edges: edges.iter().map(|edge| edge.to_string()).collect(),
        }
    }

    #[test]
    fn a_function_reference_label_checks_the_edges_of_one_use() {
        let correct = vec![observed_reference("app.py#run", 6, &["app.py#worker"])];
        assert_eq!(reference_failures(correct), Vec::<String>::new());
        assert_eq!(
            reference_failures(Vec::new()),
            ["python/references/references/callback/no_missed_edge"]
        );
        let wrong = vec![
            observed_reference("app.py#run", 6, &["app.py#other"]),
            observed_reference("app.py#other", 9, &["app.py#worker"]),
        ];
        assert_eq!(
            reference_failures(wrong),
            [
                "python/references/references/callback/no_false_edge",
                "python/references/references/callback/no_missed_edge",
                "python/references/references/shadowed/no_false_edge",
            ]
        );
        // A use on a different line is a different use.
        let elsewhere = vec![observed_reference("app.py#run", 7, &["app.py#worker"])];
        assert_eq!(
            reference_failures(elsewhere),
            ["python/references/references/callback/no_missed_edge"]
        );
    }

    #[test]
    fn a_path_label_checks_the_number_of_function_references() {
        let mut labels = path_labels();
        labels.paths[0].function_references = Some(1);
        let failing = |observed: ObservedPath| -> Vec<String> {
            let observed = Observed {
                paths: vec![observed],
                ..Observed::default()
            };
            compare("python/paths", &labels, &observed)
                .unwrap()
                .into_iter()
                .filter(|assertion| !assertion.passed)
                .map(|assertion| assertion.id)
                .collect()
        };
        assert_eq!(
            failing(observed_path()),
            ["python/paths/paths/update_to_system/function_references"]
        );
        let mut through_reference = observed_path();
        through_reference.function_references = 1;
        assert_eq!(failing(through_reference), Vec::<String>::new());
    }

    #[test]
    fn sink_summary_is_per_language_and_category() {
        let assertions = [
            result("python/security/sinks/weak_hash.real_call/found", true),
            result("python/security/sinks/weak_hash.in_test/found", false),
            result(
                "python/security/sinks/unlabelled:a.py:3:weak_hash/absent",
                false,
            ),
            result("python/security/sinks/weak_cipher.real_call/found", true),
            result("python/security/calls/x/no_missed_edge", false),
        ];
        let table = summarize_sinks(&assertions);
        assert_eq!(
            table[&("python".to_string(), "weak_hash".to_string())],
            Tally {
                passed: 1,
                expected_failures: 2
            }
        );
        assert_eq!(table.len(), 2);
        assert!(render_sink_table(&table).contains("weak_cipher"));
    }
}
