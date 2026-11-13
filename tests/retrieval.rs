//! Retrieval gate. `tests/fixtures/retrieval/corpus.json` holds labelled
//! search queries for the truth fixture cases. This test runs the search
//! engine for each query and compares the results with the labels.
//!
//! `expected_failures.json` lists the queries for which the engine does not
//! give an acceptable symbol. The test fails if a failure is not in the list,
//! or if a listed query is now correct.
//!
//! `ranks.json` holds the rank of the best acceptable symbol of each query.
//! The test fails if a rank is worse or better than the recorded rank.
//!
//! To write the two files again from the current results, run:
//!
//! ```sh
//! OMEN_TRUTH_BLESS=1 cargo test --test retrieval
//! ```
//!
//! Then examine the diff. A new entry is permitted only for a new query.
//!
//! A query of the base branch must not change, and each label that it expects
//! must keep its file, name, and start line in `labels.json`. Thus a change of
//! a truth fixture that moves a symbol needs new queries. To correct a query,
//! move it to `retired` in the corpus, with its type, text, expected labels,
//! and a reason. Then add a new query with a new id. A query that is the same
//! as a retired query is not a new query.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use omen::benchmark::retrieval::{
    assertions, check_ranks, expected_locations, ledger_growth, metrics_by_language_and_type,
    rank_baseline, rank_growth, render_table, validate, worst_case_order, Corpus, Outcome,
    RankEntry, Revision,
};
use omen::benchmark::truth::{bless, check_ledger, Labels, LedgerEntry, Location};
use omen::config::Config;
use omen::semantic::{SearchConfig, SemanticSearch};

const CORPUS_FILE: &str = "corpus.json";
const LEDGER_FILE: &str = "expected_failures.json";
const RANKS_FILE: &str = "ranks.json";
const BLESS_TRACKING: &str = "phase-4-search";

fn retrieval_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/retrieval")
}

fn truth_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/truth")
}

