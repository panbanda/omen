//! The `all` command. It runs each analyzer and writes one document.

use std::io::Write;
use std::path::PathBuf;

use super::scope::{scoped_file_set, Scope};
use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, Result};
use crate::output::{format_with_limits, Format};

pub fn run(
    path: &PathBuf,
    config: &Config,
    scope: &Scope,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    run_with_source(path, config, scope, format, out, None)
}

fn run_with_source(
    path: &PathBuf,
    config: &Config,
    scope: &Scope,
    format: Format,
    out: &mut dyn Write,
    source: Option<std::sync::Arc<dyn crate::core::ContentSource>>,
) -> Result<()> {
    let mut out = out;
    use serde_json::{json, Value};

    fn collected_result<T: serde::de::DeserializeOwned>(
        results: &[Value],
        name: &str,
    ) -> Option<T> {
        let value = results
            .iter()
            .find(|entry| entry.get("analyzer").and_then(Value::as_str) == Some(name))?
            .get("result")?
            .clone();
        match serde_json::from_value(value) {
            Ok(result) => Some(result),
            Err(error) => {
                tracing::warn!(analyzer = name, %error, "Failed to parse collected analyzer result");
                None
            }
        }
    }

    let file_set = scoped_file_set(path, config, scope)?;
    let git_root = crate::git::GitRepo::open(path)
        .ok()
        .map(|r| r.root().to_path_buf());
    let ctx = AnalysisContext::new(&file_set, config, Some(path));
    let ctx = if let Some(ref git_path) = git_root {
        ctx.with_git_path(git_path)
    } else {
        ctx
    };
    let ctx = match source {
        Some(source) => ctx.with_content_source(source),
        None => ctx,
    };

    let changed_file: parking_lot::Mutex<Option<crate::core::Error>> =
        parking_lot::Mutex::new(None);

    fn keep_if_changed(
        slot: &parking_lot::Mutex<Option<crate::core::Error>>,
        error: crate::core::Error,
    ) {
        if matches!(error, crate::core::Error::ChangedDuringAnalysis { .. }) {
            slot.lock().get_or_insert(error);
        }
    }

    macro_rules! run_and_collect {
        ($ctx:expr, $analyzer:ty, $name:expr) => {{
            let a = <$analyzer>::default();
            match a.analyze($ctx) {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => json!({ "analyzer": $name, "result": v }),
                    Err(e) => json!({ "analyzer": $name, "error": format!("serialization failed: {e}") }),
                },
                Err(e) => {
                    let entry = json!({ "analyzer": $name, "error": e.to_string() });
                    keep_if_changed(&changed_file, e);
                    entry
                }
            }
        }};
    }

    macro_rules! run_instance_and_collect {
        ($ctx:expr, $analyzer:expr, $name:expr) => {{
            let a = $analyzer;
            match a.analyze($ctx) {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => json!({ "analyzer": $name, "result": v }),
                    Err(e) => json!({ "analyzer": $name, "error": format!("serialization failed: {e}") }),
                },
                Err(e) => {
                    let entry = json!({ "analyzer": $name, "error": e.to_string() });
                    keep_if_changed(&changed_file, e);
                    entry
                }
            }
        }};
    }

    let security_analyzer = crate::analyzers::security::Analyzer::from_config(config)?;
    let clones_analyzer = crate::analyzers::duplicates::Analyzer::from_config(config);

    // Run analyzers in parallel using std::thread::scope.
    //
    // Group A: file-based analyzers (no git dependency)
    // Group B: git-based analyzers
    // These two groups run concurrently. After both complete,
    // Group C (analyzers that internally depend on git + file data)
    // and score run sequentially.
    let (group_a, group_b) = std::thread::scope(|s| {
        let handle_a = s.spawn(|| -> Vec<Value> {
            vec![
                run_and_collect!(&ctx, crate::analyzers::complexity::Analyzer, "complexity"),
                run_and_collect!(&ctx, crate::analyzers::satd::Analyzer, "satd"),
                run_and_collect!(&ctx, crate::analyzers::stubs::Analyzer, "stubs"),
                run_and_collect!(&ctx, crate::analyzers::deadcode::Analyzer, "deadcode"),
                run_and_collect!(&ctx, crate::analyzers::cohesion::Analyzer, "cohesion"),
                run_and_collect!(&ctx, crate::analyzers::graph::Analyzer, "graph"),
                run_and_collect!(&ctx, crate::analyzers::repomap::Analyzer, "repomap"),
                run_and_collect!(&ctx, crate::analyzers::smells::Analyzer, "smells"),
                run_and_collect!(&ctx, crate::analyzers::flags::Analyzer, "flags"),
                run_instance_and_collect!(&ctx, security_analyzer, "security"),
                run_instance_and_collect!(&ctx, clones_analyzer, "clones"),
            ]
        });

        let handle_b = s.spawn(|| -> Vec<Value> {
            vec![
                run_and_collect!(&ctx, crate::analyzers::churn::Analyzer, "churn"),
                run_and_collect!(&ctx, crate::analyzers::temporal::Analyzer, "temporal"),
                run_and_collect!(&ctx, crate::analyzers::ownership::Analyzer, "ownership"),
            ]
        });

        (
            handle_a.join().unwrap_or_default(),
            handle_b.join().unwrap_or_default(),
        )
    });

    let mut results: Vec<Value> = Vec::with_capacity(18);
    results.extend(group_a);
    results.extend(group_b);

    // Group C: analyzers that internally depend on both file and git data.
    // Run after groups A and B to benefit from warm OS page cache.
    let mut hotspot_analyzer = crate::analyzers::hotspot::Analyzer::from_config(config);
    if let Some(complexity) = collected_result(&results, "complexity") {
        hotspot_analyzer = hotspot_analyzer.with_precomputed_complexity(complexity);
    }
    results.push(run_instance_and_collect!(&ctx, hotspot_analyzer, "hotspot"));
    let mut tdg_analyzer = crate::analyzers::tdg::Analyzer::default();
    if let Some(hotspot) = collected_result(&results, "hotspot") {
        tdg_analyzer = tdg_analyzer.with_precomputed_hotspot(hotspot);
    }
    if let Some(temporal) = collected_result(&results, "temporal") {
        tdg_analyzer = tdg_analyzer.with_precomputed_temporal(temporal);
    }
    results.push(run_instance_and_collect!(&ctx, tdg_analyzer, "tdg"));
    let mut defect_analyzer = crate::analyzers::defect::Analyzer::from_config(config);
    if let Some(complexity) = collected_result(&results, "complexity") {
        defect_analyzer = defect_analyzer.with_precomputed_complexity(complexity);
    }
    if let Some(duplicates) = collected_result(&results, "clones") {
        defect_analyzer = defect_analyzer.with_precomputed_duplicates(duplicates);
    }
    if let Some(graph) = collected_result(&results, "graph") {
        defect_analyzer = defect_analyzer.with_precomputed_graph(graph);
    }
    results.push(run_instance_and_collect!(&ctx, defect_analyzer, "defect"));
    results.push(run_and_collect!(
        &ctx,
        crate::analyzers::changes::Analyzer,
        "changes"
    ));

    let complexity = collected_result(&results, "complexity");
    let satd = collected_result(&results, "satd");
    let stubs = collected_result(&results, "stubs");
    let duplicates = collected_result(&results, "clones");
    let cohesion = collected_result(&results, "cohesion");
    let tdg = collected_result(&results, "tdg");
    let graph = collected_result(&results, "graph");
    let smells = collected_result(&results, "smells");
    let components = crate::score::Components {
        complexity: complexity.as_ref(),
        satd: satd.as_ref(),
        stubs: stubs.as_ref(),
        duplicates: duplicates.as_ref(),
        cohesion: cohesion.as_ref(),
        tdg: tdg.as_ref(),
        graph: graph.as_ref(),
        smells: smells.as_ref(),
    };
    let score = match crate::score::compute_from_components(&components, ctx.files.files().len()) {
        Ok(result) => match serde_json::to_value(&result) {
            Ok(value) => json!({ "analyzer": "score", "result": value }),
            Err(error) => {
                json!({ "analyzer": "score", "error": format!("serialization failed: {error}") })
            }
        },
        Err(error) => json!({ "analyzer": "score", "error": error.to_string() }),
    };
    results.push(score);

    if let Some(error) = changed_file.into_inner() {
        return Err(error);
    }

    // Each section has `is_test` in its rows, and `--exclude-tests` removes
    // the test rows, as the single command does. This step is after each
    // analyzer, because an analyzer reads the full result of a different one.
    for entry in &mut results {
        let name = entry
            .get("analyzer")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(result) = entry.get_mut("result") else {
            continue;
        };
        if name == "security" {
            if scope.exclude_tests {
                let report: Option<crate::analyzers::security::Report> =
                    serde_json::from_value(result.clone()).ok();
                if let Some(report) = report {
                    *result = serde_json::to_value(report.without_tests())?;
                }
            }
            continue;
        }
        super::test_rows::mark(result, &name, &ctx, scope.exclude_tests);
    }

    // `--top` and `--offset` apply to each section as they apply to the
    // single command. SARIF keeps each finding.
    let top = scope.top.unwrap_or(0);
    let offset = scope.offset.unwrap_or(0);
    if !matches!(format, Format::Sarif) {
        for entry in &mut results {
            if let Some(result) = entry.get_mut("result") {
                crate::output::truncate_lists(result, top, offset);
            }
        }
    }

    let combined = match format {
        Format::Json | Format::JsonCompact | Format::Sarif | Format::Mermaid => {
            json!({ "analyzers": results })
        }
        // One object with a key for each analyzer gives one section for
        // each analyzer.
        Format::Markdown | Format::Text => Value::Object(
            results
                .into_iter()
                .filter_map(|mut entry| {
                    let name = entry.get("analyzer")?.as_str()?.to_string();
                    let section = match entry.get_mut("result") {
                        Some(result) => result.take(),
                        None => json!({ "error": entry.get("error") }),
                    };
                    Some((name, section))
                })
                .collect(),
        ),
    };
    format_with_limits(combined, format, None, None, &mut out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    #[test]
    fn test_each_analyzer_has_a_section_in_the_report() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(
            &path,
            &Config::default(),
            &Scope::default(),
            Format::Json,
            &mut out,
        )
        .expect("all");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        let names: Vec<&str> = value["analyzers"]
            .as_array()
            .expect("analyzers")
            .iter()
            .map(|entry| entry["analyzer"].as_str().expect("name"))
            .collect();
        for expected in ["complexity", "satd", "stubs", "score", "tdg", "hotspot"] {
            assert!(names.contains(&expected), "{expected}: {names:?}");
        }
    }

    #[test]
    fn test_a_file_that_changed_during_analysis_fails_the_command() {
        let (_directory, path) = repository();
        let source = std::sync::Arc::new(crate::commands::testing::ChangingSource::new(&path));
        let mut out = Vec::new();
        let result = run_with_source(
            &path,
            &Config::default(),
            &Scope::default(),
            Format::Json,
            &mut out,
            Some(source),
        );
        let error = result.expect_err("a changed file fails the command");
        assert!(
            error.to_string().contains("changed or became unreadable"),
            "{error}"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn test_all_has_the_security_analysis_with_the_configuration() {
        let directory = tempfile::tempdir().expect("directory");
        std::fs::write(
            directory.path().join("app.py"),
            "import os\n\ntoken = \"AKIAJ3KD92LQ7MZP5XWB\"\n\n\ndef run(command):\n    os.system(command)\n",
        )
        .expect("write");
        let path = directory.path().to_path_buf();
        let security_of = |config: &Config| {
            let mut out = Vec::new();
            run(&path, config, &Scope::default(), Format::Json, &mut out).expect("all");
            let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
            value["analyzers"]
                .as_array()
                .expect("analyzers")
                .iter()
                .find(|entry| entry["analyzer"] == "security")
                .expect("a security entry")["result"]["summary"]["sinks"]
                .as_u64()
                .expect("sinks")
        };
        assert_eq!(security_of(&Config::default()), 1);
        let mut experimental = Config::default();
        experimental.security.experimental = true;
        assert_eq!(security_of(&experimental), 2);
    }

    /// The analyzer entries of `all` on the sources of `language` at `depth`, in a new
    /// repository, on a thread with the default stack of 2 MiB.
    fn analyzer_entries_at(
        language: crate::core::Language,
        depth: usize,
    ) -> Vec<serde_json::Value> {
        use crate::parser::deep;
        let directory = tempfile::tempdir().expect("directory");
        for source in deep::deep_sources_at(language, depth) {
            if !source.slow {
                std::fs::write(directory.path().join(&source.file), &source.source).expect("write");
            }
        }
        for arguments in [
            &["init", "-q"][..],
            &["add", "-A"],
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "commit",
                "-q",
                "-m",
                "deep",
            ],
        ] {
            let status = std::process::Command::new("git")
                .args(arguments)
                .current_dir(directory.path())
                .status()
                .expect("git");
            assert!(status.success(), "git {arguments:?}");
        }
        let path = directory.path().to_path_buf();
        let out = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut out = Vec::new();
                run(
                    &path,
                    &Config::default(),
                    &Scope::default(),
                    Format::Json,
                    &mut out,
                )
                .expect("all");
                out
            })
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        value["analyzers"].as_array().expect("analyzers").clone()
    }

    fn analyzer_names(entries: &[serde_json::Value]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry["analyzer"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn whole_analysis_of_shallow_sources_gives_a_result_for_each_analyzer() {
        let entries = analyzer_entries_at(crate::core::Language::Go, 1);
        assert!(
            entries.iter().all(|entry| entry.get("result").is_some()),
            "{entries:?}"
        );
        assert!(analyzer_names(&entries).contains(&"security".to_string()));
    }

    /// Runs `all` on the sources of `language` at `LARGE_DEPTH`. Each analyzer of a run on
    /// shallow sources gives a result, and no analyzer gives an error. A group of analyzers whose
    /// thread panics gives no entries, thus the names of the two runs must be the same.
    fn whole_analysis_of_deep_sources(language: crate::core::Language) {
        let shallow = analyzer_names(&analyzer_entries_at(language, 1));
        let entries = analyzer_entries_at(language, crate::parser::deep::LARGE_DEPTH);
        assert_eq!(analyzer_names(&entries), shallow);
        let failed: Vec<&serde_json::Value> = entries
            .iter()
            .filter(|entry| entry.get("result").is_none())
            .collect();
        assert!(failed.is_empty(), "{failed:?}");
    }

    macro_rules! whole_analysis_of_deep_sources {
        ($($name:ident: $language:ident,)*) => {$(
            #[test]
            #[ignore = "deep: run in release with --ignored deep_whole_analysis_"]
            fn $name() {
                whole_analysis_of_deep_sources(crate::core::Language::$language);
            }
        )*};
    }

    whole_analysis_of_deep_sources! {
        deep_whole_analysis_go: Go,
        deep_whole_analysis_rust: Rust,
        deep_whole_analysis_python: Python,
        deep_whole_analysis_typescript: TypeScript,
        deep_whole_analysis_javascript: JavaScript,
        deep_whole_analysis_tsx: Tsx,
        deep_whole_analysis_jsx: Jsx,
        deep_whole_analysis_java: Java,
        deep_whole_analysis_c: C,
        deep_whole_analysis_cpp: Cpp,
        deep_whole_analysis_csharp: CSharp,
        deep_whole_analysis_ruby: Ruby,
        deep_whole_analysis_php: Php,
        deep_whole_analysis_bash: Bash,
    }
}
