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

use omen::benchmark::truth::{
    bless, check_ledger, compare, coverage_gaps, label_assertion_ids, ledger_growth,
    render_sink_table, render_table, summarize, summarize_sinks, Assertion, ImportedName, Kind,
    Labels, LedgerEntry, Location, Observed, ObservedCall, ObservedImport, ObservedSink,
    ObservedSymbol,
};
use omen::core::Language;
use omen::index::extract::{extract_file, link_files, FileSymbols};
use omen::index::imports::{resolve_imports, Manifests};
use omen::index::model::{
    Exported, ImportBinding, ImportKind as IndexImportKind, ImportTarget, ResolutionStatus, Symbol,
    SymbolKind,
};
use omen::index::resolve::resolve_references;
use omen::index::security::find_sinks;
use omen::parser::Parser;

const LABELS_FILE: &str = "labels.json";
const LEDGER_FILE: &str = "expected_failures.json";
const BLESS_TRACKING: &str = "phase-2-shared-index";
const SINK_TRACKING: &str = "phase-5-security";

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

/// The label model has no kind for a field, a constant, a variable, or a macro.
fn label_kind(kind: SymbolKind) -> Option<Kind> {
    match kind {
        SymbolKind::Function => Some(Kind::Function),
        SymbolKind::Method => Some(Kind::Method),
        SymbolKind::Constructor => Some(Kind::Constructor),
        SymbolKind::Class => Some(Kind::Class),
        SymbolKind::Struct => Some(Kind::Struct),
        SymbolKind::Interface => Some(Kind::Interface),
        SymbolKind::Trait => Some(Kind::Trait),
        SymbolKind::Enum => Some(Kind::Enum),
        SymbolKind::Module => Some(Kind::Module),
        SymbolKind::TypeAlias => Some(Kind::TypeAlias),
        SymbolKind::Field | SymbolKind::Constant | SymbolKind::Variable | SymbolKind::Macro => None,
    }
}

fn observed_symbol(file: &str, symbol: Symbol) -> Option<ObservedSymbol> {
    Some(ObservedSymbol {
        kind: Some(label_kind(symbol.kind)?),
        location: Location {
            file: file.to_string(),
            name: symbol.name,
            start_line: symbol.range.start_line,
        },
        container: symbol.container,
        end_line: symbol.range.end_line,
        exported: match symbol.exported {
            Exported::Exported => Some(true),
            Exported::NotExported => Some(false),
            Exported::Unknown(_) => None,
        },
    })
}

/// Reads each `go.mod` file and each `.gemspec` file below `dir`.
fn read_manifests(root: &Path, dir: &Path, manifests: &mut Manifests) {
    for path in sorted_entries(dir) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            read_manifests(root, &path, manifests);
        } else if name == "go.mod" || name.ends_with(".gemspec") {
            let text = std::fs::read_to_string(&path).unwrap();
            manifests.add(&relative(root, &path), &text);
        }
    }
}

/// A label describes one import statement and one path. Thus the adapter
/// puts the bindings with the same line and the same path in one import.
fn observed_imports(bindings: &[ImportBinding]) -> Vec<ObservedImport> {
    let mut imports: Vec<ObservedImport> = Vec::new();
    for binding in bindings {
        // A Rust `mod name;` declares a module. The labels have no category for it.
        if binding.kind == IndexImportKind::ModuleDeclaration {
            continue;
        }
        let line = binding.range.start_line;
        let position = imports
            .iter()
            .position(|import| import.line == line && import.path == binding.module_path);
        let position = position.unwrap_or_else(|| {
            imports.push(ObservedImport {
                file: binding.file.clone(),
                line,
                path: binding.module_path.clone(),
                names: Vec::new(),
                resolved_files: Vec::new(),
            });
            imports.len() - 1
        });
        // Each binding can target a different file.
        let targets = match &binding.target_file {
            ImportTarget::Resolved(file) => vec![file.clone()],
            ImportTarget::Package(files) | ImportTarget::Ambiguous(files) => files.clone(),
            ImportTarget::External | ImportTarget::Unresolved => Vec::new(),
        };
        for target in targets {
            if !imports[position].resolved_files.contains(&target) {
                imports[position].resolved_files.push(target);
            }
        }
        if let Some(name) = &binding.imported_name {
            imports[position].names.push(ImportedName {
                name: name.clone(),
                alias: binding.local_name.clone().filter(|local| local != name),
            });
        }
    }
    imports
}

/// The calls of the shared index. A call without an enclosing definition has
/// no caller, and the labels do not describe it. An edge exists only for one
/// candidate that the rules selected.
fn observed_calls(files: &[FileSymbols]) -> Vec<ObservedCall> {
    let locations: BTreeMap<&str, Location> = files
        .iter()
        .flat_map(|file| {
            file.symbols.iter().map(|symbol| {
                let location = Location {
                    file: file.file.clone(),
                    name: symbol.name.clone(),
                    start_line: symbol.range.start_line,
                };
                (symbol.key.as_str(), location)
            })
        })
        .collect();
    let mut calls = Vec::new();
    for reference in files.iter().flat_map(|file| &file.references) {
        let Some(caller) = reference.enclosing.as_deref() else {
            continue;
        };
        let candidates: Vec<Location> = reference
            .resolution
            .candidates
            .iter()
            .map(|key| locations[key.as_str()].clone())
            .collect();
        let edge = matches!(
            reference.resolution.status,
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
        );
        let edges = if edge { candidates.clone() } else { Vec::new() };
        // A definition that a later definition replaces is a candidate, but not an edge.
        let superseded = &reference.resolution.superseded;
        let offered = candidates
            .into_iter()
            .chain(superseded.iter().map(|key| locations[key.as_str()].clone()))
            .collect();
        calls.push(ObservedCall {
            caller: locations[caller].clone(),
            call: reference.name.clone(),
            edges,
            candidates: offered,
        });
    }
    calls
}

