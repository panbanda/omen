//! Two definitions with one name in one file must be two search results.

use std::path::{Path, PathBuf};

use omen::config::Config;
use omen::semantic::{SearchConfig, SemanticSearch};

fn fixture(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/search_identity")
        .join(language)
}

/// The start lines of the results for `name`, in order.
fn start_lines(language: &str, query: &str, name: &str) -> Vec<u32> {
    let index_directory = tempfile::tempdir().unwrap();
    let config = SearchConfig {
        cache_path: Some(index_directory.path().join("search.db")),
        min_score: 0.0,
        ..SearchConfig::default()
    };
    let search = SemanticSearch::new(&config, fixture(language)).unwrap();
    search.index(&Config::default()).unwrap();
    let mut lines: Vec<u32> = search
        .search(query, Some(usize::MAX))
        .unwrap()
        .results
        .into_iter()
        .filter(|result| result.symbol_name == name)
        .map(|result| result.start_line)
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn rust_nested_function_keeps_its_own_result() {
    assert_eq!(start_lines("rust", "only_here", "only_here"), vec![1, 6]);
}

#[test]
fn rust_methods_of_two_types_are_two_results() {
    assert_eq!(start_lines("rust", "area", "area"), vec![20, 26]);
}

#[test]
fn python_methods_of_two_classes_are_two_results() {
    assert_eq!(start_lines("python", "area", "area"), vec![6, 11]);
}

#[test]
fn go_methods_of_two_types_are_two_results() {
    assert_eq!(start_lines("go", "Area", "Area"), vec![7, 11]);
}

#[test]
fn java_methods_of_two_classes_are_two_results() {
    assert_eq!(start_lines("java", "area", "area"), vec![5, 13]);
}

#[test]
fn typescript_methods_of_two_classes_are_two_results() {
    assert_eq!(start_lines("typescript", "area", "area"), vec![4, 12]);
}

/// Search with the default configuration, as the CLI does.
fn default_search_names(directory: &Path, query: &str) -> Vec<String> {
    let index_directory = tempfile::tempdir().unwrap();
    let config = SearchConfig {
        cache_path: Some(index_directory.path().join("search.db")),
        ..SearchConfig::default()
    };
    let search = SemanticSearch::new(&config, directory).unwrap();
    search.index(&Config::default()).unwrap();
    search
        .search(query, Some(usize::MAX))
        .unwrap()
        .results
        .into_iter()
        .map(|result| result.symbol_name)
        .collect()
}

#[test]
fn a_correct_result_stays_when_unrelated_code_is_added() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixture("go").join("shapes.go"),
        directory.path().join("shapes.go"),
    )
    .unwrap();
    let query = "circle";
    let before = default_search_names(directory.path(), query);
    assert!(before.contains(&"Area".to_string()), "{before:?}");

    for number in 0..60 {
        std::fs::write(
            directory.path().join(format!("unrelated{number}.go")),
            format!(
                "package unrelated\n\nfunc Draw{number}(circle int) int {{\n\treturn circle + {number}\n}}\n"
            ),
        )
        .unwrap();
    }
    let after = default_search_names(directory.path(), query);
    assert!(after.contains(&"Area".to_string()), "{after:?}");
}

#[test]
fn a_query_without_a_shared_term_has_no_result() {
    let names = default_search_names(&fixture("go"), "zzqx wvvk");
    assert!(names.is_empty(), "{names:?}");
}
