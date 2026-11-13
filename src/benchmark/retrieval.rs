//! Retrieval evaluation: labelled search queries, and the metrics for the
//! ranked results that a search engine gives for them.
//!
//! This module is pure. It reads no file and runs no search. `tests/retrieval.rs`
//! loads the corpus, runs the engine, and calls this module.
//!
//! A query lists one or more acceptable symbols. Each of them is a correct
//! answer, and the engine must find one of them, not all of them. A query has a
//! hit at `k` if one of these symbols is in the first `k` results. `hit@k` is
//! the fraction of the queries that have a hit at `k`. It is not recall: the
//! harness does not count the acceptable symbols that the engine finds. A
//! query that expects no result is a negative query. It has no hit and no
//! reciprocal rank, so the metrics do not include it.
//!
//! The metrics are for each pair of a language and a query type. This module
//! does not give a number that pools languages or query types.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::truth::{Assertion, Labels, LedgerEntry, Location};
use crate::semantic::tokenize::{lowercase, tokenize};

pub const CORPUS_VERSION: u32 = 1;

/// The smallest number of queries of each type for one language.
pub const MINIMUM_QUERIES_OF_EACH_TYPE: usize = 4;

/// The smallest number of queries for one language.
pub const MINIMUM_QUERIES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    /// An identifier, or one part of an identifier.
    Identifier,
    /// A description in words that are not all in the name of the symbol.
    NaturalLanguage,
    /// A fragment of code, such as a body or a signature.
    CodeSketch,
}

impl QueryType {
    pub const ALL: [Self; 3] = [Self::Identifier, Self::NaturalLanguage, Self::CodeSketch];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::NaturalLanguage => "natural_language",
            Self::CodeSketch => "code_sketch",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub version: u32,
    pub split: String,
    pub oracle: String,
    pub cases: Vec<Case>,
    /// The queries that were removed from the corpus. A query of the base
    /// branch must not change. To correct a query, retire its id and add a
    /// new query with a new id.
    #[serde(default)]
    pub retired: Vec<Retired>,
}

/// A removed query. It keeps the type, the text, the expected labels, and
/// their locations at the time of retirement, so that the same query cannot
/// come back with a new id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retired {
    /// `<language>/<query id>`.
    pub id: String,
    #[serde(rename = "type")]
    pub query_type: QueryType,
    pub query: String,
    pub expected: Vec<String>,
    /// The locations of the expected labels in the labels of the base
    /// revision. A label can move or go away later, so the comparison with a
    /// live query uses these locations, not the label ids.
    pub locations: Vec<RetiredLocation>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetiredLocation {
    pub file: String,
    pub name: String,
    pub start_line: u32,
}

impl From<Location> for RetiredLocation {
    fn from(location: Location) -> Self {
        Self {
            file: location.file,
            name: location.name,
            start_line: location.start_line,
        }
    }
}

/// The retrieval fixtures of one revision. `labels` has the labels of each
/// case, with the `case` value as the key.
#[derive(Debug, Clone, Copy)]
pub struct Revision<'a> {
    pub corpus: &'a Corpus,
    pub labels: &'a BTreeMap<String, Labels>,
    pub ledger: &'a [LedgerEntry],
}

/// The queries for one truth fixture case. The source files of the case are
/// the documents.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub language: String,
    /// The directory of the case below the truth fixtures, such as `rust/basics`.
    pub case: String,
    pub queries: Vec<Query>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub id: String,
    #[serde(rename = "type")]
    pub query_type: QueryType,
    pub query: String,
    /// The `label` ids of the acceptable symbols in the `labels.json` of the
    /// case. An empty list makes a negative query.
    pub expected: Vec<String>,
}

/// One query with its acceptable symbols and the results of the engine, best
/// result first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub language: String,
    pub id: String,
    pub query_type: QueryType,
    pub expected: Vec<Location>,
    pub ranked: Vec<Location>,
}

impl Outcome {
    /// The position of the first acceptable symbol, where the best result has
    /// rank 1.
    pub fn first_rank(&self) -> Option<usize> {
        self.ranked
            .iter()
            .position(|result| self.expected.contains(result))
            .map(|index| index + 1)
    }

    fn hit_at(&self, limit: usize) -> bool {
        self.first_rank().is_some_and(|rank| rank <= limit)
    }

    fn is_negative(&self) -> bool {
        self.expected.is_empty()
    }
}

/// Find the first word of a query that is also a word of the name of an
/// expected symbol. The tokenizer gives the words of the two texts.
///
/// A natural language query must describe a symbol in words that are not in
/// its name. If it has a word of the name, it is an identifier query.
fn word_of_a_symbol_name<'a>(query: &Query, labels: &'a Labels) -> Option<(String, &'a str)> {
    let words = tokenize(&query.query);
    labels
        .symbols
        .iter()
        .filter(|symbol| query.expected.contains(&symbol.label))
        .find_map(|symbol| {
            let name_words = tokenize(&symbol.name);
            let word = words.iter().find(|word| name_words.contains(word))?;
            Some((word.clone(), symbol.name.as_str()))
        })
}

/// Find the problems of a corpus. `labels` has the labels of each case, with
/// the `case` value as the key.
pub fn validate(corpus: &Corpus, labels: &BTreeMap<String, Labels>) -> Vec<String> {
    let mut problems = Vec::new();
    if corpus.version != CORPUS_VERSION {
        problems.push(format!("version must be {CORPUS_VERSION}"));
    }
    if corpus.split != "development" {
        problems.push("split must be `development`".to_string());
    }
    if corpus.oracle.trim().is_empty() {
        problems.push("oracle must say how the labels were made".to_string());
    }

    let mut languages = BTreeSet::new();
    for case in &corpus.cases {
        let language = &case.language;
        if !languages.insert(language) {
            problems.push(format!("{language}: more than one case"));
        }
        let Some(case_labels) = labels.get(&case.case) else {
            problems.push(format!(
                "{language}: no labels for the case `{}`",
                case.case
            ));
            continue;
        };
        let known: BTreeSet<&str> = case_labels
            .symbols
            .iter()
            .map(|symbol| symbol.label.as_str())
            .collect();

        let mut ids = BTreeSet::new();
        let mut counts: BTreeMap<QueryType, usize> = BTreeMap::new();
        let mut negatives = 0;
        for query in &case.queries {
            let id = &query.id;
            if id.is_empty() || id.contains('/') {
                problems.push(format!("{language}/{id}: the id must have text and no `/`"));
            }
            if !ids.insert(id) {
                problems.push(format!("{language}/{id}: the id occurs more than one time"));
            }
            if query.query.trim().is_empty() {
                problems.push(format!("{language}/{id}: the query has no text"));
            }
            if query.expected.is_empty() {
                negatives += 1;
            } else {
                *counts.entry(query.query_type).or_default() += 1;
            }
            let mut expected = BTreeSet::new();
            for label in &query.expected {
                if !known.contains(label.as_str()) {
                    problems.push(format!("{language}/{id}: no symbol label `{label}`"));
                }
                if !expected.insert(label) {
                    problems.push(format!(
                        "{language}/{id}: `{label}` occurs more than one time"
                    ));
                }
            }
            if query.query_type == QueryType::NaturalLanguage {
                // The tokenizer normalizes Unicode forms, but it does not find a
                // letter of a different script that looks the same. ASCII text
                // closes that gap for the name check.
                if !query.query.is_ascii() {
                    problems.push(format!(
                        "{language}/{id}: the natural language query must have only ASCII characters"
                    ));
                }
                if let Some((word, name)) = word_of_a_symbol_name(query, case_labels) {
                    problems.push(format!(
                        "{language}/{id}: the natural language query has the word `{word}` of the symbol name `{name}`"
                    ));
                }
            }
        }

        for query_type in QueryType::ALL {
            let count = counts.get(&query_type).copied().unwrap_or(0);
            if count < MINIMUM_QUERIES_OF_EACH_TYPE {
                problems.push(format!(
                    "{language}: {count} `{}` queries with an expected symbol, minimum {MINIMUM_QUERIES_OF_EACH_TYPE}",
                    query_type.as_str()
                ));
            }
        }
        let total: usize = counts.values().sum();
        if total < MINIMUM_QUERIES {
            problems.push(format!(
                "{language}: {total} queries with an expected symbol, minimum {MINIMUM_QUERIES}"
            ));
        }
        if negatives == 0 {
            problems.push(format!("{language}: no negative query"));
        }
    }

    let queries = queries_by_key(corpus);
    let mut retired = BTreeSet::new();
    for entry in &corpus.retired {
        let id = &entry.id;
        let well_formed = id
            .split_once('/')
            .is_some_and(|(language, query)| !language.is_empty() && !query.is_empty());
        if !well_formed {
            problems.push(format!(
                "retired {id}: the id must be `<language>/<query id>`"
            ));
        }
        if queries.contains_key(id) {
            problems.push(format!("retired {id}: the corpus has a query with this id"));
        }
        if entry.reason.trim().is_empty() {
            problems.push(format!("retired {id}: the reason has no text"));
        }
        if !retired.insert(id) {
            problems.push(format!("retired {id}: the id occurs more than one time"));
        }
    }
    problems.extend(returned_queries(corpus, labels));
    problems
}

