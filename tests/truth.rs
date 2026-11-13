//! Ground-truth gate. Each case under `tests/fixtures/truth/<language>/<case>/`
//! holds source files and a `labels.json`. This test compares the facts that
//! omen reports with the labels.
//!
//! `expected_failures.json` lists the facts that omen reports incorrectly. The
//! test fails if a failure is not in the list, or if a listed fact is now
//! correct.
//!
//! To write the list again from the current results, run:
//!
//! ```sh
//! OMEN_TRUTH_BLESS=1 cargo test --test truth
//! ```
//!
//! Then examine the diff. A new entry is permitted only for a new label.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use omen::analyzers::import_resolver::ImportIndex;
use omen::analyzers::repomap::{build_index, ResolutionStatus};
use omen::benchmark::truth::{
    bless, check_ledger, compare, coverage_gaps, label_assertion_ids, ledger_growth, render_table,
    summarize, Assertion, ImportedName, Kind, Labels, LedgerEntry, Location, Observed,
    ObservedCall, ObservedImport, ObservedSymbol,
};
use omen::core::Language;
use omen::parser::{extract_classes, extract_imports, ImportKind, Parser};

const LABELS_FILE: &str = "labels.json";
const LEDGER_FILE: &str = "expected_failures.json";
const BLESS_TRACKING: &str = "phase-2-shared-index";

fn truth_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/truth")
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    entries
}

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for path in sorted_entries(dir) {
        if path.is_dir() {
            source_files(&path, out);
        } else if Language::detect(&path).is_some() {
            out.push(path);
        }
    }
}

