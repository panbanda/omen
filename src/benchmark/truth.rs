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
pub const SINK_CATEGORIES: [&str; 9] = [
    "command_execution",
    "code_evaluation",
    "unsafe_deserialization",
    "sql_string_construction",
    "weak_hash",
    "weak_cipher",
    "tls_verification_disabled",
    "hardcoded_credential",
    "memory_unsafe",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Symbols,
    Calls,
    Imports,
    Sinks,
}

impl Dimension {
    pub const ALL: [Self; 4] = [Self::Symbols, Self::Calls, Self::Imports, Self::Sinks];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symbols => "symbols",
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::Sinks => "sinks",
        }
    }

    pub fn categories(self) -> &'static [&'static str] {
        match self {
            Self::Symbols => &SYMBOL_CATEGORIES,
            Self::Calls => &CALL_CATEGORIES,
            Self::Imports => &IMPORT_CATEGORIES,
            Self::Sinks => &SINK_CATEGORIES,
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
    /// When set, a sink that omen reports and that no label names is a failure.
    #[serde(default)]
    pub sinks_closed_world: bool,
    #[serde(default)]
    pub sinks: Vec<SinkLabel>,
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
}

#[derive(Debug, Clone)]
pub struct ObservedSink {
    pub file: String,
    pub line: u32,
    pub category: String,
    pub basis: String,
    pub in_test: bool,
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
    pub edges: Vec<Location>,
    pub candidates: Vec<Location>,
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
            && (sink.basis.is_some() || sink.in_test || sink.constant_argument.is_some())
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

fn compare_sinks(case: &str, labels: &Labels, observed: &Observed, out: &mut Vec<Assertion>) {
    let mut labelled = BTreeSet::new();
    for label in &labels.sinks {
        let key = (label.file.as_str(), label.line, label.category.as_str());
        labelled.insert(key);
        let prefix = format!("{case}/sinks/{}", label.label);
        let found = observed
            .sinks
            .iter()
            .find(|sink| (sink.file.as_str(), sink.line, sink.category.as_str()) == key);
        if !label.flagged {
            out.push(assertion(
                format!("{prefix}/absent"),
                found.map(|sink| format!("reported with the basis {}", sink.basis)),
            ));
            continue;
        }
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
        if let Some(constant) = label.constant_argument {
            out.push(check(
                "constant_argument",
                found.and_then(|sink| {
                    mismatch(constant.to_string(), sink.constant_argument.to_string())
                }),
            ));
        }
    }
    if labels.sinks_closed_world {
        let unlabelled: BTreeSet<_> = observed
            .sinks
            .iter()
            .map(|sink| (sink.file.as_str(), sink.line, sink.category.as_str()))
            .filter(|key| !labelled.contains(key))
            .collect();
        for (file, line, category) in unlabelled {
            out.push(assertion(
                format!("{case}/sinks/unlabelled:{file}:{line}:{category}/absent"),
                Some("reported but not a labelled sink".to_string()),
            ));
        }
    }
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
                    edges: vec![location("a.rs", "helper", 1)],
                    candidates: vec![location("a.rs", "helper", 1)],
                },
                ObservedCall {
                    caller: location("main.rs", "caller", 3),
                    call: "area".to_string(),
                    edges: vec![],
                    candidates: vec![location("a.rs", "area", 5)],
                },
            ],
            sinks: vec![],
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
            category: category.to_string(),
            basis: "imported_symbol".to_string(),
            in_test: file.starts_with("tests/"),
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
        // The real call has four checks, the safe variant one, and the test call two.
        assert_eq!(assertions.len(), 4 + 1 + 2);
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
            ["python/security/sinks/unlabelled:sinks.py:5:weak_hash/absent"]
        );
    }

    #[test]
    fn sink_basis_and_flags_are_checked_when_the_label_gives_them() {
        let mut observed = correct_sinks();
        observed.sinks[0].basis = "name_only".to_string();
        observed.sinks[0].in_test = true;
        observed.sinks[1].in_test = false;
        observed.sinks[0].constant_argument = true;
        assert_eq!(
            sink_failures(&observed),
            [
                "python/security/sinks/command_execution.real_call/basis",
                "python/security/sinks/command_execution.real_call/in_test",
                "python/security/sinks/command_execution.real_call/constant_argument",
                "python/security/sinks/command_execution.in_test/in_test",
            ]
        );
    }

    #[test]
    fn sinks_closed_world_rejects_unlabelled_sinks_only_when_set() {
        let mut observed = correct_sinks();
        observed.sinks.push(sink("sinks.py", 9, "weak_hash"));
        assert_eq!(
            sink_failures(&observed),
            ["python/security/sinks/unlabelled:sinks.py:9:weak_hash/absent"]
        );
        let mut open = sink_labels();
        open.sinks_closed_world = false;
        let failing = compare("python/security", &open, &observed)
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