/// Change the expected labels of a query into locations.
pub fn expected_locations(query: &Query, labels: &Labels) -> Vec<Location> {
    query
        .expected
        .iter()
        .filter_map(|label| labels.symbols.iter().find(|symbol| &symbol.label == label))
        .map(|symbol| Location {
            file: symbol.file.clone(),
            name: symbol.name.clone(),
            start_line: symbol.start_line,
        })
        .collect()
}

/// Give the results of one query one order.
///
/// `scored` has the results in the order of the engine, each with its score.
/// An engine that has no rule for results of equal score can give them in a
/// different order on each run. In each sequence of results with one score,
/// this function puts the acceptable symbols last. Thus the rank of an
/// acceptable symbol is the worst rank that the engine can give it, and equal
/// input gives equal output.
pub fn worst_case_order(scored: Vec<(f32, Location)>, expected: &[Location]) -> Vec<Location> {
    let mut ranked: Vec<Location> = Vec::with_capacity(scored.len());
    let mut start = 0;
    while start < scored.len() {
        let score = scored[start].0;
        let end = scored[start..]
            .iter()
            .position(|(other, _)| other.to_bits() != score.to_bits())
            .map_or(scored.len(), |length| start + length);
        let mut tied: Vec<Location> = scored[start..end]
            .iter()
            .map(|(_, location)| location.clone())
            .collect();
        tied.sort_by(|left, right| {
            expected
                .contains(left)
                .cmp(&expected.contains(right))
                .then_with(|| left.cmp(right))
        });
        ranked.extend(tied);
        start = end;
    }
    ranked
}

/// The rank of the best acceptable symbol of one query, in the worst-case
/// order. The id is `<language>/<query id>`. `None` means that the results
/// have no acceptable symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankEntry {
    pub id: String,
    pub rank: Option<usize>,
}

/// The ranks of the queries that have an expected symbol, in the order of the
/// ids.
pub fn rank_baseline(outcomes: &[Outcome]) -> Vec<RankEntry> {
    let ranks: BTreeMap<String, Option<usize>> = outcomes
        .iter()
        .filter(|outcome| !outcome.is_negative())
        .map(|outcome| {
            (
                format!("{}/{}", outcome.language, outcome.id),
                outcome.first_rank(),
            )
        })
        .collect();
    ranks
        .into_iter()
        .map(|(id, rank)| RankEntry { id, rank })
        .collect()
}

/// Compare two ranks. A smaller rank is better, and a rank is better than no
/// rank.
fn compare_ranks(current: Option<usize>, baseline: Option<usize>) -> std::cmp::Ordering {
    let worst = usize::MAX;
    baseline.unwrap_or(worst).cmp(&current.unwrap_or(worst))
}

fn rank_text(rank: Option<usize>) -> String {
    rank.map_or_else(|| "no rank".to_string(), |rank| format!("rank {rank}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RankViolation {
    /// The rank is worse than the baseline.
    Worse {
        id: String,
        baseline: Option<usize>,
        current: Option<usize>,
    },
    /// The rank is better than the baseline; write the baseline again.
    Better {
        id: String,
        baseline: Option<usize>,
        current: Option<usize>,
    },
    /// A query has no baseline entry.
    Missing { id: String },
    /// The baseline has an entry for a query that does not exist.
    Stale { id: String },
    /// Baseline ids must be strictly ascending.
    Unsorted { id: String },
}

impl std::fmt::Display for RankViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Worse {
                id,
                baseline,
                current,
            } => write!(
                f,
                "WORSE {id}: {}, the baseline has {}",
                rank_text(*current),
                rank_text(*baseline)
            ),
            Self::Better {
                id,
                baseline,
                current,
            } => write!(
                f,
                "BETTER {id}: {}, the baseline has {}; write the baseline again",
                rank_text(*current),
                rank_text(*baseline)
            ),
            Self::Missing { id } => write!(f, "MISSING {id}: the baseline has no entry"),
            Self::Stale { id } => write!(f, "STALE {id}: no such query"),
            Self::Unsorted { id } => {
                write!(f, "ORDER {id}: baseline ids must be strictly ascending")
            }
        }
    }
}

/// Enforce the rank baseline in both directions, as `check_ledger` does for
/// the ledger. The hit assertions see only rank 1 and rank 10. This check sees
/// each change of rank.
pub fn check_ranks(outcomes: &[Outcome], baseline: &[RankEntry]) -> Vec<RankViolation> {
    let mut violations = Vec::new();
    for pair in baseline.windows(2) {
        if pair[0].id >= pair[1].id {
            violations.push(RankViolation::Unsorted {
                id: pair[1].id.clone(),
            });
        }
    }
    let recorded: BTreeMap<&str, Option<usize>> = baseline
        .iter()
        .map(|entry| (entry.id.as_str(), entry.rank))
        .collect();
    let current = rank_baseline(outcomes);
    for entry in &current {
        let Some(&baseline) = recorded.get(entry.id.as_str()) else {
            violations.push(RankViolation::Missing {
                id: entry.id.clone(),
            });
            continue;
        };
        match compare_ranks(entry.rank, baseline) {
            std::cmp::Ordering::Less => violations.push(RankViolation::Worse {
                id: entry.id.clone(),
                baseline,
                current: entry.rank,
            }),
            std::cmp::Ordering::Greater => violations.push(RankViolation::Better {
                id: entry.id.clone(),
                baseline,
                current: entry.rank,
            }),
            std::cmp::Ordering::Equal => {}
        }
    }
    let produced: BTreeSet<&str> = current.iter().map(|entry| entry.id.as_str()).collect();
    for entry in baseline {
        if !produced.contains(entry.id.as_str()) {
            violations.push(RankViolation::Stale {
                id: entry.id.clone(),
            });
        }
    }
    violations
}

/// The queries of a corpus, each with its case, with `<language>/<query id>`
/// as the key.
fn queries_by_key(corpus: &Corpus) -> BTreeMap<String, (&Case, &Query)> {
    corpus
        .cases
        .iter()
        .flat_map(|case| {
            case.queries
                .iter()
                .map(move |query| (format!("{}/{}", case.language, query.id), (case, query)))
        })
        .collect()
}

/// The `<language>/<query id>` part of an assertion id.
fn query_key(assertion_id: &str) -> &str {
    assertion_id
        .rsplit_once('/')
        .map_or(assertion_id, |(key, _)| key)
}

fn label_set(expected: &[String]) -> BTreeSet<&str> {
    expected.iter().map(String::as_str).collect()
}

/// The location of each expected label. `None` means that the labels do not
/// have the label.
fn definitions<'a>(
    expected: &'a [String],
    labels: Option<&Labels>,
) -> BTreeMap<&'a str, Option<Location>> {
    expected
        .iter()
        .map(|label| {
            let location = labels
                .and_then(|labels| labels.symbols.iter().find(|symbol| &symbol.label == label))
                .map(|symbol| Location {
                    file: symbol.file.clone(),
                    name: symbol.name.clone(),
                    start_line: symbol.start_line,
                });
            (label.as_str(), location)
        })
        .collect()
}

