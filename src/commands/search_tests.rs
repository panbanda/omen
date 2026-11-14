use super::*;
use crate::cli::SearchSubcommand;
use crate::commands::testing::repository;

fn query(text: &str, top: Option<usize>) -> SearchSubcommand {
    use clap::Parser;
    let mut arguments = vec!["omen", "search", "query", text];
    let top_text;
    if let Some(top) = top {
        top_text = top.to_string();
        arguments.extend(["--top", &top_text]);
    }
    match crate::cli::Cli::parse_from(arguments).command {
        crate::cli::Command::Search(command) => command.subcommand,
        _ => unreachable!("the arguments name `search`"),
    }
}

/// `search query` reads the index that `search index` wrote.
fn indexed_repository() -> (tempfile::TempDir, PathBuf) {
    use clap::Parser;
    let (directory, path) = repository();
    let subcommand = match crate::cli::Cli::parse_from(["omen", "search", "index"]).command {
        crate::cli::Command::Search(command) => command.subcommand,
        _ => unreachable!("the arguments name `search`"),
    };
    run(
        &path,
        &Config::default(),
        subcommand,
        Format::Json,
        &mut Vec::new(),
    )
    .expect("index");
    (directory, path)
}

#[test]
fn test_query_writes_the_results_as_json() {
    let (_directory, path) = indexed_repository();
    let mut out = Vec::new();
    run(
        &path,
        &Config::default(),
        query("helper", None),
        Format::Json,
        &mut out,
    )
    .expect("search");
    let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(value["query"], "helper");
    assert_eq!(value["results"][0]["symbol_name"], "helper");
}

#[test]
fn test_top_limits_the_results() {
    let (_directory, path) = indexed_repository();
    let mut out = Vec::new();
    run(
        &path,
        &Config::default(),
        query("return", Some(1)),
        Format::Json,
        &mut out,
    )
    .expect("search");
    let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(value["results"].as_array().expect("results").len(), 1);
}

#[test]
fn test_markdown_writes_text() {
    let (_directory, path) = indexed_repository();
    let mut out = Vec::new();
    run(
        &path,
        &Config::default(),
        query("helper", None),
        Format::Markdown,
        &mut out,
    )
    .expect("search");
    let text = String::from_utf8(out).expect("utf-8");
    assert!(text.starts_with("Query: helper\n"), "{text}");
}
