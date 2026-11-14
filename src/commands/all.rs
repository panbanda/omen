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

    macro_rules! run_and_collect {
        ($ctx:expr, $analyzer:ty, $name:expr) => {{
            let a = <$analyzer>::default();
            match a.analyze($ctx) {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => json!({ "analyzer": $name, "result": v }),
                    Err(e) => json!({ "analyzer": $name, "error": format!("serialization failed: {e}") }),
                },
                Err(e) => {
                    json!({ "analyzer": $name, "error": e.to_string() })
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
                Err(e) => json!({ "analyzer": $name, "error": e.to_string() }),
            }
        }};
    }

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
                run_and_collect!(&ctx, crate::analyzers::duplicates::Analyzer, "clones"),
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

    let mut results: Vec<Value> = Vec::with_capacity(17);
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
        Format::Json | Format::JsonCompact | Format::Sarif => {
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
}
