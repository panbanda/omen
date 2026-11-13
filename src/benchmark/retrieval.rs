//! Retrieval evaluation: labelled search queries, and the metrics for the
//! ranked results that a search engine gives for them.
//!
//! This module is pure. It reads no file and runs no search. `tests/retrieval.rs`
//! loads the corpus, runs the engine, and calls this module.
//!
//! A query lists one or more acceptable symbols. A query has a hit at `k` if
//! one of these symbols is in the first `k` results. `recall@k` is the fraction
//! of the queries that have a hit at `k`. A query that expects no result is a
//! negative query. It has no recall and no reciprocal rank, so the metrics do
//! not include it.
//!
//! The metrics are for each language and for each query type. This module does
//! not give one number for all queries.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::truth::{Assertion, Labels, Location};

pub const CORPUS_VERSION: u32 = 1;

/// The smallest number of queries of each type for one language.
pub const MINIMUM_QUERIES_OF_EACH_TYPE: usize = 4;

/// The smallest number of queries for one language.
pub const MINIMUM_QUERIES: usize = 12;

/// The number of results that the driver must request for each query.
pub const RESULT_LIMIT: usize = 10;

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
    pub recall_at_1: Option<f64>,
    pub recall_at_5: Option<f64>,
    pub recall_at_10: Option<f64>,
    /// The mean of `1 / rank` of the first acceptable symbol. A query with no
    /// acceptable symbol in its results adds 0.
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
        recall_at_1: mean(hits[0] as f64),
        recall_at_5: mean(hits[1] as f64),
        recall_at_10: mean(hits[2] as f64),
        mrr: mean(reciprocal_ranks),
    }
}

/// The metrics for each language.
pub fn metrics_by_language(outcomes: &[Outcome]) -> BTreeMap<String, Metrics> {
    let languages: BTreeSet<&str> = outcomes
        .iter()
        .map(|outcome| outcome.language.as_str())
        .collect();
    languages
        .into_iter()
        .map(|language| {
            let group = outcomes
                .iter()
                .filter(|outcome| outcome.language == language);
            (language.to_string(), metrics(group))
        })
        .collect()
}

/// The metrics for each query type. Each type has an entry, also a type that
/// has no query.
pub fn metrics_by_type(outcomes: &[Outcome]) -> BTreeMap<String, Metrics> {
    QueryType::ALL
        .into_iter()
        .map(|query_type| {
            let group = outcomes
                .iter()
                .filter(|outcome| outcome.query_type == query_type);
            (query_type.as_str().to_string(), metrics(group))
        })
        .collect()
}

/// The metrics for each language and query type.
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

/// Text table with one row for each group.
pub fn render_table<'a>(
    heading: &str,
    rows: impl IntoIterator<Item = (String, &'a Metrics)>,
) -> String {
    let mut out = format!(
        "{heading:<28} {:>7} {:>8} {:>8} {:>8} {:>9} {:>7}\n",
        "queries", "negative", "recall@1", "recall@5", "recall@10", "mrr"
    );
    for (name, metrics) in rows {
        out.push_str(&format!(
            "{name:<28} {:>7} {:>8} {:>8} {:>8} {:>9} {:>7}\n",
            metrics.queries,
            metrics.negative_queries,
            cell(metrics.recall_at_1),
            cell(metrics.recall_at_5),
            cell(metrics.recall_at_10),
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
                    "query": "helper", "expected": ["helper"]
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
        assert_eq!(result.recall_at_1, Some(0.25));
        assert_eq!(result.recall_at_5, Some(0.5));
        assert_eq!(result.recall_at_10, Some(0.75));
        assert_eq!(result.mrr, Some((1.0 + 1.0 / 5.0 + 1.0 / 10.0) / 4.0));
    }

    #[test]
    fn metrics_are_null_for_a_group_with_no_expected_symbol() {
        let result = metrics(&[outcome("rust", "x", &[], &["a"])]);

        assert_eq!(result.queries, 0);
        assert_eq!(result.negative_queries, 1);
        assert_eq!(result.recall_at_1, None);
        assert_eq!(result.recall_at_5, None);
        assert_eq!(result.recall_at_10, None);
        assert_eq!(result.mrr, None);
        let json = serde_json::to_value(result).unwrap();
        assert!(json["recall_at_10"].is_null());
        assert!(json["mrr"].is_null());
    }

    #[test]
    fn a_negative_query_does_not_change_the_recall() {
        let with_negative = metrics(&[
            outcome("rust", "q", &["a"], &["a"]),
            outcome("rust", "x", &[], &["a"]),
        ]);

        assert_eq!(with_negative.recall_at_1, Some(1.0));
        assert_eq!(with_negative.mrr, Some(1.0));
        assert_eq!(with_negative.negative_queries, 1);
    }

    #[test]
    fn metrics_are_separate_for_each_language() {
        let outcomes = [
            outcome("go", "q", &["a"], &["a"]),
            outcome("rust", "q", &["a"], &["b"]),
        ];

        let table = metrics_by_language(&outcomes);

        assert_eq!(table.len(), 2);
        assert_eq!(table["go"].recall_at_1, Some(1.0));
        assert_eq!(table["rust"].recall_at_1, Some(0.0));
    }

    #[test]
    fn metrics_are_separate_for_each_query_type() {
        let outcomes = [
            typed(QueryType::Identifier, &["a"], &["a"]),
            typed(QueryType::CodeSketch, &["a"], &["b", "a"]),
        ];

        let table = metrics_by_type(&outcomes);

        assert_eq!(table["identifier"].mrr, Some(1.0));
        assert_eq!(table["code_sketch"].mrr, Some(0.5));
        assert_eq!(table["natural_language"].queries, 0);
        assert_eq!(table["natural_language"].mrr, None);
    }

    #[test]
    fn metrics_by_language_and_type_have_a_row_for_each_pair() {
        let table = metrics_by_language_and_type(&[typed(QueryType::Identifier, &["a"], &["a"])]);

        assert_eq!(table.len(), 3);
        let key = ("rust".to_string(), "identifier".to_string());
        assert_eq!(table[&key].recall_at_1, Some(1.0));
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
        let table = metrics_by_type(&[typed(QueryType::Identifier, &["a"], &["b", "a"])]);

        let text = render_table(
            "query type",
            table.iter().map(|(name, metrics)| (name.clone(), metrics)),
        );

        let identifier = text
            .lines()
            .find(|line| line.starts_with("identifier"))
            .unwrap();
        assert!(identifier.contains("0.500"), "{text}");
        let natural = text
            .lines()
            .find(|line| line.starts_with("natural_language"))
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
}