/// The sinks of the security matcher. A label names the line on which a sink starts.
fn observed_sinks(trees: &[omen::parser::ParseResult], files: &[FileSymbols]) -> Vec<ObservedSink> {
    find_sinks(trees, files)
        .into_iter()
        .map(|sink| ObservedSink {
            file: sink.file,
            line: sink.range.start_line,
            column: sink.range.start_column,
            category: sink.category.name().to_string(),
            basis: sink.basis.name().to_string(),
            in_test: sink.flags.in_test,
            constant_argument: sink.flags.constant_argument,
        })
        .collect()
}

/// Convert the facts that omen reports for one case into the label model.
///
/// The symbols, the imports, and the calls come from the shared index.
///
/// If omen does not report a fact, the adapter gives `None`. It does not
/// calculate a value that omen does not report.
fn index_case(dir: &Path) -> (Vec<omen::parser::ParseResult>, Vec<FileSymbols>) {
    let mut files = Vec::new();
    source_files(dir, &mut files);
    let mut index_files = Vec::new();
    let mut trees = Vec::new();
    for path in &files {
        let file = relative(dir, path);
        let parsed = Parser::new().parse_file(path).expect("parse");
        index_files.push(extract_file(&parsed, &file));
        trees.push(parsed);
    }

    link_files(&mut index_files);
    let mut manifests = Manifests::default();
    read_manifests(dir, dir, &mut manifests);
    resolve_imports(&mut index_files, &manifests);
    resolve_references(&mut index_files);
    (trees, index_files)
}

fn observe(dir: &Path) -> Observed {
    let (trees, index_files) = index_case(dir);
    let mut imports = Vec::new();
    let calls = observed_calls(&index_files);
    let sinks = observed_sinks(&trees, &index_files);
    let mut symbols = Vec::new();
    for index_file in index_files {
        imports.extend(observed_imports(&index_file.imports));
        let file = index_file.file;
        symbols.extend(
            index_file
                .symbols
                .into_iter()
                .filter_map(|symbol| observed_symbol(&file, symbol)),
        );
    }

    Observed {
        symbols,
        calls,
        imports,
        sinks,
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
        println!("{}", render_sink_table(&summarize_sinks(&assertions)));
        for assertion in assertions.iter().filter(|assertion| !assertion.passed) {
            println!("{}\t{}", assertion.id, assertion.detail);
        }
        return;
    }

    let assertions = run_cases(&cases);

    if std::env::var_os("OMEN_TRUTH_BLESS").is_some() {
        let mut blessed = bless(&assertions, &load_ledger(&root), BLESS_TRACKING);
        for entry in &mut blessed {
            if entry.id.contains("/sinks/") && entry.tracking == BLESS_TRACKING {
                entry.tracking = SINK_TRACKING.to_string();
            }
        }
        let mut text = serde_json::to_string_pretty(&blessed).unwrap();
        text.push('\n');
        std::fs::write(root.join(LEDGER_FILE), text).unwrap();
    }

    println!("{}", render_table(&summarize(&assertions)));
    println!("{}", render_sink_table(&summarize_sinks(&assertions)));
    let violations = check_ledger(&assertions, &load_ledger(&root));
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "{} ledger violations:\n{}",
        violations.len(),
        report.join("\n")
    );
}

/// The secret in the fixtures. A sink must not hold it, or a part of it.
const FIXTURE_SECRET: &str = "AKIAIOSFODNN7EXAMPLE";

#[test]
fn no_sink_holds_a_fixture_secret() {
    let mut sources_with_secret = 0;
    let tail = &FIXTURE_SECRET[4..];
    for case in load_cases(&truth_root()) {
        let (trees, files) = index_case(&case.dir);
        sources_with_secret += trees
            .iter()
            .filter(|tree| String::from_utf8_lossy(&tree.source).contains(FIXTURE_SECRET))
            .count();
        let serialized = serde_json::to_string(&find_sinks(&trees, &files)).unwrap();
        assert!(!serialized.contains(FIXTURE_SECRET), "{}", case.id);
        for start in 0..=tail.len() - 6 {
            let part = &tail[start..start + 6];
            assert!(
                !serialized.contains(part),
                "{}: a sink holds a part of the secret: {part}",
                case.id
            );
        }
    }
    assert!(
        sources_with_secret > 10,
        "the fixtures must hold the secret"
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

/// `from pkg import left, right` has one import statement and two bindings. A
/// binding can target a different file. The import keeps each target.
#[test]
fn observed_imports_keep_each_target_of_a_grouped_import() {
    let span = omen::index::model::Span {
        start_byte: 0,
        end_byte: 0,
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 0,
    };
    let binding = |name: &str, target: &str| ImportBinding {
        file: "main.py".to_string(),
        range: span,
        module_path: "pkg".to_string(),
        kind: IndexImportKind::Named,
        imported_name: Some(name.to_string()),
        local_name: Some(name.to_string()),
        target_file: ImportTarget::Resolved(target.to_string()),
        target_symbol: None,
    };
    let bindings = [
        binding("left", "pkg/left.py"),
        binding("right", "pkg/right.py"),
        binding("again", "pkg/right.py"),
    ];
    let imports = observed_imports(&bindings);
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].resolved_files, ["pkg/left.py", "pkg/right.py"]);
    assert_eq!(imports[0].names.len(), 3);
}
