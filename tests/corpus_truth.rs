//! Absolute check of the tokenized call corpora in `tests/fixtures/quality/`.
//!
//! `omen benchmark` compares two binaries on these corpora. It finds only a
//! difference between the binaries. This test compares the current source with
//! the `expected` list of each case.
//!
//! `expected` is a closed list of the direct callees of the query symbol. This
//! test makes these checks from it:
//!
//! - `edges/required`: omen reports an edge to each callee in the list.
//! - `edges/forbidden`: omen reports no edge to a callee that is not in the list.
//! - `offers/required`: omen offers each callee in the list as a candidate.
//!
//! `expected_failures.json` lists the checks that fail. The test fails if a
//! failure is not in the list, or if a listed check now passes.
//!
//! To write the list again from the current results, run:
//!
//! ```sh
//! OMEN_TRUTH_BLESS=1 cargo test --test corpus_truth
//! ```
//!
//! Then examine the diff. Do not change `expected` in a corpus to remove an entry.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use omen::analyzers::repomap::build_index;
use omen::benchmark::truth::{
    bless, check_ledger, ledger_growth, render_table, summarize, Assertion, LedgerEntry,
};
use omen::benchmark::{load_cases, materialize};
use omen::core::FileSet;
use omen::index::build::CodeIndex;
use omen::index::model::ResolutionStatus;
use serde_json::Value;

/// `real.json` is not in this list. Its cases need checkouts of other
/// repositories, and this test does not use the network.
const CORPORA: [&str; 2] = ["corpus", "scoped"];
const LEDGER_FILE: &str = "expected_failures.json";
/// The ledger of the checks that fail with the shared index and its resolver.
const INDEX_LEDGER_FILE: &str = "expected_failures_index.json";
const BLESS_TRACKING: &str = "phase-2-shared-index";
/// The paired harness uses the same query when a case does not give one.
const DEFAULT_QUERY: &str = "caller";
const EDGES: &str = "edges";
const OFFERS: &str = "offers";

fn quality_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quality")
}

/// The expectations of one corpus case.
struct Expectation<'a> {
    corpus: &'a str,
    language: &'a str,
    case: &'a str,
    query: &'a str,
    expected: &'a [String],
}

/// The callees that omen reports for the query symbol of one case.
#[derive(Default)]
struct Observation {
    edges: BTreeSet<String>,
    candidates: BTreeSet<String>,
}

impl<'a> Expectation<'a> {
    fn of_case(corpus: &'a str, case: &'a Value, expected: &'a [String]) -> Self {
        let text = |key: &str| case.get(key).and_then(Value::as_str);
        Self {
            corpus,
            language: text("language").unwrap_or_default(),
            case: text("id").unwrap_or_default(),
            query: text("query").unwrap_or(DEFAULT_QUERY),
            expected,
        }
    }
}

/// The first two parts of each id of a case: `<language>/<corpus>.<case>`.
fn case_prefix(expectation: &Expectation) -> String {
    format!(
        "{}/{}.{}",
        expectation.language, expectation.corpus, expectation.case
    )
}

/// The assertion ids and the case prefixes of a corpus. This function does not
/// run omen, thus it can read the corpus of a different revision.
fn expectation_ids(corpus: &str, cases: &[Value]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut ids = BTreeSet::new();
    let mut case_ids = BTreeSet::new();
    for case in cases {
        let expected = strings(case.get("expected"));
        let expectation = Expectation::of_case(corpus, case, &expected);
        let prefix = case_prefix(&expectation);
        let assertions =
            case_assertions(&expectation, None).unwrap_or_else(|error| panic!("{prefix}: {error}"));
        ids.extend(assertions.into_iter().map(|assertion| assertion.id));
        case_ids.insert(prefix);
    }
    (ids, case_ids)
}

