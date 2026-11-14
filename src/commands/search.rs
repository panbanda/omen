//! The `search` command.

use std::io::Write;
use std::path::PathBuf;

use crate::cli::SearchSubcommand;
use crate::config::Config;
use crate::output::Format;

/// The number of results that `search query` gives if `--top` is absent.
const DEFAULT_SEARCH_RESULTS: usize = 10;

pub fn run(
    path: &PathBuf,
    config: &Config,
    subcommand: SearchSubcommand,
    format: Format,
    out: &mut dyn Write,
) -> crate::core::Result<()> {
    use crate::semantic::{SearchConfig, SemanticSearch};

    let mut out = out;

    // The search applies the threshold of its configuration before the
    // `--min-score` filter. Thus give it the same value, or a `--min-score`
    // below the default has no effect.
    let search_config = match &subcommand {
        SearchSubcommand::Query(args) => SearchConfig {
            min_score: args.min_score,
            ..SearchConfig::default()
        },
        SearchSubcommand::Index(_) => SearchConfig::default(),
    };
    let search = SemanticSearch::new(&search_config, path)?;

    match subcommand {
        SearchSubcommand::Index(args) => {
            if args.force {
                // Remove existing cache
                let cache_path = path.join(".omen").join("search.db");
                if cache_path.exists() {
                    std::fs::remove_file(&cache_path)?;
                    eprintln!("Removed existing index: {}", cache_path.display());
                }
                // Recreate search instance with fresh cache
                let search = SemanticSearch::new(&search_config, path)?;
                let stats = search.index(config)?;
                eprintln!(
                    "Indexed {} files ({} symbols), {} errors",
                    stats.indexed, stats.symbols, stats.errors
                );
            } else {
                let stats = search.index(config)?;
                eprintln!(
                    "Indexed {} files ({} symbols), {} removed, {} errors",
                    stats.indexed, stats.symbols, stats.removed, stats.errors
                );
            }
        }
        SearchSubcommand::Query(args) => {
            let file_filter: Option<Vec<&str>> =
                args.files.as_ref().map(|f| f.split(',').collect());
            // SARIF keeps each finding, so `--top` and `--offset` do not apply to it.
            let (offset, limit) = if matches!(format, Format::Sarif) {
                (0, usize::MAX)
            } else {
                let limit = match args.pagination.top {
                    Some(0) => usize::MAX,
                    Some(top) => top,
                    None => DEFAULT_SEARCH_RESULTS,
                };
                (args.pagination.offset.unwrap_or(0), limit)
            };
            // The search must return the skipped results too.
            let fetch = limit.saturating_add(offset);

            let output = if let Some(ref include) = args.include_project {
                // Multi-repo search: combine current project with additional projects
                let extra_paths: Vec<std::path::PathBuf> = include
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| std::path::PathBuf::from(s.trim()))
                    .collect();

                let mut all_projects: Vec<&std::path::Path> = vec![path.as_path()];
                all_projects.extend(extra_paths.iter().map(|p| p.as_path()));

                let mr = crate::semantic::multi_repo::multi_repo_search(
                    &all_projects,
                    &args.query,
                    fetch,
                    args.min_score,
                )?;
                crate::semantic::SearchOutput::new(args.query.clone(), mr.total_symbols, mr.results)
            } else if let Some(files) = file_filter {
                search.search_in_files(&args.query, &files, Some(fetch))?
            } else {
                search.search(&args.query, Some(fetch))?
            };

            // Filter by min_score
            let filtered_results: Vec<_> = output
                .results
                .into_iter()
                .filter(|r| r.score >= args.min_score)
                .skip(offset)
                .take(limit)
                .collect();

            let output = crate::semantic::SearchOutput::new(
                output.query,
                output.total_symbols,
                filtered_results,
            );

            match format {
                Format::Json | Format::JsonCompact => format.format(&output, &mut out)?,
                Format::Markdown | Format::Text => {
                    say!(out, "Query: {}", output.query);
                    say!(out, "Total symbols indexed: {}", output.total_symbols);
                    say!(out, "Results: {}\n", output.results.len());

                    for (i, result) in output.results.iter().enumerate() {
                        say!(
                            out,
                            "{}. {} ({}) - score: {:.3}",
                            i + 1,
                            result.symbol_name,
                            result.symbol_type,
                            result.score
                        );
                        say!(
                            out,
                            "   {}:{}-{}",
                            result.file,
                            result.start_line,
                            result.end_line
                        );
                        say!(out, "   {}", result.signature);
                        say!(out);
                    }
                }
                Format::Sarif => format.format(&output, &mut out)?,
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
