//! Compares the call edges of `repomap::build_index` with the call edges of
//! the shared index on one repository.
//!
//! This test reads a repository that is not a fixture, thus it does not run
//! by default. Give the root of the repository:
//!
//! ```sh
//! OMEN_DIFFERENTIAL_ROOT=/path/to/repository \
//!     cargo test --release --test index_differential -- --ignored --nocapture
//! ```
//!
//! With `OMEN_DIFFERENTIAL_OUT=/path/to/file`, the test writes each difference
//! to that file. The test prints the counts and the first differences of each
//! class in sorted order. It fails only when it cannot read the repository.
//!
//! A row is one caller and one callee name. The classes are:
//!
//! - `same`: the two indexes give the same targets.
//! - `only_old`: only `build_index` gives an edge.
//! - `only_new`: only the shared index gives an edge.
//! - `different_target`: the two indexes give different targets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use omen::analyzers::repomap::{build_index, ResolutionStatus as OldStatus};
use omen::core::FileSet;
use omen::index::build::CodeIndex;
use omen::index::model::ResolutionStatus;

/// A definition: its file, its first line, and its name.
type Place = (String, u32, String);
/// A caller and the name that it calls.
type Row = (Place, String);

const SHOWN: usize = 25;

/// The name of a call without a receiver or a path.
fn bare_name(call: &str) -> String {
    call.rsplit(['.', ':', '>', '\\'])
        .next()
        .unwrap_or(call)
        .trim()
        .to_string()
}

fn old_edges(root: &Path, files: &[PathBuf]) -> BTreeMap<Row, BTreeSet<Place>> {
    let index = build_index(root, files).expect("build_index");
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
    let place = |location: &omen::analyzers::repomap::SymbolLocation| -> Place {
        let key = (
            location.file.as_str(),
            location.line,
            location.qualified_name.as_str(),
        );
        (
            location.file.replace('\\', "/"),
            location.line,
            names.get(&key).copied().unwrap_or("?").to_string(),
        )
    };
    let mut edges: BTreeMap<Row, BTreeSet<Place>> = BTreeMap::new();
    for resolution in &index.call_resolutions {
        if resolution.status != OldStatus::UniqueCandidate {
            continue;
        }
        let row = (place(&resolution.caller), bare_name(&resolution.call));
        edges
            .entry(row)
            .or_default()
            .extend(resolution.candidates.iter().map(place));
    }
    edges
}

/// The edges of the shared index, and for each row the status and the basis
/// of each call with that name.
fn new_edges(
    index: &CodeIndex,
) -> (
    BTreeMap<Row, BTreeSet<Place>>,
    BTreeMap<Row, BTreeSet<String>>,
) {
    let places: BTreeMap<&str, Place> = index
        .files
        .iter()
        .flat_map(|file| {
            file.symbols.iter().map(|symbol| {
                let place = (
                    file.file.clone(),
                    symbol.range.start_line,
                    symbol.name.clone(),
                );
                (symbol.key.as_str(), place)
            })
        })
        .collect();
    let mut edges: BTreeMap<Row, BTreeSet<Place>> = BTreeMap::new();
    let mut outcomes: BTreeMap<Row, BTreeSet<String>> = BTreeMap::new();
    for reference in index.files.iter().flat_map(|file| &file.references) {
        let Some(caller) = reference.enclosing.as_deref() else {
            continue;
        };
        let row = (places[caller].clone(), bare_name(&reference.name));
        let resolution = &reference.resolution;
        let basis = resolution
            .basis
            .map_or("-".to_string(), |basis| format!("{basis:?}"));
        outcomes
            .entry(row.clone())
            .or_default()
            .insert(format!("{:?}/{basis}", resolution.status));
        let edge = matches!(
            resolution.status,
            ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
        );
        if edge {
            edges.entry(row).or_default().extend(
                resolution
                    .candidates
                    .iter()
                    .map(|key| places[key.as_str()].clone()),
            );
        }
    }
    (edges, outcomes)
}

fn show(place: &Place) -> String {
    format!("{}:{}:{}", place.0, place.1, place.2)
}

fn show_all(places: Option<&BTreeSet<Place>>) -> String {
    let places: Vec<String> = places.into_iter().flatten().map(show).collect();
    if places.is_empty() {
        "-".to_string()
    } else {
        places.join(", ")
    }
}

#[test]
#[ignore = "needs OMEN_DIFFERENTIAL_ROOT"]
fn call_edges_of_the_two_indexes() {
    let root = PathBuf::from(
        std::env::var_os("OMEN_DIFFERENTIAL_ROOT").expect("set OMEN_DIFFERENTIAL_ROOT"),
    );
    let file_set = FileSet::from_path_default(&root).expect("file set");
    let files: Vec<PathBuf> = file_set.iter().map(|path| root.join(path)).collect();
    let old = old_edges(&root, &files);
    let index = CodeIndex::from_files(&root, &files);
    let (new, outcomes) = new_edges(&index);

    let rows: BTreeSet<&Row> = old.keys().chain(new.keys()).collect();
    let mut classes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut by_outcome: BTreeMap<(&str, String), usize> = BTreeMap::new();
    for row in rows {
        let (old_targets, new_targets) = (old.get(row), new.get(row));
        let class = match (old_targets, new_targets) {
            (Some(old_targets), Some(new_targets)) if old_targets == new_targets => "same",
            (Some(_), Some(_)) => "different_target",
            (Some(_), None) => "only_old",
            (None, _) => "only_new",
        };
        let outcome = outcomes.get(row).map_or("no call site".to_string(), |all| {
            all.iter().cloned().collect::<Vec<_>>().join("+")
        });
        *by_outcome.entry((class, outcome.clone())).or_default() += 1;
        classes.entry(class).or_default().push(format!(
            "{} calls {}\told: {}\tnew: {}\t{outcome}",
            show(&row.0),
            row.1,
            show_all(old_targets),
            show_all(new_targets)
        ));
    }

    println!("repository: {}", root.display());
    println!("files: {}", files.len());
    for (class, lines) in &classes {
        println!("{class}: {}", lines.len());
    }
    for ((class, outcome), count) in &by_outcome {
        println!("{class}\t{outcome}\t{count}");
    }
    let mut report = String::new();
    for (class, lines) in &classes {
        if *class == "same" {
            continue;
        }
        println!("== {class}: the first {SHOWN} of {}", lines.len());
        for line in lines.iter().take(SHOWN) {
            println!("{line}");
        }
        for line in lines {
            report.push_str(&format!("{class}\t{line}\n"));
        }
    }
    if let Some(out) = std::env::var_os("OMEN_DIFFERENTIAL_OUT") {
        std::fs::write(out, report).expect("write the report");
    }
}