/// Make the assertions of one case. `observed` is `None` when omen finds no
/// symbol for the query.
///
/// An id is `<language>/<corpus>.<case>/<dimension>/<kind>/<caller>-><callee>`.
/// `summarize` reads the first and third parts, thus the language is first.
fn case_assertions(
    expectation: &Expectation,
    observed: Option<&Observation>,
) -> Result<Vec<Assertion>, String> {
    for (name, part) in [
        ("corpus", expectation.corpus),
        ("language", expectation.language),
        ("case", expectation.case),
    ] {
        if part.is_empty() || part.contains('/') || part.chars().any(char::is_control) {
            return Err(format!(
                "the {name} name {part:?} must be non-empty, without '/' and control characters"
            ));
        }
    }
    let callees: BTreeSet<&str> = expectation.expected.iter().map(String::as_str).collect();
    if callees.len() != expectation.expected.len() {
        return Err("a callee occurs twice in `expected`".to_string());
    }

    let prefix = case_prefix(expectation);
    // `escape_debug` keeps a line break in a name out of the id.
    let query = expectation.query.escape_debug();
    let no_symbol = format!("omen finds no symbol for the query {query}");
    let check = |id: String, failure: Option<String>| {
        let failure = match observed {
            Some(_) => failure,
            None => Some(no_symbol.clone()),
        };
        Assertion {
            id,
            passed: failure.is_none(),
            detail: failure.unwrap_or_default(),
        }
    };
    let empty = Observation::default();
    let found = observed.unwrap_or(&empty);

    let mut assertions = Vec::new();
    for callee in &callees {
        let target = format!("{query}->{}", callee.escape_debug());
        assertions.push(check(
            format!("{prefix}/{EDGES}/required/{target}"),
            (!found.edges.contains(*callee)).then(|| format!("omen reports no edge to {callee}")),
        ));
        assertions.push(check(
            format!("{prefix}/{OFFERS}/required/{target}"),
            (!found.candidates.contains(*callee))
                .then(|| format!("omen does not offer {callee} as a candidate")),
        ));
    }
    let false_edges: Vec<&str> = found
        .edges
        .iter()
        .map(String::as_str)
        .filter(|edge| !callees.contains(edge))
        .collect();
    assertions.push(check(
        format!("{prefix}/{EDGES}/forbidden/{query}->*"),
        (!false_edges.is_empty())
            .then(|| format!("omen reports a false edge to {}", false_edges.join(", "))),
    ));
    Ok(assertions)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Collect the files as the `symbol` command does with the default
/// configuration, then read the callees of the first symbol that matches.
fn observe(root: &Path, query: &str) -> Option<Observation> {
    let file_set = FileSet::from_path_default(root).expect("file set");
    let files: Vec<PathBuf> = file_set.iter().map(|path| root.join(path)).collect();
    let index = build_index(root, &files).expect("build_index");
    let primary = *index.resolve(query).first()?;
    let symbol = &index.symbols[primary];
    Some(Observation {
        edges: index
            .callees(&[primary], 1)
            .into_iter()
            .flatten()
            .map(|callee| index.symbols[callee].qualified_name.clone())
            .collect(),
        candidates: index
            .call_resolutions
            .iter()
            .filter(|resolution| {
                resolution.caller.qualified_name == symbol.qualified_name
                    && resolution.caller.file == symbol.file
                    && resolution.caller.line == symbol.line
            })
            .flat_map(|resolution| &resolution.candidates)
            .map(|candidate| candidate.qualified_name.clone())
            .collect(),
    })
}

/// Collect the same files, then read the callees of the first symbol with the
/// name of the query from the shared index and its resolver.
///
/// A corpus names a callee as `<file>:<name>`.
fn observe_index(root: &Path, query: &str) -> Option<Observation> {
    let file_set = FileSet::from_path_default(root).expect("file set");
    let files: Vec<PathBuf> = file_set.iter().map(|path| root.join(path)).collect();
    let index = CodeIndex::from_files(root, &files);
    let names: BTreeMap<&str, String> = index
        .files
        .iter()
        .flat_map(|file| {
            file.symbols.iter().map(|symbol| {
                (
                    symbol.key.as_str(),
                    format!("{}:{}", file.file, symbol.name),
                )
            })
        })
        .collect();
    let caller = index
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .find(|symbol| symbol.name == query)?;
    let mut observation = Observation::default();
    let calls = index
        .files
        .iter()
        .flat_map(|file| &file.references)
        .filter(|reference| reference.enclosing.as_deref() == Some(caller.key.as_str()));
    for call in calls {
        let resolution = &call.resolution;
        let offered = resolution.candidates.iter().chain(&resolution.superseded);
        observation
            .candidates
            .extend(offered.map(|key| names[key.as_str()].clone()));
        let edge = matches!(
            resolution.status,
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
        );
        if edge {
            observation.edges.extend(
                resolution
                    .candidates
                    .iter()
                    .map(|key| names[key.as_str()].clone()),
            );
        }
    }
    Some(observation)
}

fn run_corpus(corpus: &str, observe: fn(&Path, &str) -> Option<Observation>) -> Vec<Assertion> {
    let cases = load_corpus(&quality_root(), corpus);
    let mut assertions = Vec::new();
    for case in &cases {
        let expected = strings(case.get("expected"));
        let expectation = Expectation::of_case(corpus, case, &expected);
        let id = expectation.case;
        let scratch = tempfile::Builder::new()
            .prefix("omen-corpus-truth-")
            .tempdir()
            .expect("temp dir");
        materialize(case, scratch.path()).unwrap_or_else(|error| panic!("{corpus}.{id}: {error}"));
        let observed = observe(scratch.path(), expectation.query);
        assertions.extend(
            case_assertions(&expectation, observed.as_ref())
                .unwrap_or_else(|error| panic!("{corpus}.{id}: {error}")),
        );
    }
    assertions
}

fn load_corpus(root: &Path, corpus: &str) -> Vec<Value> {
    let path = root.join(format!("{corpus}.json"));
    load_cases(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn load_ledger_file(root: &Path, file: &str) -> Vec<LedgerEntry> {
    match std::fs::read_to_string(root.join(file)) {
        Ok(text) => serde_json::from_str(&text).expect("the ledger must be valid JSON"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{file}: {error}"),
    }
}

fn load_ledger(root: &Path) -> Vec<LedgerEntry> {
    load_ledger_file(root, LEDGER_FILE)
}

#[test]
fn corpus_expectations_agree_with_omen_or_the_ledger() {
    check_corpora("commands", observe, LEDGER_FILE);
}

/// The same corpora and the same three rules for the shared index. The
/// commands do not use this index until the two ledgers are examined.
#[test]
fn corpus_expectations_agree_with_the_index_or_its_ledger() {
    check_corpora("index", observe_index, INDEX_LEDGER_FILE);
}

fn check_corpora(source: &str, observe: fn(&Path, &str) -> Option<Observation>, ledger_file: &str) {
    let mut assertions = Vec::new();
    for corpus in CORPORA {
        let found = run_corpus(corpus, observe);
        assert!(!found.is_empty(), "no checks in {corpus}.json");
        // One table for each corpus. Do not add the corpora together.
        println!(
            "source: {source}, corpus: {corpus}\n{}",
            render_table(&summarize(&found))
        );
        assertions.extend(found);
    }

    let mut seen = BTreeSet::new();
    for assertion in &assertions {
        assert!(
            seen.insert(&assertion.id),
            "duplicate assertion {}",
            assertion.id
        );
    }

    if std::env::var_os("OMEN_TRUTH_BLESS").is_some() {
        let ledger = load_ledger_file(&quality_root(), ledger_file);
        let blessed = bless(&assertions, &ledger, BLESS_TRACKING);
        let mut text = serde_json::to_string_pretty(&blessed).unwrap();
        text.push('\n');
        std::fs::write(quality_root().join(ledger_file), text).unwrap();
    }

    let ledger = load_ledger_file(&quality_root(), ledger_file);
    let violations = check_ledger(&assertions, &ledger);
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "{} ledger violations:\n{}",
        violations.len(),
        report.join("\n")
    );
}

/// CI runs this test with `OMEN_QUALITY_BASE_DIR` set to the fixture directory
/// of the base branch.
#[test]
#[ignore = "needs OMEN_QUALITY_BASE_DIR"]
fn ledger_never_grows() {
    let base = PathBuf::from(
        std::env::var_os("OMEN_QUALITY_BASE_DIR").expect("set OMEN_QUALITY_BASE_DIR"),
    );
    if !base.join(LEDGER_FILE).is_file() {
        // The base branch has no ledger, thus it did not make these checks and
        // this test cannot know which of them the base branch passed. The pull
        // request that adds the ledger must show the paired benchmark result.
        return;
    }
    let mut base_ids = BTreeSet::new();
    let mut base_case_ids = BTreeSet::new();
    for corpus in CORPORA {
        // A corpus that the base branch does not have gives only new checks.
        if !base.join(format!("{corpus}.json")).is_file() {
            continue;
        }
        let (ids, case_ids) = expectation_ids(corpus, &load_corpus(&base, corpus));
        base_ids.extend(ids);
        base_case_ids.extend(case_ids);
    }
    let grown = ledger_growth(
        &base_ids,
        &base_case_ids,
        &load_ledger(&base),
        &load_ledger(&quality_root()),
    );
    assert!(
        grown.is_empty(),
        "the base branch made these checks and they are now in the ledger:\n{}",
        grown.join("\n")
    );
}

fn expected(callees: &[&str]) -> Vec<String> {
    callees.iter().map(|callee| (*callee).to_string()).collect()
}

fn observation(edges: &[&str], candidates: &[&str]) -> Observation {
    Observation {
        edges: expected(edges).into_iter().collect(),
        candidates: expected(candidates).into_iter().collect(),
    }
}

fn assertions_for(expected: &[String], observed: Option<&Observation>) -> Vec<Assertion> {
    let expectation = Expectation {
        corpus: "scoped",
        language: "rust",
        case: "rust_alias",
        query: "caller",
        expected,
    };
    case_assertions(&expectation, observed).expect("assertions")
}

fn outcomes(assertions: &[Assertion]) -> Vec<(&str, bool)> {
    assertions
        .iter()
        .map(|assertion| (assertion.id.as_str(), assertion.passed))
        .collect()
}

#[test]
fn a_correct_observation_passes_each_check() {
    let assertions = assertions_for(
        &expected(&["a.rs:helper"]),
        Some(&observation(&["a.rs:helper"], &["a.rs:helper"])),
    );
    assert_eq!(
        outcomes(&assertions),
        [
            (
                "rust/scoped.rust_alias/edges/required/caller->a.rs:helper",
                true
            ),
            (
                "rust/scoped.rust_alias/offers/required/caller->a.rs:helper",
                true
            ),
            ("rust/scoped.rust_alias/edges/forbidden/caller->*", true),
        ]
    );
}

#[test]
fn an_ambiguous_call_fails_the_edge_and_passes_the_offer() {
    let assertions = assertions_for(
        &expected(&["a.rs:helper"]),
        Some(&observation(&[], &["a.rs:helper", "b.rs:helper"])),
    );
    assert_eq!(
        outcomes(&assertions)
            .iter()
            .map(|(_, passed)| *passed)
            .collect::<Vec<bool>>(),
        [false, true, true]
    );
    assert_eq!(assertions[0].detail, "omen reports no edge to a.rs:helper");
}

#[test]
fn an_incorrect_target_fails_the_required_and_the_forbidden_edge() {
    let assertions = assertions_for(
        &expected(&["a.rs:helper"]),
        Some(&observation(
            &["b.rs:helper", "c.rs:helper"],
            &["b.rs:helper"],
        )),
    );
    assert_eq!(
        outcomes(&assertions)
            .iter()
            .map(|(_, passed)| *passed)
            .collect::<Vec<bool>>(),
        [false, false, false]
    );
    assert_eq!(
        assertions[1].detail,
        "omen does not offer a.rs:helper as a candidate"
    );
    assert_eq!(
        assertions[2].detail,
        "omen reports a false edge to b.rs:helper, c.rs:helper"
    );
}

#[test]
fn an_empty_list_makes_only_the_forbidden_check() {
    let assertions = assertions_for(&[], Some(&observation(&[], &["main.rs:helper"])));
    assert_eq!(
        outcomes(&assertions),
        [("rust/scoped.rust_alias/edges/forbidden/caller->*", true)]
    );
}

#[test]
fn a_query_without_a_symbol_fails_each_check() {
    let assertions = assertions_for(&expected(&["a.rs:helper"]), None);
    assert_eq!(assertions.len(), 3);
    for assertion in &assertions {
        assert!(!assertion.passed, "{}", assertion.id);
        assert_eq!(
            assertion.detail,
            "omen finds no symbol for the query caller"
        );
    }
}

#[test]
fn an_id_stays_on_one_line() {
    let assertions = assertions_for(&expected(&["a.rs:hel\nper"]), None);
    assert_eq!(
        assertions[0].id,
        "rust/scoped.rust_alias/edges/required/caller->a.rs:hel\\nper"
    );
}

#[test]
fn a_case_name_with_a_separator_is_an_error() {
    for (language, case) in [
        ("rust", "a/b"),
        ("ru/st", "a"),
        ("rust", ""),
        ("rust", "a\nb"),
    ] {
        let expectation = Expectation {
            corpus: "scoped",
            language,
            case,
            query: "caller",
            expected: &[],
        };
        assert!(
            case_assertions(&expectation, None).is_err(),
            "{language:?} {case:?}"
        );
    }
}

#[test]
fn a_callee_that_occurs_twice_is_an_error() {
    let expectation = Expectation {
        corpus: "scoped",
        language: "rust",
        case: "rust_alias",
        query: "caller",
        expected: &expected(&["a.rs:helper", "a.rs:helper"]),
    };
    assert!(case_assertions(&expectation, None).is_err());
}

#[test]
fn the_ids_of_a_corpus_come_from_the_expectations_alone() {
    let cases: Vec<Value> = serde_json::from_str(
        r#"[{"id":"go_unique","language":"go","files":{},"expected":["z/helper.go:Helper"]},
            {"id":"ts_invisible","language":"typescript","query":"main.ts:caller","files":{},"expected":[]}]"#,
    )
    .expect("cases");
    let (ids, case_ids) = expectation_ids("corpus", &cases);
    assert_eq!(
        ids.iter().map(String::as_str).collect::<Vec<&str>>(),
        [
            "go/corpus.go_unique/edges/forbidden/caller->*",
            "go/corpus.go_unique/edges/required/caller->z/helper.go:Helper",
            "go/corpus.go_unique/offers/required/caller->z/helper.go:Helper",
            "typescript/corpus.ts_invisible/edges/forbidden/main.ts:caller->*",
        ]
    );
    assert_eq!(
        case_ids.iter().map(String::as_str).collect::<Vec<&str>>(),
        ["go/corpus.go_unique", "typescript/corpus.ts_invisible"]
    );
}

#[test]
fn an_observation_does_not_change_the_ids() {
    let callees = expected(&["a.rs:helper"]);
    let ids = |observed: Option<&Observation>| -> Vec<String> {
        assertions_for(&callees, observed)
            .into_iter()
            .map(|assertion| assertion.id)
            .collect()
    };
    assert_eq!(
        ids(None),
        ids(Some(&observation(&["b.rs:helper"], &["c.rs:helper"])))
    );
}