fn language_name(language: Language) -> String {
    serde_json::to_value(language)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

struct Case {
    id: String,
    dir: PathBuf,
    labels: Labels,
}

fn load_cases(root: &Path) -> Vec<Case> {
    let mut cases = Vec::new();
    for language_dir in sorted_entries(root) {
        // With `OMEN_TRUTH_LANGUAGE`, do not read the cases of other languages.
        // An author can then work while another case is not complete.
        let other_language = std::env::var("OMEN_TRUTH_LANGUAGE").is_ok_and(|only| {
            language_dir
                .file_name()
                .is_some_and(|name| name != only.as_str())
        });
        if !language_dir.is_dir() || other_language {
            continue;
        }
        for dir in sorted_entries(&language_dir) {
            let labels_path = dir.join(LABELS_FILE);
            let text = std::fs::read_to_string(&labels_path)
                .unwrap_or_else(|error| panic!("{}: {error}", labels_path.display()));
            let labels: Labels = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", labels_path.display()));
            let language = language_dir.file_name().unwrap().to_string_lossy();
            assert_eq!(
                language,
                language_name(labels.language),
                "{}: the directory name and the label language must agree",
                labels_path.display()
            );
            let case = dir.file_name().unwrap().to_string_lossy();
            cases.push(Case {
                id: format!("{language}/{case}"),
                dir,
                labels,
            });
        }
    }
    cases
}

fn load_ledger(root: &Path) -> Vec<LedgerEntry> {
    match std::fs::read_to_string(root.join(LEDGER_FILE)) {
        Ok(text) => serde_json::from_str(&text).expect("the ledger must be valid JSON"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{LEDGER_FILE}: {error}"),
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// Convert the facts that omen reports for one case into the label model.
///
/// This adapter uses the same functions as the `symbol`, `impact`, `repomap`,
/// `outline`, and `graph` commands. If omen does not report a fact, the adapter
/// gives `None`. It does not calculate a value that omen does not report.
fn observe(dir: &Path) -> Observed {
    let mut files = Vec::new();
    source_files(dir, &mut files);
    let index = build_index(dir, &files).expect("build_index");

    let mut symbols: Vec<ObservedSymbol> = index
        .symbols
        .iter()
        .map(|symbol| ObservedSymbol {
            location: Location {
                file: symbol.file.replace('\\', "/"),
                name: symbol.name.clone(),
                start_line: symbol.line,
            },
            // The index gives the kind `function` to each symbol.
            kind: Some(Kind::Function),
            container: symbol.owner.clone(),
            end_line: symbol.end_line,
            exported: Some(symbol.is_exported),
        })
        .collect();

    let names: BTreeMap<(&str, u32, &str), &str> = index
        .symbols
        .iter()
        .map(|symbol| {
            (
                (
                    symbol.file.as_str(),
                    symbol.line,
                    symbol.qualified_name.as_str(),
                ),
                symbol.name.as_str(),
            )
        })
        .collect();
    let locate = |location: &omen::analyzers::repomap::SymbolLocation| Location {
        file: location.file.replace('\\', "/"),
        name: names[&(
            location.file.as_str(),
            location.line,
            location.qualified_name.as_str(),
        )]
            .to_string(),
        start_line: location.line,
    };
    let calls = index
        .call_resolutions
        .iter()
        .map(|resolution| {
            let candidates: Vec<Location> = resolution.candidates.iter().map(locate).collect();
            ObservedCall {
                caller: locate(&resolution.caller),
                call: resolution.call.clone(),
                edges: if resolution.status == ResolutionStatus::UniqueCandidate {
                    candidates.clone()
                } else {
                    Vec::new()
                },
                candidates,
            }
        })
        .collect();

    let import_index = ImportIndex::new(&files, dir);
    let mut imports = Vec::new();
    for path in &files {
        let file = relative(dir, path);
        let parsed = Parser::new().parse_file(path).expect("parse");
        for class in extract_classes(&parsed) {
            symbols.push(ObservedSymbol {
                location: Location {
                    file: file.clone(),
                    name: class.name,
                    start_line: class.start_line,
                },
                // The parser does not report which type-like kind this is.
                kind: None,
                container: None,
                end_line: class.end_line,
                exported: Some(class.is_exported),
            });
        }
        for import in extract_imports(&parsed) {
            if import.kind != ImportKind::Use {
                continue;
            }
            imports.push(ObservedImport {
                file: file.clone(),
                line: import.line,
                names: import
                    .names
                    .into_iter()
                    .map(|name| ImportedName { name, alias: None })
                    .collect(),
                resolved_files: import_index
                    .find_match(&import.path, Path::new(&file))
                    .into_iter()
                    .map(|target| target.replace('\\', "/"))
                    .collect(),
                path: import.path,
            });
        }
    }

    Observed {
        symbols,
        calls,
        imports,
    }
}

fn run_cases(cases: &[Case]) -> Vec<Assertion> {
    let mut assertions = Vec::new();
    for case in cases {
        let observed = observe(&case.dir);
        assertions.extend(
            compare(&case.id, &case.labels, &observed)
                .unwrap_or_else(|error| panic!("{}: {error}", case.id)),
        );
    }
    let mut seen = BTreeSet::new();
    for assertion in &assertions {
        assert!(
            seen.insert(&assertion.id),
            "duplicate assertion {}",
            assertion.id
        );
    }
    assertions
}

#[test]
fn reported_facts_agree_with_labels_or_the_ledger() {
    let root = truth_root();
    let mut cases = load_cases(&root);
    assert!(!cases.is_empty(), "no fixture cases in {}", root.display());

    // An author who writes a new case uses this mode to see each failure of one
    // language. This mode does not read or enforce the ledger.
    if let Ok(language) = std::env::var("OMEN_TRUTH_LANGUAGE") {
        cases.retain(|case| language_name(case.labels.language) == language);
        assert!(!cases.is_empty(), "no fixture cases for {language}");
        let assertions = run_cases(&cases);
        println!("{}", render_table(&summarize(&assertions)));
        for assertion in assertions.iter().filter(|assertion| !assertion.passed) {
            println!("{}\t{}", assertion.id, assertion.detail);
        }
        return;
    }

    let assertions = run_cases(&cases);

    if std::env::var_os("OMEN_TRUTH_BLESS").is_some() {
        let blessed = bless(&assertions, &load_ledger(&root), BLESS_TRACKING);
        let mut text = serde_json::to_string_pretty(&blessed).unwrap();
        text.push('\n');
        std::fs::write(root.join(LEDGER_FILE), text).unwrap();
    }

    println!("{}", render_table(&summarize(&assertions)));
    let violations = check_ledger(&assertions, &load_ledger(&root));
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "{} ledger violations:\n{}",
        violations.len(),
        report.join("\n")
    );
}

#[test]
fn each_language_labels_each_category() {
    let cases = load_cases(&truth_root());
    let mut by_language: BTreeMap<String, Vec<&Labels>> = BTreeMap::new();
    for case in &cases {
        by_language
            .entry(language_name(case.labels.language))
            .or_default()
            .push(&case.labels);
    }
    let gaps: Vec<String> = by_language
        .iter()
        .flat_map(|(language, labels)| {
            coverage_gaps(labels)
                .into_iter()
                .map(move |gap| format!("{language}: {gap}"))
        })
        .collect();
    assert!(
        gaps.is_empty(),
        "label each category, or add a `not_applicable` entry with a reason:\n{}",
        gaps.join("\n")
    );
}

const LANGUAGES: [Language; 14] = [
    Language::Go,
    Language::Rust,
    Language::Python,
    Language::TypeScript,
    Language::JavaScript,
    Language::Tsx,
    Language::Jsx,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::CSharp,
    Language::Ruby,
    Language::Php,
    Language::Bash,
];

/// A new `Language` variant does not compile here until `LANGUAGES` has it.
fn listed(language: Language) -> bool {
    match language {
        Language::Go
        | Language::Rust
        | Language::Python
        | Language::TypeScript
        | Language::JavaScript
        | Language::Tsx
        | Language::Jsx
        | Language::Java
        | Language::C
        | Language::Cpp
        | Language::CSharp
        | Language::Ruby
        | Language::Php
        | Language::Bash => LANGUAGES.contains(&language),
    }
}

#[test]
fn each_supported_language_has_a_case() {
    if std::env::var_os("OMEN_TRUTH_LANGUAGE").is_some() {
        return;
    }
    let with_case: BTreeSet<String> = load_cases(&truth_root())
        .iter()
        .map(|case| language_name(case.labels.language))
        .collect();
    let missing: Vec<String> = LANGUAGES
        .iter()
        .filter(|language| listed(**language))
        .map(|language| language_name(*language))
        .filter(|name| !with_case.contains(name))
        .collect();
    assert!(missing.is_empty(), "no fixture case for: {missing:?}");
}

/// CI runs this test with `OMEN_TRUTH_BASE_DIR` set to the fixture directory of
/// the base branch.
#[test]
#[ignore = "needs OMEN_TRUTH_BASE_DIR"]
fn ledger_never_grows() {
    let base =
        PathBuf::from(std::env::var_os("OMEN_TRUTH_BASE_DIR").expect("set OMEN_TRUTH_BASE_DIR"));
    if !base.is_dir() {
        // The base branch does not have fixtures. Thus each entry is new.
        return;
    }
    let base_cases = load_cases(&base);
    let mut base_ids = BTreeSet::new();
    for case in &base_cases {
        base_ids.extend(label_assertion_ids(&case.id, &case.labels).unwrap());
    }
    let base_case_ids = base_cases.iter().map(|case| case.id.clone()).collect();
    let grown = ledger_growth(
        &base_ids,
        &base_case_ids,
        &load_ledger(&base),
        &load_ledger(&truth_root()),
    );
    assert!(
        grown.is_empty(),
        "these facts were correct on the base branch and are now in the ledger:\n{}",
        grown.join("\n")
    );
}