/// The text form for the comparison with a retired query. Case, white space,
/// and the Unicode form do not change the tokens, so they do not make a new
/// query. Punctuation stays, because two code sketches can differ only in an
/// operator.
fn comparable_text(text: &str) -> String {
    lowercase(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The locations of the expected labels. `None` means that a label is not in
/// the labels.
fn resolved_locations(
    expected: &[String],
    labels: Option<&Labels>,
) -> Option<BTreeSet<RetiredLocation>> {
    definitions(expected, labels)
        .into_values()
        .map(|location| location.map(RetiredLocation::from))
        .collect()
}

/// Tell if a live query is a retired query again. The expected symbols are
/// the same if the current labels of the live query give the locations that
/// the retired entry recorded. The label ids are not compared, because a
/// label can move or get a new id.
fn is_retired_query(query: &Query, retired: &Retired, labels: Option<&Labels>) -> bool {
    let recorded: BTreeSet<RetiredLocation> = retired.locations.iter().cloned().collect();
    query.query_type == retired.query_type
        && comparable_text(&query.query) == comparable_text(&retired.query)
        && resolved_locations(&query.expected, labels) == Some(recorded)
}

/// Find the live queries that are the same as a retired query.
fn returned_queries(corpus: &Corpus, labels: &BTreeMap<String, Labels>) -> Vec<String> {
    let mut problems = Vec::new();
    for (key, (case, query)) in queries_by_key(corpus) {
        let prefix = format!("{}/", case.language);
        let case_labels = labels.get(&case.case);
        for retired in &corpus.retired {
            if retired.id.starts_with(&prefix) && is_retired_query(query, retired, case_labels) {
                problems.push(format!(
                    "{key}: the query is the same as the retired query `{}`",
                    retired.id
                ));
            }
        }
    }
    problems
}

/// Tell if a query has a different type, text, or set of expected labels.
fn is_changed(base: &Query, current: &Query) -> bool {
    base.query_type != current.query_type
        || base.query != current.query
        || label_set(&base.expected) != label_set(&current.expected)
}

/// Tell if a retired entry records a query exactly, with the locations that
/// `labels` give for its expected labels.
fn records(retired: &Retired, query: &Query, labels: Option<&Labels>) -> bool {
    let recorded: BTreeSet<RetiredLocation> = retired.locations.iter().cloned().collect();
    retired.query_type == query.query_type
        && retired.query == query.query
        && label_set(&retired.expected) == label_set(&query.expected)
        && resolved_locations(&query.expected, labels) == Some(recorded)
}

fn retired_by_id(corpus: &Corpus) -> BTreeMap<&str, &Retired> {
    corpus
        .retired
        .iter()
        .map(|retired| (retired.id.as_str(), retired))
        .collect()
}

/// Find the changes since the base revision that make the ledger grow, or
/// that change the oracle. The comparison uses the query id, not the
/// assertion id.
///
/// - A new ledger entry is permitted only if the base corpus does not have the
///   query. A changed query keeps its id, so a new assertion id for it is not
///   a new query.
/// - A query of the base corpus must keep its type, its text, and its
///   expected labels. A change can remove a failure from the ledger, or make
///   the metrics better, while the engine is the same.
/// - Each expected label of a query of the base corpus must keep its file,
///   name, and start line. Otherwise the label id stays, but it is a
///   different symbol. A label that no base query uses can change.
/// - A query of the base corpus that is not in the corpus must be in
///   `retired`, with its type, text, and expected labels. Without this rule, a
///   new id for a query starts a new ledger and rank history.
/// - A retired entry of the base corpus must stay, with no change, so the id
///   is not used again.
pub fn ledger_growth(base_revision: &Revision, revision: &Revision) -> Vec<String> {
    let (base, base_ledger) = (base_revision.corpus, base_revision.ledger);
    let (corpus, ledger) = (revision.corpus, revision.ledger);
    let base_queries = queries_by_key(base);
    let queries = queries_by_key(corpus);
    let retired = retired_by_id(corpus);
    let before: BTreeSet<&str> = base_ledger.iter().map(|entry| entry.id.as_str()).collect();

    let mut problems = BTreeSet::new();
    for entry in ledger {
        let key = query_key(&entry.id);
        if !before.contains(entry.id.as_str()) && base_queries.contains_key(key) {
            problems.insert(format!(
                "{}: the query is in the base corpus, and the base ledger does not have this entry",
                entry.id
            ));
        }
    }
    for (key, (base_case, base_query)) in &base_queries {
        match (queries.get(key), retired.get(key.as_str())) {
            (Some((_, query)), _) if is_changed(base_query, query) => {
                problems.insert(format!(
                    "{key}: a query of the base corpus must not change; retire its id and add a new query"
                ));
            }
            (Some((case, _)), _) => {
                let before = definitions(
                    &base_query.expected,
                    base_revision.labels.get(&base_case.case),
                );
                let after = definitions(&base_query.expected, revision.labels.get(&case.case));
                if let Some(label) = before
                    .iter()
                    .find(|(label, location)| after.get(*label) != Some(*location))
                    .map(|(label, _)| label)
                {
                    problems.insert(format!(
                        "{key}: the definition of the expected label `{label}` changed; retire the query id and add a new query"
                    ));
                }
            }
            (None, Some(entry))
                if !records(entry, base_query, base_revision.labels.get(&base_case.case)) =>
            {
                problems.insert(format!(
                    "{key}: the retired entry must have the type, the text, the expected labels, and the base locations of the base query"
                ));
            }
            (None, Some(_)) => {}
            (None, None) => {
                problems.insert(format!(
                    "{key}: the query of the base corpus was removed; add its id to `retired` with a reason"
                ));
            }
        }
    }
    for base_entry in &base.retired {
        let key = &base_entry.id;
        match retired.get(key.as_str()) {
            None => {
                problems.insert(format!(
                    "{key}: the id is retired on the base branch and must stay retired"
                ));
            }
            Some(entry)
                if entry.query_type != base_entry.query_type
                    || entry.query != base_entry.query
                    || label_set(&entry.expected) != label_set(&base_entry.expected)
                    || entry.locations != base_entry.locations =>
            {
                problems.insert(format!(
                    "{key}: the retired entry of the base branch must not change"
                ));
            }
            Some(_) => {}
        }
    }
    problems.into_iter().collect()
}

/// Find the ranks that are worse than on the base revision, for the queries
/// that the base corpus has. A query that the base corpus does not have is new,
/// and each rank is permitted for it.
pub fn rank_growth(base: &Corpus, base_ranks: &[RankEntry], ranks: &[RankEntry]) -> Vec<String> {
    let base_queries = queries_by_key(base);
    let before: BTreeMap<&str, Option<usize>> = base_ranks
        .iter()
        .map(|entry| (entry.id.as_str(), entry.rank))
        .collect();
    ranks
        .iter()
        .filter(|entry| base_queries.contains_key(&entry.id))
        .filter_map(|entry| {
            let baseline = *before.get(entry.id.as_str())?;
            (compare_ranks(entry.rank, baseline) == std::cmp::Ordering::Less).then(|| {
                format!(
                    "{}: {}, the base branch has {}",
                    entry.id,
                    rank_text(entry.rank),
                    rank_text(baseline)
                )
            })
        })
        .collect()
}

fn assertion_id(language: &str, query: &str, check: &str) -> String {
    format!("{language}/{query}/{check}")
}

/// The assertion ids that follow from the corpus alone, whatever the engine
/// gives.
pub fn assertion_ids(corpus: &Corpus) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for case in &corpus.cases {
        for query in &case.queries {
            let checks: &[&str] = if query.expected.is_empty() {
                &["empty"]
            } else {
                &["hit@1", "hit@10"]
            };
            for check in checks {
                ids.insert(assertion_id(&case.language, &query.id, check));
            }
        }
    }
    ids
}

fn listing(locations: &[Location]) -> String {
    if locations.is_empty() {
        return "no result".to_string();
    }
    locations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The checked facts of the outcomes, for the ledger.
///
/// A query with an expected symbol gives `hit@1` and `hit@10`. A negative
/// query gives `empty`, which passes if the engine gives no result.
pub fn assertions(outcomes: &[Outcome]) -> Vec<Assertion> {
    let mut assertions = Vec::new();
    for outcome in outcomes {
        if outcome.is_negative() {
            assertions.push(Assertion {
                id: assertion_id(&outcome.language, &outcome.id, "empty"),
                passed: outcome.ranked.is_empty(),
                detail: format!("expected no result, got {}", listing(&outcome.ranked)),
            });
            continue;
        }
        for limit in [1, 10] {
            let shown = &outcome.ranked[..outcome.ranked.len().min(limit)];
            assertions.push(Assertion {
                id: assertion_id(&outcome.language, &outcome.id, &format!("hit@{limit}")),
                passed: outcome.hit_at(limit),
                detail: format!(
                    "expected one of {} in the first {limit}, got {}",
                    listing(&outcome.expected),
                    listing(shown)
                ),
            });
        }
    }
    assertions
}

/// The metrics of one group of queries. A value is `None` if the group has no
/// query with an expected symbol. JSON shows `None` as `null`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Metrics {
    /// The number of queries with an expected symbol.
    pub queries: usize,
    /// The number of negative queries. The other values do not include them.
    pub negative_queries: usize,
    pub hit_at_1: Option<f64>,
    pub hit_at_5: Option<f64>,
    pub hit_at_10: Option<f64>,
    /// The mean of `1 / rank` of the first acceptable symbol in the complete
    /// result list. It has no cut at ten results. A query with no acceptable
    /// symbol in its results adds 0.
    pub mrr: Option<f64>,
}

/// Calculate the metrics of one group. The order of the outcomes gives the
/// order of each sum, so equal input gives equal output.
pub fn metrics<'a>(outcomes: impl IntoIterator<Item = &'a Outcome>) -> Metrics {
    let mut queries = 0usize;
    let mut negative_queries = 0usize;
    let mut hits = [0usize; 3];
    let mut reciprocal_ranks = 0.0f64;
    for outcome in outcomes {
        if outcome.is_negative() {
            negative_queries += 1;
            continue;
        }
        queries += 1;
        for (index, limit) in [1, 5, 10].into_iter().enumerate() {
            if outcome.hit_at(limit) {
                hits[index] += 1;
            }
        }
        if let Some(rank) = outcome.first_rank() {
            reciprocal_ranks += 1.0 / rank as f64;
        }
    }
    let mean = |sum: f64| (queries > 0).then(|| sum / queries as f64);
    Metrics {
        queries,
        negative_queries,
        hit_at_1: mean(hits[0] as f64),
        hit_at_5: mean(hits[1] as f64),
        hit_at_10: mean(hits[2] as f64),
        mrr: mean(reciprocal_ranks),
    }
}