fn load_corpus(root: &Path) -> Corpus {
    let path = root.join(CORPUS_FILE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn load_ledger(root: &Path) -> Vec<LedgerEntry> {
    match std::fs::read_to_string(root.join(LEDGER_FILE)) {
        Ok(text) => serde_json::from_str(&text).expect("the ledger must be valid JSON"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{LEDGER_FILE}: {error}"),
    }
}

fn load_ranks(root: &Path) -> Vec<RankEntry> {
    match std::fs::read_to_string(root.join(RANKS_FILE)) {
        Ok(text) => serde_json::from_str(&text).expect("the rank baseline must be valid JSON"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{RANKS_FILE}: {error}"),
    }
}

/// The labels of each case of the corpus, with the `case` value as the key.
fn load_labels(corpus: &Corpus) -> BTreeMap<String, Labels> {
    load_labels_from(&truth_root(), corpus)
}

/// The labels of each case of the corpus below the truth fixture directory
/// `truth`.
fn load_labels_from(truth: &Path, corpus: &Corpus) -> BTreeMap<String, Labels> {
    corpus
        .cases
        .iter()
        .map(|case| {
            let path = truth.join(&case.case).join("labels.json");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let labels = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (case.case.clone(), labels)
        })
        .collect()
}

/// Run each query of the corpus through the search engine.
///
/// The source files of a case are the documents. The index is in a temporary
/// directory, so the fixture directory does not change.
fn run_engine(corpus: &Corpus, labels: &BTreeMap<String, Labels>) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for case in &corpus.cases {
        let index_directory = tempfile::tempdir().unwrap();
        let config = SearchConfig {
            cache_path: Some(index_directory.path().join("search.db")),
            ..SearchConfig::default()
        };
        let search = SemanticSearch::new(&config, truth_root().join(&case.case)).unwrap();
        search.index(&Config::default()).unwrap();

        for query in &case.queries {
            // The complete list, with the default minimum score of the engine.
            // The cut at a rank occurs after the worst-case order.
            let output = search.search(&query.query, Some(usize::MAX)).unwrap();
            let expected = expected_locations(query, &labels[&case.case]);
            // The engine has no rule for results of equal score, and their
            // order changes between runs. The worst-case order makes the gate
            // give one result.
            let scored = output
                .results
                .into_iter()
                .map(|result| {
                    let location = Location {
                        file: result.file,
                        name: result.symbol_name,
                        start_line: result.start_line,
                    };
                    (result.score, location)
                })
                .collect();
            outcomes.push(Outcome {
                language: case.language.clone(),
                id: query.id.clone(),
                query_type: query.query_type,
                ranked: worst_case_order(scored, &expected),
                expected,
            });
        }
    }
    outcomes
}

#[test]
fn the_corpus_is_complete_for_each_language() {
    let corpus = load_corpus(&retrieval_root());
    let problems = validate(&corpus, &load_labels(&corpus));
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // Each truth case is a document set. A new language needs queries.
    let with_queries: BTreeSet<&str> = corpus.cases.iter().map(|case| case.case.as_str()).collect();
    let mut without_queries = Vec::new();
    for language in std::fs::read_dir(truth_root()).unwrap() {
        let language = language.unwrap().path();
        if !language.is_dir() {
            continue;
        }
        let case = format!("{}/basics", language.file_name().unwrap().to_string_lossy());
        if !with_queries.contains(case.as_str()) {
            without_queries.push(case);
        }
    }
    without_queries.sort();
    assert!(
        without_queries.is_empty(),
        "add queries for: {without_queries:?}"
    );
}

#[test]
fn search_results_agree_with_the_labels_or_the_ledger() {
    let root = retrieval_root();
    let corpus = load_corpus(&root);
    let outcomes = run_engine(&corpus, &load_labels(&corpus));
    let assertions = assertions(&outcomes);

    if std::env::var_os("OMEN_TRUTH_BLESS").is_some() {
        let blessed = bless(&assertions, &load_ledger(&root), BLESS_TRACKING);
        let mut text = serde_json::to_string_pretty(&blessed).unwrap();
        text.push('\n');
        std::fs::write(root.join(LEDGER_FILE), text).unwrap();

        let mut text = serde_json::to_string_pretty(&rank_baseline(&outcomes)).unwrap();
        text.push('\n');
        std::fs::write(root.join(RANKS_FILE), text).unwrap();
    }

    println!("{}", render_table(&metrics_by_language_and_type(&outcomes)));

    let violations = check_ledger(&assertions, &load_ledger(&root));
    let mut report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    // The ledger sees a change only at rank 1 and at rank 10. The rank
    // baseline sees each change of rank.
    let rank_violations = check_ranks(&outcomes, &load_ranks(&root));
    report.extend(rank_violations.iter().map(ToString::to_string));
    assert!(
        report.is_empty(),
        "{} ledger violations and {} rank violations:\n{}",
        violations.len(),
        rank_violations.len(),
        report.join("\n")
    );
}

#[test]
fn search_results_have_one_order() {
    let corpus = load_corpus(&retrieval_root());
    let labels = load_labels(&corpus);
    let first = run_engine(&corpus, &labels);
    let second = run_engine(&corpus, &labels);
    let different: Vec<String> = first
        .iter()
        .zip(&second)
        .filter(|(left, right)| left != right)
        .map(|(left, right)| {
            format!(
                "{}/{}\n  {:?}\n  {:?}",
                left.language, left.id, left.ranked, right.ranked
            )
        })
        .collect();
    assert!(different.is_empty(), "{}", different.join("\n"));
}

/// The ledger must not get an entry for a query of the base branch, a query
/// of the base branch must not change or go without a `retired` entry, and a
/// rank must not get worse. A label that a base query uses must keep its
/// definition. CI runs this test with `OMEN_RETRIEVAL_BASE_DIR` set to
/// `tests/fixtures/retrieval` of the base branch. CI also extracts
/// `tests/fixtures/truth` of the base branch, next to it, for the base labels.
#[test]
#[ignore = "needs OMEN_RETRIEVAL_BASE_DIR"]
fn ledger_never_grows() {
    let base = PathBuf::from(
        std::env::var_os("OMEN_RETRIEVAL_BASE_DIR").expect("set OMEN_RETRIEVAL_BASE_DIR"),
    );
    if !base.join(CORPUS_FILE).is_file() {
        // The base branch does not have the corpus. Thus each entry is new.
        return;
    }
    let root = retrieval_root();
    let base_corpus = load_corpus(&base);
    let base_labels = load_labels_from(&base.join("../truth"), &base_corpus);
    let base_ledger = load_ledger(&base);
    let corpus = load_corpus(&root);
    let labels = load_labels(&corpus);
    let ledger = load_ledger(&root);
    let mut grown = ledger_growth(
        &Revision {
            corpus: &base_corpus,
            labels: &base_labels,
            ledger: &base_ledger,
        },
        &Revision {
            corpus: &corpus,
            labels: &labels,
            ledger: &ledger,
        },
    );
    grown.extend(rank_growth(
        &base_corpus,
        &load_ranks(&base),
        &load_ranks(&root),
    ));
    assert!(
        grown.is_empty(),
        "these queries are worse than on the base branch, or hide a failure:\n{}",
        grown.join("\n")
    );
}