/// The metrics for each language and query type. Each language of the
/// outcomes has a row for each query type, also a type that has no query. No
/// row pools two languages or two query types.
pub fn metrics_by_language_and_type(outcomes: &[Outcome]) -> BTreeMap<(String, String), Metrics> {
    let languages: BTreeSet<&str> = outcomes
        .iter()
        .map(|outcome| outcome.language.as_str())
        .collect();
    let mut table = BTreeMap::new();
    for language in languages {
        for query_type in QueryType::ALL {
            let group = outcomes
                .iter()
                .filter(|outcome| outcome.language == language && outcome.query_type == query_type);
            table.insert(
                (language.to_string(), query_type.as_str().to_string()),
                metrics(group),
            );
        }
    }
    table
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_string(), |value| format!("{value:.3}"))
}

/// Text table with one row for each language and query type.
pub fn render_table(table: &BTreeMap<(String, String), Metrics>) -> String {
    let mut out = format!(
        "{:<10} {:<16} {:>7} {:>8} {:>8} {:>8} {:>9} {:>17}\n",
        "language",
        "query type",
        "queries",
        "negative",
        "hit@1",
        "hit@5",
        "hit@10",
        "mrr (all results)"
    );
    for ((language, query_type), metrics) in table {
        out.push_str(&format!(
            "{language:<10} {query_type:<16} {:>7} {:>8} {:>8} {:>8} {:>9} {:>17}\n",
            metrics.queries,
            metrics.negative_queries,
            cell(metrics.hit_at_1),
            cell(metrics.hit_at_5),
            cell(metrics.hit_at_10),
            cell(metrics.mrr),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(name: &str) -> Location {
        Location {
            file: "a.rs".to_string(),
            name: name.to_string(),
            start_line: 1,
        }
    }

    fn outcome(language: &str, id: &str, expected: &[&str], ranked: &[&str]) -> Outcome {
        Outcome {
            language: language.to_string(),
            id: id.to_string(),
            query_type: QueryType::Identifier,
            expected: expected.iter().map(|name| location(name)).collect(),
            ranked: ranked.iter().map(|name| location(name)).collect(),
        }
    }

    fn typed(query_type: QueryType, expected: &[&str], ranked: &[&str]) -> Outcome {
        Outcome {
            query_type,
            ..outcome("rust", "q", expected, ranked)
        }
    }

    fn labels() -> Labels {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "language": "rust",
            "split": "development",
            "oracle": "author-reviewed, not executed",
            "closed_world": false,
            "symbols": [
                {"label": "helper", "category": "free_function", "file": "a.rs", "name": "helper",
                 "kind": "function", "container": null, "start_line": 3, "end_line": 4, "exported": true},
                {"label": "other", "category": "free_function", "file": "b.rs", "name": "helper",
                 "kind": "function", "container": null, "start_line": 9, "end_line": 9, "exported": true}
            ]
        }))
        .unwrap()
    }

    fn queries(count: usize, query_type: &str, prefix: &str) -> Vec<serde_json::Value> {
        (0..count)
            .map(|index| {
                serde_json::json!({
                    "id": format!("{prefix}{index}"), "type": query_type,
                    "query": "give support", "expected": ["helper"]
                })
            })
            .collect()
    }

    fn corpus(extra: Vec<serde_json::Value>) -> Corpus {
        let mut all = queries(4, "identifier", "i");
        all.extend(queries(4, "natural_language", "n"));
        all.extend(queries(4, "code_sketch", "c"));
        all.push(serde_json::json!({
            "id": "x", "type": "identifier", "query": "absent", "expected": []
        }));
        all.extend(extra);
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "split": "development",
            "oracle": "An author wrote it from the source.",
            "cases": [{"language": "rust", "case": "rust/basics", "queries": all}]
        }))
        .unwrap()
    }

    fn case_labels() -> BTreeMap<String, Labels> {
        BTreeMap::from([("rust/basics".to_string(), labels())])
    }

    #[test]
    fn first_rank_is_the_best_acceptable_symbol() {
        assert_eq!(
            outcome("rust", "q", &["b", "c"], &["a", "c", "b"]).first_rank(),
            Some(2)
        );
        assert_eq!(outcome("rust", "q", &["z"], &["a", "b"]).first_rank(), None);
        assert_eq!(outcome("rust", "q", &["a"], &[]).first_rank(), None);
    }

    #[test]
    fn a_location_matches_on_file_name_and_line() {
        let mut found = outcome("rust", "q", &["a"], &["a"]);
        found.ranked[0].start_line = 2;
        assert_eq!(found.first_rank(), None);
    }

    #[test]
    fn metrics_count_a_hit_at_each_limit() {
        let ranked: Vec<String> = (0..10).map(|index| format!("r{index}")).collect();
        let ranked: Vec<&str> = ranked.iter().map(String::as_str).collect();
        let outcomes = [
            outcome("rust", "first", &["r0"], &ranked),
            outcome("rust", "fifth", &["r4"], &ranked),
            outcome("rust", "tenth", &["r9"], &ranked),
            outcome("rust", "absent", &["none"], &ranked),
        ];

        let result = metrics(&outcomes);

        assert_eq!(result.queries, 4);
        assert_eq!(result.hit_at_1, Some(0.25));
        assert_eq!(result.hit_at_5, Some(0.5));
        assert_eq!(result.hit_at_10, Some(0.75));
        assert_eq!(result.mrr, Some((1.0 + 1.0 / 5.0 + 1.0 / 10.0) / 4.0));
    }

    #[test]
    fn metrics_are_null_for_a_group_with_no_expected_symbol() {
        let result = metrics(&[outcome("rust", "x", &[], &["a"])]);

        assert_eq!(result.queries, 0);
        assert_eq!(result.negative_queries, 1);
        assert_eq!(result.hit_at_1, None);
        assert_eq!(result.hit_at_5, None);
        assert_eq!(result.hit_at_10, None);
        assert_eq!(result.mrr, None);
        let json = serde_json::to_value(result).unwrap();
        assert!(json["hit_at_10"].is_null());
        assert!(json["mrr"].is_null());
    }

    #[test]
    fn a_negative_query_does_not_change_the_hit_rate() {
        let with_negative = metrics(&[
            outcome("rust", "q", &["a"], &["a"]),
            outcome("rust", "x", &[], &["a"]),
        ]);

        assert_eq!(with_negative.hit_at_1, Some(1.0));
        assert_eq!(with_negative.mrr, Some(1.0));
        assert_eq!(with_negative.negative_queries, 1);
    }

    fn pair(language: &str, query_type: QueryType) -> (String, String) {
        (language.to_string(), query_type.as_str().to_string())
    }

    #[test]
    fn metrics_are_separate_for_each_language_and_each_query_type() {
        // No cell pools two languages or two query types.
        let outcomes = [
            outcome("go", "q", &["a"], &["a"]),
            outcome("rust", "q", &["a"], &["b"]),
            typed(QueryType::CodeSketch, &["a"], &["b", "a"]),
        ];

        let table = metrics_by_language_and_type(&outcomes);

        assert_eq!(table.len(), 6);
        assert_eq!(
            table[&pair("go", QueryType::Identifier)].hit_at_1,
            Some(1.0)
        );
        assert_eq!(
            table[&pair("rust", QueryType::Identifier)].hit_at_1,
            Some(0.0)
        );
        assert_eq!(table[&pair("rust", QueryType::CodeSketch)].mrr, Some(0.5));
        assert_eq!(table[&pair("go", QueryType::CodeSketch)].queries, 0);
        assert_eq!(table[&pair("go", QueryType::CodeSketch)].mrr, None);
    }

    #[test]
    fn metrics_by_language_and_type_have_a_row_for_each_pair() {
        let table = metrics_by_language_and_type(&[typed(QueryType::Identifier, &["a"], &["a"])]);

        assert_eq!(table.len(), 3);
        let key = ("rust".to_string(), "identifier".to_string());
        assert_eq!(table[&key].hit_at_1, Some(1.0));
    }

    #[test]
    fn a_query_gives_two_hit_assertions() {
        let found = assertions(&[outcome("rust", "q", &["b"], &["a", "b"])]);

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "rust/q/hit@1");
        assert!(!found[0].passed);
        assert_eq!(found[1].id, "rust/q/hit@10");
        assert!(found[1].passed);
        assert!(found[0].detail.contains("a.rs:b:1"), "{}", found[0].detail);
    }

    #[test]
    fn hit_at_ten_ignores_the_eleventh_result() {
        let ranked: Vec<String> = (0..11).map(|index| format!("r{index}")).collect();
        let ranked: Vec<&str> = ranked.iter().map(String::as_str).collect();

        let found = assertions(&[outcome("rust", "q", &["r10"], &ranked)]);

        assert!(!found[1].passed);
    }

    #[test]
    fn a_negative_query_passes_only_with_no_result() {
        let found = assertions(&[
            outcome("rust", "quiet", &[], &[]),
            outcome("rust", "noisy", &[], &["a"]),
        ]);

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "rust/quiet/empty");
        assert!(found[0].passed);
        assert_eq!(found[1].id, "rust/noisy/empty");
        assert!(!found[1].passed);
    }

    #[test]
    fn assertion_ids_agree_with_the_assertions() {
        let corpus = corpus(Vec::new());
        let labels = labels();
        let outcomes: Vec<Outcome> = corpus.cases[0]
            .queries
            .iter()
            .map(|query| Outcome {
                language: "rust".to_string(),
                id: query.id.clone(),
                query_type: query.query_type,
                expected: expected_locations(query, &labels),
                ranked: Vec::new(),
            })
            .collect();

        let produced: BTreeSet<String> = assertions(&outcomes)
            .into_iter()
            .map(|assertion| assertion.id)
            .collect();

        assert_eq!(assertion_ids(&corpus), produced);
        assert!(produced.contains("rust/x/empty"));
        assert!(produced.contains("rust/i0/hit@10"));
    }

    #[test]
    fn expected_locations_come_from_the_symbol_labels() {
        let corpus = corpus(Vec::new());

        let found = expected_locations(&corpus.cases[0].queries[0], &labels());

        assert_eq!(
            found,
            [Location {
                file: "a.rs".to_string(),
                name: "helper".to_string(),
                start_line: 3
            }]
        );
    }

    #[test]
    fn a_complete_corpus_has_no_problem() {
        assert_eq!(
            validate(&corpus(Vec::new()), &case_labels()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn validation_finds_each_kind_of_problem() {
        let mut broken = corpus(vec![
            serde_json::json!({"id": "i0", "type": "identifier", "query": "helper", "expected": ["helper"]}),
            serde_json::json!({"id": "bad/id", "type": "identifier", "query": " ", "expected": ["nothing"]}),
            serde_json::json!({"id": "twice", "type": "identifier", "query": "helper", "expected": ["helper", "helper"]}),
        ]);
        broken.version = 2;
        broken.split = "held_out".to_string();
        broken.oracle = " ".to_string();

        let problems = validate(&broken, &case_labels()).join("\n");

        for expected in [
            "version must be 1",
            "split must be `development`",
            "oracle must say",
            "rust/i0: the id occurs more than one time",
            "rust/bad/id: the id must have text and no `/`",
            "rust/bad/id: the query has no text",
            "rust/bad/id: no symbol label `nothing`",
            "rust/twice: `helper` occurs more than one time",
        ] {
            assert!(
                problems.contains(expected),
                "no `{expected}` in:\n{problems}"
            );
        }
    }

    #[test]
    fn validation_requires_each_query_type_and_a_negative_query() {
        let thin: Corpus = serde_json::from_value(serde_json::json!({
            "version": 1,
            "split": "development",
            "oracle": "An author wrote it from the source.",
            "cases": [
                {"language": "rust", "case": "rust/basics", "queries": queries(4, "identifier", "i")},
                {"language": "go", "case": "go/basics", "queries": []}
            ]
        }))
        .unwrap();

        let problems = validate(&thin, &case_labels()).join("\n");

        for expected in [
            "rust: 0 `natural_language` queries",
            "rust: 0 `code_sketch` queries",
            "rust: 4 queries with an expected symbol, minimum 12",
            "rust: no negative query",
            "go: no labels for the case `go/basics`",
        ] {
            assert!(
                problems.contains(expected),
                "no `{expected}` in:\n{problems}"
            );
        }
    }

    #[test]
    fn the_corpus_rejects_an_unknown_field_and_an_unknown_type() {
        let unknown_field = serde_json::json!({
            "version": 1, "split": "development", "oracle": "x", "cases": [], "notes": ""
        });
        assert!(serde_json::from_value::<Corpus>(unknown_field).is_err());

        let unknown_type = serde_json::json!({
            "id": "q", "type": "regex", "query": "x", "expected": []
        });
        assert!(serde_json::from_value::<Query>(unknown_type).is_err());
    }

    #[test]
    fn the_table_shows_null_for_an_undefined_value() {
        let table =
            metrics_by_language_and_type(&[typed(QueryType::Identifier, &["a"], &["b", "a"])]);

        let text = render_table(&table);

        let identifier = text
            .lines()
            .find(|line| line.starts_with("rust") && line.contains("identifier"))
            .unwrap();
        assert!(identifier.contains("0.500"), "{text}");
        let natural = text
            .lines()
            .find(|line| line.contains("natural_language"))
            .unwrap();
        assert!(natural.contains("null"), "{text}");
    }
    #[test]
    fn worst_case_order_puts_an_acceptable_symbol_last_in_its_tie() {
        let scored = vec![
            (0.9, location("top")),
            (0.5, location("wanted")),
            (0.5, location("zeta")),
            (0.5, location("alpha")),
            (0.1, location("low")),
        ];

        let ranked = worst_case_order(scored, &[location("wanted")]);

        let names: Vec<&str> = ranked.iter().map(|result| result.name.as_str()).collect();
        assert_eq!(names, ["top", "alpha", "zeta", "wanted", "low"]);
    }

    #[test]
    fn worst_case_order_does_not_depend_on_the_order_in_a_tie() {
        let expected = [location("b")];
        let one = vec![
            (0.5, location("a")),
            (0.5, location("b")),
            (0.5, location("c")),
        ];
        let two = vec![
            (0.5, location("c")),
            (0.5, location("b")),
            (0.5, location("a")),
        ];

        assert_eq!(
            worst_case_order(one, &expected),
            worst_case_order(two, &expected)
        );
    }

    #[test]
    fn worst_case_order_keeps_the_order_of_different_scores() {
        let scored = vec![(0.9, location("wanted")), (0.5, location("other"))];

        let ranked = worst_case_order(scored, &[location("wanted")]);

        assert_eq!(ranked, [location("wanted"), location("other")]);
        assert!(worst_case_order(Vec::new(), &[]).is_empty());
    }
    #[test]
    fn a_tie_of_eleven_results_puts_the_acceptable_symbol_after_the_tenth() {
        // The driver must give the complete list. If it cuts the list at ten
        // before this order, the acceptable symbol can be in the ten by chance.
        let scored: Vec<(f32, Location)> = (0..11)
            .map(|index| (0.5, location(&format!("r{index:02}"))))
            .collect();
        let expected = [location("r03")];

        let found = Outcome {
            ranked: worst_case_order(scored, &expected),
            ..outcome("rust", "q", &["r03"], &[])
        };

        assert_eq!(found.first_rank(), Some(11));
        let checks = assertions(std::slice::from_ref(&found));
        assert!(!checks[1].passed, "hit@10 must fail");
        assert_eq!(metrics(&[found]).mrr, Some(1.0 / 11.0));
    }

    #[test]
    fn the_table_says_that_the_mrr_uses_all_results() {
        let text = render_table(&BTreeMap::new());

        assert!(
            text.lines().next().unwrap().contains("mrr (all results)"),
            "{text}"
        );
    }
    #[test]
    fn the_table_names_the_hit_rate_and_not_recall() {
        // One acceptable symbol of four in the first result is a hit. The
        // value is a hit rate, so the name must not be recall.
        let found = metrics(&[outcome("rust", "q", &["a", "b", "c", "d"], &["a"])]);
        let table = BTreeMap::from([(("rust".to_string(), "identifier".to_string()), found)]);

        let text = render_table(&table);

        let heading = text.lines().next().unwrap();
        assert!(heading.contains("hit@1"), "{text}");
        assert!(!text.contains("recall"), "{text}");
        let json = serde_json::to_value(found).unwrap();
        assert_eq!(json["hit_at_1"], 1.0);
        let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        assert!(!keys.iter().any(|key| key.contains("recall")), "{json}");
    }

    fn rank(id: &str, rank: Option<usize>) -> RankEntry {
        RankEntry {
            id: id.to_string(),
            rank,
        }
    }

    #[test]
    fn rank_baseline_has_each_positive_query_in_id_order() {
        let outcomes = [
            outcome("rust", "second", &["b"], &["a", "b"]),
            outcome("go", "absent", &["z"], &["a"]),
            outcome("rust", "negative", &[], &["a"]),
        ];

        assert_eq!(
            rank_baseline(&outcomes),
            [rank("go/absent", None), rank("rust/second", Some(2))]
        );
    }

    #[test]
    fn an_equal_rank_is_no_violation() {
        let outcomes = [
            outcome("rust", "found", &["b"], &["a", "b"]),
            outcome("rust", "absent", &["z"], &["a"]),
        ];
        let baseline = [rank("rust/absent", None), rank("rust/found", Some(2))];

        assert_eq!(check_ranks(&outcomes, &baseline), []);
    }

    #[test]
    fn a_rank_inside_the_first_ten_that_gets_worse_is_a_violation() {
        // `hit@10` passes for rank 2 and for rank 9, so only this check sees it.
        let ranked: Vec<String> = (0..9).map(|index| format!("r{index}")).collect();
        let ranked: Vec<&str> = ranked.iter().map(String::as_str).collect();
        let outcomes = [outcome("rust", "q", &["r8"], &ranked)];

        let violations = check_ranks(&outcomes, &[rank("rust/q", Some(2))]);

        assert_eq!(
            violations,
            [RankViolation::Worse {
                id: "rust/q".to_string(),
                baseline: Some(2),
                current: Some(9)
            }]
        );
        assert_eq!(
            violations[0].to_string(),
            "WORSE rust/q: rank 9, the baseline has rank 2"
        );
    }

    #[test]
    fn a_lost_symbol_is_worse_than_each_rank() {
        let outcomes = [outcome("rust", "q", &["z"], &["a"])];

        let violations = check_ranks(&outcomes, &[rank("rust/q", Some(40))]);

        assert!(matches!(
            violations[0],
            RankViolation::Worse { current: None, .. }
        ));
    }

    #[test]
    fn a_better_rank_needs_a_new_baseline() {
        let found = [outcome("rust", "q", &["a"], &["a"])];

        let from_rank = check_ranks(&found, &[rank("rust/q", Some(3))]);
        let from_absent = check_ranks(&found, &[rank("rust/q", None)]);

        assert!(matches!(from_rank[0], RankViolation::Better { .. }));
        assert!(matches!(
            from_absent[0],
            RankViolation::Better { baseline: None, .. }
        ));
        assert_eq!(
            from_absent[0].to_string(),
            "BETTER rust/q: rank 1, the baseline has no rank; write the baseline again"
        );
    }

    #[test]
    fn the_baseline_must_agree_with_the_queries() {
        let outcomes = [outcome("rust", "new", &["a"], &["a"])];
        let baseline = [rank("rust/z", Some(1)), rank("rust/gone", Some(1))];

        let violations = check_ranks(&outcomes, &baseline);

        assert!(violations.contains(&RankViolation::Unsorted {
            id: "rust/gone".to_string()
        }));
        assert!(violations.contains(&RankViolation::Missing {
            id: "rust/new".to_string()
        }));
        assert!(violations.contains(&RankViolation::Stale {
            id: "rust/z".to_string()
        }));
    }

    #[test]
    fn a_negative_query_has_no_rank_entry() {
        let outcomes = [outcome("rust", "x", &[], &["a"])];

        assert_eq!(check_ranks(&outcomes, &[]), []);
    }

    #[test]
    fn a_rank_entry_is_null_in_json_when_there_is_no_rank() {
        let text = serde_json::to_string(&rank("rust/q", None)).unwrap();

        assert_eq!(text, r#"{"id":"rust/q","rank":null}"#);
        assert_eq!(
            serde_json::from_str::<RankEntry>(&text).unwrap(),
            rank("rust/q", None)
        );
    }
    fn entry(id: &str) -> LedgerEntry {
        LedgerEntry {
            id: id.to_string(),
            reason: "reason".to_string(),
            tracking: "tracking".to_string(),
        }
    }

    /// The complete corpus with one query replaced or added.
    fn corpus_with(query: serde_json::Value) -> Corpus {
        let mut changed = corpus(Vec::new());
        let query: Query = serde_json::from_value(query).unwrap();
        let queries = &mut changed.cases[0].queries;
        match queries.iter_mut().find(|existing| existing.id == query.id) {
            Some(existing) => *existing = query,
            None => queries.push(query),
        }
        changed
    }

    #[test]
    fn a_ledger_entry_for_a_new_query_is_permitted() {
        let base = corpus(Vec::new());
        let current = corpus_with(serde_json::json!({
            "id": "fresh", "type": "identifier", "query": "helper", "expected": ["helper"]
        }));

        let grown = growth(&base, &[], &current, &[entry("rust/fresh/hit@1")]);

        assert_eq!(grown, Vec::<String>::new());
    }

    #[test]
    fn a_ledger_entry_for_a_query_that_passed_on_the_base_is_growth() {
        let base = corpus(Vec::new());

        let grown = growth(&base, &[], &base, &[entry("rust/i0/hit@1")]);

        assert_eq!(
            grown,
            ["rust/i0/hit@1: the query is in the base corpus, and the base ledger does not have this entry"]
        );
    }

    #[test]
    fn an_entry_that_the_base_ledger_has_is_not_growth() {
        let base = corpus(Vec::new());
        let ledger = [entry("rust/i0/hit@1")];

        assert_eq!(growth(&base, &ledger, &base, &ledger), Vec::<String>::new());
    }

    const CHANGED: &str =
        "rust/i0: a query of the base corpus must not change; retire its id and add a new query";

    #[test]
    fn a_changed_query_type_does_not_permit_a_new_entry() {
        let base = corpus(Vec::new());
        let current = corpus_with(serde_json::json!({
            "id": "i0", "type": "code_sketch", "query": "give support", "expected": ["helper"]
        }));

        let grown = growth(&base, &[], &current, &[entry("rust/i0/hit@10")]);

        assert_eq!(
            grown,
            [
                "rust/i0/hit@10: the query is in the base corpus, and the base ledger does not have this entry",
                CHANGED,
            ]
        );
    }

    #[test]
    fn a_positive_query_that_becomes_negative_does_not_permit_a_new_entry() {
        // The assertion id `empty` is new, but the query id is not.
        let base = corpus(Vec::new());
        let current = corpus_with(serde_json::json!({
            "id": "i0", "type": "identifier", "query": "give support", "expected": []
        }));

        let grown = growth(&base, &[], &current, &[entry("rust/i0/empty")]);

        assert!(grown.contains(&CHANGED.to_string()), "{grown:?}");
        assert!(grown[0].starts_with("rust/i0/empty: the query is in the base corpus"));
    }

    #[test]
    fn a_change_of_a_query_of_the_base_corpus_is_growth() {
        // A change of a query that passes can make the metrics better while
        // the engine is the same. A change of a query that fails can remove
        // it from the ledger.
        let base = corpus(Vec::new());
        let changes = [
            serde_json::json!({"id": "i0", "type": "code_sketch", "query": "give support", "expected": ["helper"]}),
            serde_json::json!({"id": "i0", "type": "identifier", "query": "give support", "expected": ["other"]}),
            serde_json::json!({"id": "i0", "type": "identifier", "query": "give support", "expected": ["helper", "other"]}),
            serde_json::json!({"id": "i0", "type": "identifier", "query": "helper", "expected": ["helper"]}),
        ];
        for base_ledger in [Vec::new(), vec![entry("rust/i0/hit@1")]] {
            for change in &changes {
                let current = corpus_with(change.clone());

                let grown = growth(&base, &base_ledger, &current, &[]);

                assert_eq!(grown, [CHANGED], "{change}");
            }
        }
    }

    /// Compare two revisions that have the same labels.
    fn growth(
        base: &Corpus,
        base_ledger: &[LedgerEntry],
        corpus: &Corpus,
        ledger: &[LedgerEntry],
    ) -> Vec<String> {
        let labels = case_labels();
        ledger_growth(
            &Revision {
                corpus: base,
                labels: &labels,
                ledger: base_ledger,
            },
            &Revision {
                corpus,
                labels: &labels,
                ledger,
            },
        )
    }

    /// A retired entry with the locations that the test labels give.
    fn retired(id: &str, query_type: QueryType, query: &str, expected: &[&str]) -> Retired {
        let expected: Vec<String> = expected.iter().map(ToString::to_string).collect();
        let labels = labels();
        let locations = definitions(&expected, Some(&labels))
            .into_values()
            .flatten()
            .map(RetiredLocation::from)
            .collect();
        Retired {
            id: id.to_string(),
            query_type,
            query: query.to_string(),
            expected,
            locations,
            reason: "the label was wrong".to_string(),
        }
    }

    #[test]
    fn a_retired_query_is_found_after_its_label_is_removed() {
        // The retired label `gone` is not in the labels now. A new label for
        // the same symbol does not make a new query.
        let mut old = retired_old();
        old.expected = vec!["gone".to_string()];
        let mut returned = corpus(vec![serde_json::json!({"id": "back", "type": "identifier",
            "query": "call the helper", "expected": ["helper"]})]);
        returned.retired.push(old);

        let problems = validate(&returned, &case_labels());

        assert_eq!(
            problems,
            ["rust/back: the query is the same as the retired query `rust/old`"]
        );
    }

    #[test]
    fn a_replacement_for_a_moved_label_is_a_new_query() {
        // The label `helper` moved after the retirement. The replacement has
        // the same text and label id, but it expects a different location.
        let mut moved = labels();
        moved.symbols[0].start_line = 30;
        let labels = BTreeMap::from([("rust/basics".to_string(), moved)]);
        let mut replaced = corpus(vec![
            serde_json::json!({"id": "replacement", "type": "identifier",
            "query": "call the helper", "expected": ["helper"]}),
        ]);
        replaced.retired.push(retired_old());

        assert_eq!(validate(&replaced, &labels), Vec::<String>::new());
    }

    /// The retired entry for the query `i0` of the test corpus.
    fn retired_i0() -> Retired {
        retired(
            "rust/i0",
            QueryType::Identifier,
            "give support",
            &["helper"],
        )
    }

    /// A retired query whose text no query of the test corpus has.
    fn retired_old() -> Retired {
        retired(
            "rust/old",
            QueryType::Identifier,
            "call the helper",
            &["helper"],
        )
    }

    fn without_i0() -> Corpus {
        let mut removed = corpus(Vec::new());
        removed.cases[0].queries.retain(|query| query.id != "i0");
        removed
    }

    #[test]
    fn a_removed_query_must_be_retired() {
        let base = corpus(Vec::new());
        let base_ledger = [entry("rust/i0/hit@1")];
        let mut removed = without_i0();

        let grown = growth(&base, &base_ledger, &removed, &[]);

        assert_eq!(
            grown,
            ["rust/i0: the query of the base corpus was removed; add its id to `retired` with a reason"]
        );

        removed.retired.push(retired_i0());
        assert_eq!(
            growth(&base, &base_ledger, &removed, &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_retired_entry_must_copy_the_base_query() {
        // A retired entry with a different text does not record the query
        // that was removed, so the check for a query used again fails.
        let base = corpus(Vec::new());
        let mut removed = without_i0();
        removed.retired.push(retired(
            "rust/i0",
            QueryType::Identifier,
            "other text",
            &["helper"],
        ));

        let grown = growth(&base, &[], &removed, &[]);

        assert_eq!(
            grown,
            ["rust/i0: the retired entry must have the type, the text, the expected labels, and the base locations of the base query"]
        );
    }

    #[test]
    fn a_renamed_query_is_a_removed_query() {
        // A new id must not start a new ledger and rank history for an old
        // query without a record.
        let base = corpus(Vec::new());
        let mut renamed = corpus(Vec::new());
        renamed.cases[0].queries[0].id = "i0-new".to_string();

        let grown = growth(&base, &[entry("rust/i0/hit@1")], &renamed, &[]);

        assert_eq!(grown.len(), 1, "{grown:?}");
        assert!(grown[0].starts_with("rust/i0: the query of the base corpus was removed"));
    }

    #[test]
    fn a_retired_id_must_stay_retired_and_unchanged() {
        let mut base = corpus(Vec::new());
        base.retired.push(retired(
            "rust/old",
            QueryType::Identifier,
            "old text",
            &["helper"],
        ));
        let reused = corpus(vec![serde_json::json!({
            "id": "old", "type": "identifier", "query": "give support", "expected": ["helper"]
        })]);
        let mut edited = corpus(Vec::new());
        edited.retired.push(retired(
            "rust/old",
            QueryType::Identifier,
            "new text",
            &["helper"],
        ));

        assert_eq!(
            growth(&base, &[], &reused, &[]),
            ["rust/old: the id is retired on the base branch and must stay retired"]
        );
        assert_eq!(
            growth(&base, &[], &edited, &[]),
            ["rust/old: the retired entry of the base branch must not change"]
        );
    }

    #[test]
    fn a_retired_query_must_not_come_back_with_a_new_id() {
        // The text comparison ignores case, white space, and the Unicode form,
        // because the tokenizer gives the same tokens for these changes. It
        // keeps punctuation, because two code sketches can differ only in an
        // operator.
        let mut returned = corpus(vec![
            serde_json::json!({"id": "same", "type": "identifier",
                "query": "  Call   THE helper ", "expected": ["helper"]}),
            serde_json::json!({"id": "other-type", "type": "code_sketch",
                "query": "call the helper", "expected": ["helper"]}),
            serde_json::json!({"id": "other-text", "type": "identifier",
                "query": "call the helper!", "expected": ["helper"]}),
        ]);
        returned.retired.push(retired_old());

        let problems = validate(&returned, &case_labels());

        assert_eq!(
            problems,
            ["rust/same: the query is the same as the retired query `rust/old`"]
        );
    }

    #[test]
    fn a_retired_query_with_a_label_for_the_same_symbol_is_the_same() {
        // A new label id for the symbol of a retired label does not make a new
        // query.
        let mut labels = labels();
        let mut alias = labels.symbols[0].clone();
        alias.label = "helper_again".to_string();
        labels.symbols.push(alias);
        let labels = BTreeMap::from([("rust/basics".to_string(), labels)]);
        let mut returned = corpus(vec![
            serde_json::json!({"id": "alias", "type": "identifier",
            "query": "call the helper", "expected": ["helper_again"]}),
        ]);
        returned.retired.push(retired_old());

        let problems = validate(&returned, &labels);

        assert_eq!(
            problems,
            ["rust/alias: the query is the same as the retired query `rust/old`"]
        );
    }

    #[test]
    fn a_changed_label_definition_is_a_changed_query() {
        // The label id stays, but it points to a different symbol.
        let corpus = corpus(Vec::new());
        let base_labels = case_labels();
        let mut moved = labels();
        moved.symbols[0].start_line = 30;
        let mut renamed = labels();
        renamed.symbols[0].name = "assist".to_string();
        let mut other_file = labels();
        other_file.symbols[0].file = "c.rs".to_string();

        for changed in [moved, renamed, other_file] {
            let labels = BTreeMap::from([("rust/basics".to_string(), changed)]);

            let grown = ledger_growth(
                &Revision {
                    corpus: &corpus,
                    labels: &base_labels,
                    ledger: &[],
                },
                &Revision {
                    corpus: &corpus,
                    labels: &labels,
                    ledger: &[],
                },
            );

            // Each query of the test corpus with an expected symbol uses `helper`.
            assert_eq!(grown.len(), 12, "{grown:?}");
            assert_eq!(
                grown[0],
                "rust/c0: the definition of the expected label `helper` changed; retire the query id and add a new query"
            );
        }
    }

    #[test]
    fn a_change_of_a_label_that_no_base_query_uses_is_not_growth() {
        let corpus = corpus(Vec::new());
        let base_labels = case_labels();
        let mut changed = labels();
        changed.symbols[1].start_line = 40;
        let labels = BTreeMap::from([("rust/basics".to_string(), changed)]);

        let grown = ledger_growth(
            &Revision {
                corpus: &corpus,
                labels: &base_labels,
                ledger: &[],
            },
            &Revision {
                corpus: &corpus,
                labels: &labels,
                ledger: &[],
            },
        );

        assert_eq!(grown, Vec::<String>::new());
    }

    #[test]
    fn validation_rejects_a_bad_retired_entry() {
        let mut broken = corpus(Vec::new());
        let mut blank = retired("rust", QueryType::Identifier, "a", &[]);
        blank.reason = " ".to_string();
        broken.retired = vec![
            retired(
                "rust/i0",
                QueryType::Identifier,
                "give support",
                &["helper"],
            ),
            blank,
            retired("rust/gone", QueryType::Identifier, "a", &[]),
            retired("rust/gone", QueryType::Identifier, "b", &[]),
        ];

        let problems = validate(&broken, &case_labels()).join("\n");

        for expected in [
            "retired rust/i0: the corpus has a query with this id",
            "retired rust: the id must be `<language>/<query id>`",
            "retired rust: the reason has no text",
            "retired rust/gone: the id occurs more than one time",
        ] {
            assert!(
                problems.contains(expected),
                "no `{expected}` in:\n{problems}"
            );
        }
    }

    #[test]
    fn the_retired_list_is_optional_in_json() {
        let corpus = corpus(Vec::new());

        assert!(corpus.retired.is_empty());
        let with_retired: Corpus = serde_json::from_value(serde_json::json!({
            "version": 1, "split": "development", "oracle": "x", "cases": [],
            "retired": [{"id": "rust/old", "type": "identifier", "query": "old text",
                "expected": ["helper"], "locations": [{"file": "a.rs", "name": "helper", "start_line": 3}],
                "reason": "the label was wrong"}]
        }))
        .unwrap();
        assert_eq!(with_retired.retired[0].id, "rust/old");
        assert_eq!(with_retired.retired[0].query, "old text");
    }

    #[test]
    fn the_order_of_the_expected_labels_is_not_a_change() {
        let base = corpus_with(serde_json::json!({
            "id": "i0", "type": "identifier", "query": "helper", "expected": ["helper", "other"]
        }));
        let current = corpus_with(serde_json::json!({
            "id": "i0", "type": "identifier", "query": "helper", "expected": ["other", "helper"]
        }));
        let ledger = [entry("rust/i0/hit@1")];

        assert_eq!(
            growth(&base, &ledger, &current, &ledger),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_rank_that_is_worse_than_on_the_base_is_growth() {
        let base = corpus(Vec::new());
        let base_ranks = [
            rank("rust/i0", Some(2)),
            rank("rust/i1", Some(3)),
            rank("rust/i2", None),
        ];
        let ranks = [
            rank("rust/fresh", None),
            rank("rust/i0", Some(9)),
            rank("rust/i1", Some(1)),
            rank("rust/i2", None),
            rank("rust/i3", Some(7)),
        ];

        let grown = rank_growth(&base, &base_ranks, &ranks);

        assert_eq!(grown, ["rust/i0: rank 9, the base branch has rank 2"]);
    }

    #[test]
    fn a_lost_symbol_is_rank_growth() {
        let base = corpus(Vec::new());

        let grown = rank_growth(&base, &[rank("rust/i0", Some(4))], &[rank("rust/i0", None)]);

        assert_eq!(grown, ["rust/i0: no rank, the base branch has rank 4"]);
    }
    #[test]
    fn a_natural_language_query_must_be_ascii() {
        // Normalization does not find a letter of a different script that
        // looks the same, such as the Cyrillic `е` in `hеlper`.
        let corpus = corpus(vec![
            serde_json::json!({"id": "cyrillic", "type": "natural_language",
                "query": "call the h\u{435}lper", "expected": ["helper"]}),
            serde_json::json!({"id": "sketch", "type": "code_sketch",
                "query": "fn größe()", "expected": ["helper"]}),
        ]);

        let problems = validate(&corpus, &case_labels());

        assert_eq!(
            problems,
            ["rust/cyrillic: the natural language query must have only ASCII characters"]
        );
    }

    #[test]
    fn a_name_word_in_a_different_unicode_form_is_a_leak() {
        let corpus = corpus(vec![
            serde_json::json!({"id": "wide", "type": "natural_language",
            "query": "the \u{ff48}\u{ff45}\u{ff4c}\u{ff50}\u{ff45}\u{ff52} code", "expected": ["helper"]}),
        ]);

        let problems = validate(&corpus, &case_labels()).join("\n");

        assert!(
            problems.contains("rust/wide: the natural language query has the word `helper`"),
            "{problems}"
        );
    }

    #[test]
    fn a_natural_language_query_must_not_have_a_word_of_the_symbol_name() {
        let named: Labels = serde_json::from_value(serde_json::json!({
            "version": 1,
            "language": "rust",
            "split": "development",
            "oracle": "author-reviewed, not executed",
            "closed_world": false,
            "symbols": [
                {"label": "helper", "category": "free_function", "file": "a.rs", "name": "helper",
                 "kind": "function", "container": null, "start_line": 3, "end_line": 4, "exported": true},
                {"label": "other", "category": "free_function", "file": "b.rs", "name": "twiceLocal",
                 "kind": "function", "container": null, "start_line": 9, "end_line": 9, "exported": true}
            ]
        }))
        .unwrap();
        let labels = BTreeMap::from([("rust/basics".to_string(), named)]);
        let leaking = corpus(vec![
            serde_json::json!({"id": "whole", "type": "natural_language",
                "query": "call the Helper function", "expected": ["helper"]}),
            serde_json::json!({"id": "part", "type": "natural_language",
                "query": "a local value two times", "expected": ["helper", "other"]}),
            serde_json::json!({"id": "clean", "type": "natural_language",
                "query": "give support to the caller", "expected": ["helper", "other"]}),
            serde_json::json!({"id": "identifier", "type": "identifier",
                "query": "helper", "expected": ["helper"]}),
            serde_json::json!({"id": "sketch", "type": "code_sketch",
                "query": "fn helper()", "expected": ["helper"]}),
        ]);

        let problems = validate(&leaking, &labels);

        assert_eq!(
            problems,
            [
                "rust/whole: the natural language query has the word `helper` of the symbol name `helper`",
                "rust/part: the natural language query has the word `local` of the symbol name `twiceLocal`",
            ]
        );
    }
}
