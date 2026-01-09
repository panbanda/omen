//! Omen CLI - Multi-language code analysis for AI assistants.

use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use omen::cli::{
    Cli, Command, McpSubcommand, OutputFormat, ReportSubcommand, ScoreSubcommand, SearchSubcommand,
};
use omen::config::Config;
use omen::core::progress::is_tty;
use omen::core::{AnalysisContext, Analyzer, FileSet};
use omen::git::{clone_remote, is_remote_repo, CloneOptions};
use omen::mcp::McpServer;
use omen::output::Format;

fn main() -> ExitCode {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Resolve the repository path, cloning if it's a remote reference.
/// Returns (resolved_path, cleanup_path) where cleanup_path is Some if we cloned a temp repo.
fn resolve_repo_path(cli: &Cli) -> omen::core::Result<(PathBuf, Option<PathBuf>)> {
    let path_str = cli.path.to_string_lossy();

    if is_remote_repo(&path_str) {
        eprintln!("Cloning remote repository: {}", path_str);

        let options = CloneOptions {
            shallow: cli.shallow,
            reference: cli.git_ref.clone(),
            target: None, // Use default temp directory
        };

        let cloned_path = clone_remote(&path_str, options)?;
        eprintln!("Cloned to: {}", cloned_path.display());

        if cli.shallow {
            eprintln!(
                "Note: Using shallow clone. Git history-based analyzers (churn, ownership, hotspot, temporal) will have limited data."
            );
        }

        Ok((cloned_path.clone(), Some(cloned_path)))
    } else {
        Ok((cli.path.clone(), None))
    }
}

/// Clean up a cloned repository directory.
fn cleanup_repo(path: &PathBuf) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        eprintln!("Warning: Failed to clean up cloned repository: {}", e);
    }
}

fn run(cli: Cli) -> omen::core::Result<()> {
    // Resolve repository path (clone if remote)
    let (path, cleanup_path) = resolve_repo_path(&cli)?;

    // Use a closure to ensure cleanup happens even on error
    let result = run_with_path(&cli, &path);

    // Clean up cloned repository if we created one
    if let Some(ref cleanup) = cleanup_path {
        cleanup_repo(cleanup);
    }

    result
}

fn run_with_path(cli: &Cli, path: &PathBuf) -> omen::core::Result<()> {
    let config = match &cli.config {
        Some(config_path) => Config::from_file(config_path)?,
        None => Config::load_default(path)?,
    };

    let format = match cli.format {
        OutputFormat::Json => Format::Json,
        OutputFormat::Markdown => Format::Markdown,
        OutputFormat::Text => Format::Text,
    };

    match &cli.command {
        Command::Mcp(cmd) => {
            match cmd.subcommand {
                Some(McpSubcommand::Manifest) => {
                    // Output MCP server manifest (standalone use only, registry publishing disabled)
                    let manifest = serde_json::json!({
                        "$schema": "https://registry.modelcontextprotocol.io/schemas/server.json",
                        "name": "panbanda/omen",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Code analysis tools for AI assistants",
                        "tools": [
                            "analyze_complexity",
                            "analyze_satd",
                            "analyze_deadcode",
                            "analyze_churn",
                            "analyze_duplicates",
                            "analyze_defect",
                            "analyze_tdg",
                            "analyze_graph",
                            "analyze_hotspot",
                            "analyze_temporal_coupling",
                            "analyze_ownership",
                            "analyze_cohesion",
                            "analyze_repo_map"
                        ]
                    });
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                }
                None => {
                    let server = McpServer::new(path.clone(), config);
                    server.run_stdio()?;
                }
            }
        }
        Command::Complexity(_args) => {
            run_analyzer::<omen::analyzers::complexity::Analyzer>(path, &config, format)?;
        }
        Command::Satd(_args) => {
            run_analyzer::<omen::analyzers::satd::Analyzer>(path, &config, format)?;
        }
        Command::Deadcode(_args) => {
            run_analyzer::<omen::analyzers::deadcode::Analyzer>(path, &config, format)?;
        }
        Command::Churn(args) => {
            run_churn_analyzer(path, &config, format, args.days)?;
        }
        Command::Clones(_args) => {
            run_analyzer::<omen::analyzers::duplicates::Analyzer>(path, &config, format)?;
        }
        Command::Defect(_args) => {
            run_analyzer::<omen::analyzers::defect::Analyzer>(path, &config, format)?;
        }
        Command::Changes(_args) => {
            run_analyzer::<omen::analyzers::changes::Analyzer>(path, &config, format)?;
        }
        Command::Diff(_args) => {
            // Diff uses the changes analyzer - base/head filtering TBD
            run_analyzer::<omen::analyzers::changes::Analyzer>(path, &config, format)?;
        }
        Command::Tdg(_args) => {
            run_analyzer::<omen::analyzers::tdg::Analyzer>(path, &config, format)?;
        }
        Command::Graph(_args) => {
            run_analyzer::<omen::analyzers::graph::Analyzer>(path, &config, format)?;
        }
        Command::Hotspot(_args) => {
            run_analyzer::<omen::analyzers::hotspot::Analyzer>(path, &config, format)?;
        }
        Command::Temporal(_args) => {
            run_analyzer::<omen::analyzers::temporal::Analyzer>(path, &config, format)?;
        }
        Command::Ownership(_args) => {
            run_analyzer::<omen::analyzers::ownership::Analyzer>(path, &config, format)?;
        }
        Command::Cohesion(_args) => {
            run_analyzer::<omen::analyzers::cohesion::Analyzer>(path, &config, format)?;
        }
        Command::Repomap(_args) => {
            run_analyzer::<omen::analyzers::repomap::Analyzer>(path, &config, format)?;
        }
        Command::Smells(_args) => {
            run_analyzer::<omen::analyzers::smells::Analyzer>(path, &config, format)?;
        }
        Command::Flags(_args) => {
            run_analyzer::<omen::analyzers::flags::Analyzer>(path, &config, format)?;
        }
        Command::LintHotspot(_args) => {
            // Lint hotspot combines lint output with hotspot analysis
            // For now, run hotspot analyzer (lint integration TBD)
            run_analyzer::<omen::analyzers::hotspot::Analyzer>(path, &config, format)?;
        }
        Command::Score(cmd) => {
            match &cmd.subcommand {
                Some(ScoreSubcommand::Trend(args)) => {
                    // Score trend analysis
                    use serde_json::json;
                    let result = json!({
                        "since": args.since,
                        "period": format!("{:?}", args.period).to_lowercase(),
                        "snap": args.snap,
                        "message": "Trend analysis not yet implemented"
                    });
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                None => {
                    run_analyzer::<omen::score::Analyzer>(path, &config, format)?;
                }
            }
        }
        Command::All(_args) => {
            use serde_json::{json, Value};
            let file_set = FileSet::from_path(path, &config)?;
            let git_root = omen::git::GitRepo::open(path)
                .ok()
                .map(|r| r.root().to_path_buf());
            let mut ctx = AnalysisContext::new(&file_set, &config, Some(path));
            if let Some(ref git_path) = git_root {
                ctx = ctx.with_git_path(git_path);
            }

            let mut results: Vec<Value> = Vec::new();

            macro_rules! run_and_collect {
                ($analyzer:ty, $name:expr) => {{
                    let a = <$analyzer>::default();
                    match a.analyze(&ctx) {
                        Ok(result) => {
                            if let Ok(v) = serde_json::to_value(&result) {
                                results.push(json!({ "analyzer": $name, "result": v }));
                            }
                        }
                        Err(e) => {
                            results.push(json!({ "analyzer": $name, "error": e.to_string() }));
                        }
                    }
                }};
            }

            run_and_collect!(omen::analyzers::complexity::Analyzer, "complexity");
            run_and_collect!(omen::analyzers::satd::Analyzer, "satd");
            run_and_collect!(omen::analyzers::deadcode::Analyzer, "deadcode");
            run_and_collect!(omen::analyzers::churn::Analyzer, "churn");
            run_and_collect!(omen::analyzers::duplicates::Analyzer, "duplicates");
            run_and_collect!(omen::analyzers::defect::Analyzer, "defect");
            run_and_collect!(omen::analyzers::changes::Analyzer, "changes");
            run_and_collect!(omen::analyzers::tdg::Analyzer, "tdg");
            run_and_collect!(omen::analyzers::graph::Analyzer, "graph");
            run_and_collect!(omen::analyzers::hotspot::Analyzer, "hotspot");
            run_and_collect!(omen::analyzers::temporal::Analyzer, "temporal");
            run_and_collect!(omen::analyzers::ownership::Analyzer, "ownership");
            run_and_collect!(omen::analyzers::cohesion::Analyzer, "cohesion");
            run_and_collect!(omen::analyzers::repomap::Analyzer, "repomap");
            run_and_collect!(omen::analyzers::smells::Analyzer, "smells");
            run_and_collect!(omen::analyzers::flags::Analyzer, "flags");
            run_and_collect!(omen::score::Analyzer, "score");

            let combined = json!({ "analyzers": results });
            println!("{}", serde_json::to_string_pretty(&combined)?);
        }
        Command::Context(args) => {
            run_context(path, &config, args, format)?;
        }
        Command::Report(cmd) => {
            run_report(path, &config, &cmd.subcommand)?;
        }
        Command::Search(ref cmd) => {
            run_search(&cli.path, &config, cmd.subcommand.clone(), format)?;
        }
    }

    Ok(())
}

fn run_analyzer<A: Analyzer + Default>(
    path: &PathBuf,
    config: &Config,
    format: Format,
) -> omen::core::Result<()> {
    let file_set = FileSet::from_path(path, config)?;

    // Show analysis progress
    let spinner = if is_tty() {
        let s = ProgressBar::new_spinner();
        s.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("valid template"),
        );
        s.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(s)
    } else {
        None
    };

    let analyzer = A::default();
    if let Some(ref s) = spinner {
        s.set_message(format!("Analyzing {} files...", file_set.len()));
    }

    let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
    // Try to find git root for this path
    if let Ok(repo) = omen::git::GitRepo::open(path) {
        let git_root = repo.root().to_path_buf();
        ctx = ctx.with_git_path(Box::leak(Box::new(git_root)));
    }

    // Add progress callback for analyzers that support it
    let progress_counter = Arc::new(AtomicUsize::new(0));
    let total_files = file_set.len();
    let spinner_clone = spinner.clone();
    let counter_clone = progress_counter.clone();
    ctx = ctx.with_progress(move |current, _total| {
        counter_clone.store(current, Ordering::Relaxed);
        if let Some(ref s) = spinner_clone {
            s.set_message(format!("Analyzing... {}/{} files", current, total_files));
        }
    });

    let result = analyzer.analyze(&ctx)?;

    if let Some(s) = spinner {
        s.finish_and_clear();
    }

    format.format(&result, &mut stdout())?;
    Ok(())
}

fn run_churn_analyzer(
    path: &PathBuf,
    config: &Config,
    format: Format,
    days: u32,
) -> omen::core::Result<()> {
    let file_set = FileSet::from_path(path, config)?;
    let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
    if let Ok(repo) = omen::git::GitRepo::open(path) {
        let git_root = repo.root().to_path_buf();
        ctx = ctx.with_git_path(Box::leak(Box::new(git_root)));
    }
    let analyzer = omen::analyzers::churn::Analyzer::new().with_days(days);
    let result = analyzer.analyze(&ctx)?;
    format.format(&result, &mut stdout())?;
    Ok(())
}

fn run_context(
    path: &PathBuf,
    config: &Config,
    args: &omen::cli::ContextArgs,
    format: Format,
) -> omen::core::Result<()> {
    use serde_json::json;

    let file_set = FileSet::from_path(path, config)?;
    let mut ctx = AnalysisContext::new(&file_set, config, Some(path));

    if let Ok(repo) = omen::git::GitRepo::open(path) {
        let git_root = repo.root().to_path_buf();
        ctx = ctx.with_git_path(Box::leak(Box::new(git_root)));
    }

    // Generate context using repomap analyzer as base
    let analyzer = omen::analyzers::repomap::Analyzer::default();
    let repomap_result = analyzer.analyze(&ctx)?;

    // Build context output
    let context = json!({
        "target": args.target,
        "max_tokens": args.max_tokens,
        "depth": args.depth,
        "symbol": args.symbol,
        "repomap": repomap_result,
    });

    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(&context)?),
        Format::Markdown => {
            println!("# Repository Context\n");
            println!("**Max Tokens:** {}", args.max_tokens);
            println!("**Depth:** {}\n", args.depth);
            if let Some(ref target) = args.target {
                println!("**Target:** {}\n", target.display());
            }
            if let Some(ref symbol) = args.symbol {
                println!("**Symbol:** {}\n", symbol);
            }
            println!("## Symbol Map\n");
            println!("{}", serde_json::to_string_pretty(&repomap_result)?);
        }
        Format::Text => {
            println!("Repository Context");
            println!("==================");
            println!("Max Tokens: {}", args.max_tokens);
            println!("Depth: {}", args.depth);
            if let Some(ref target) = args.target {
                println!("Target: {}", target.display());
            }
            if let Some(ref symbol) = args.symbol {
                println!("Symbol: {}", symbol);
            }
            println!();
            format.format(&repomap_result, &mut stdout())?;
        }
    }

    Ok(())
}

fn run_report(
    path: &PathBuf,
    config: &Config,
    subcommand: &ReportSubcommand,
) -> omen::core::Result<()> {
    use serde_json::{json, Value};

    match subcommand {
        ReportSubcommand::Generate(args) => {
            // Create output directory
            std::fs::create_dir_all(&args.output)?;

            let file_set = FileSet::from_path(path, config)?;
            let git_root = omen::git::GitRepo::open(path)
                .ok()
                .map(|r| r.root().to_path_buf());
            let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
            if let Some(ref git_path) = git_root {
                ctx = ctx.with_git_path(git_path);
            }

            // Generate metadata.json (matches Go structure)
            let repo_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            // Use --days if provided, otherwise use --since
            let since_str = if let Some(days) = args.days {
                format!("{} days", days)
            } else {
                args.since.clone()
            };
            let metadata = json!({
                "repository": repo_name,
                "generated_at": chrono::Utc::now().to_rfc3339(),
                "since": since_str,
                "omen_version": env!("CARGO_PKG_VERSION"),
                "paths": [path.display().to_string()]
            });
            let metadata_path = args.output.join("metadata.json");
            std::fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;

            let skip_list: Vec<&str> = args
                .skip
                .as_deref()
                .map(|s| s.split(',').collect())
                .unwrap_or_default();

            // Count total analyzers to run
            let analyzer_names = [
                "complexity",
                "satd",
                "deadcode",
                "churn",
                "duplicates",
                "defect",
                "changes",
                "tdg",
                "graph",
                "hotspots",
                "temporal",
                "ownership",
                "cohesion",
                "repomap",
                "smells",
                "flags",
                "score",
            ];
            let total_analyzers = analyzer_names
                .iter()
                .filter(|n| !skip_list.contains(*n))
                .count();

            // Set up progress bar
            let progress = if is_tty() {
                let bar = ProgressBar::new(total_analyzers as u64);
                bar.set_style(
                    ProgressStyle::default_bar()
                        .template("{prefix:.bold} [{bar:30.green/white}] {pos}/{len} {msg}")
                        .expect("valid template")
                        .progress_chars("=>-"),
                );
                bar.set_prefix("Generating");
                Some(bar)
            } else {
                None
            };

            let mut completed = 0;

            macro_rules! run_and_save {
                ($analyzer:ty, $name:expr) => {{
                    if !skip_list.contains(&$name) {
                        if let Some(ref bar) = progress {
                            bar.set_message(format!("{}...", $name));
                        }
                        let a = <$analyzer>::default();
                        let result: Value = match a.analyze(&ctx) {
                            Ok(r) => serde_json::to_value(&r).unwrap_or(json!({"error": "serialization failed"})),
                            Err(e) => json!({"error": e.to_string()}),
                        };
                        let output_path = args.output.join(format!("{}.json", $name));
                        std::fs::write(&output_path, serde_json::to_string_pretty(&result)?)?;
                        completed += 1;
                        if let Some(ref bar) = progress {
                            bar.set_position(completed);
                        } else {
                            eprintln!("Generated: {}", output_path.display());
                        }
                    }
                }};
            }

            run_and_save!(omen::analyzers::complexity::Analyzer, "complexity");
            run_and_save!(omen::analyzers::satd::Analyzer, "satd");
            run_and_save!(omen::analyzers::deadcode::Analyzer, "deadcode");
            run_and_save!(omen::analyzers::churn::Analyzer, "churn");
            run_and_save!(omen::analyzers::duplicates::Analyzer, "duplicates");
            run_and_save!(omen::analyzers::defect::Analyzer, "defect");
            run_and_save!(omen::analyzers::changes::Analyzer, "changes");
            run_and_save!(omen::analyzers::tdg::Analyzer, "tdg");
            run_and_save!(omen::analyzers::graph::Analyzer, "graph");
            // Use "hotspots" to match Go naming
            if !skip_list.contains(&"hotspots") {
                if let Some(ref bar) = progress {
                    bar.set_message("hotspots...");
                }
                let a = omen::analyzers::hotspot::Analyzer::default();
                let result: Value = match a.analyze(&ctx) {
                    Ok(r) => {
                        serde_json::to_value(&r).unwrap_or(json!({"error": "serialization failed"}))
                    }
                    Err(e) => json!({"error": e.to_string()}),
                };
                let output_path = args.output.join("hotspots.json");
                std::fs::write(&output_path, serde_json::to_string_pretty(&result)?)?;
                completed += 1;
                if let Some(ref bar) = progress {
                    bar.set_position(completed);
                } else {
                    eprintln!("Generated: {}", output_path.display());
                }
            }
            run_and_save!(omen::analyzers::temporal::Analyzer, "temporal");
            run_and_save!(omen::analyzers::ownership::Analyzer, "ownership");
            run_and_save!(omen::analyzers::cohesion::Analyzer, "cohesion");
            run_and_save!(omen::analyzers::repomap::Analyzer, "repomap");
            run_and_save!(omen::analyzers::smells::Analyzer, "smells");
            run_and_save!(omen::analyzers::flags::Analyzer, "flags");
            run_and_save!(omen::score::Analyzer, "score");

            if let Some(bar) = progress {
                bar.finish_with_message("done");
            }
            eprintln!("Report data generated in: {}", args.output.display());
        }
        ReportSubcommand::Validate(args) => {
            // Basic validation: check that expected JSON files exist and are valid JSON
            // File list matches Go version
            let expected_files = [
                "metadata",
                "complexity",
                "satd",
                "deadcode",
                "churn",
                "duplicates",
                "defect",
                "changes",
                "tdg",
                "graph",
                "hotspots", // Go uses plural
                "temporal",
                "ownership",
                "cohesion",
                "repomap",
                "smells",
                "flags",
                "score",
            ];

            let mut errors = Vec::new();
            let mut valid_count = 0;

            for name in expected_files {
                let file_path = args.data.join(format!("{}.json", name));
                if file_path.exists() {
                    match std::fs::read_to_string(&file_path) {
                        Ok(contents) => match serde_json::from_str::<Value>(&contents) {
                            Ok(_) => {
                                valid_count += 1;
                                eprintln!("Valid: {}.json", name);
                            }
                            Err(e) => errors.push(format!("{}.json: invalid JSON - {}", name, e)),
                        },
                        Err(e) => errors.push(format!("{}.json: read error - {}", name, e)),
                    }
                } else {
                    errors.push(format!("{}.json: missing", name));
                }
            }

            if errors.is_empty() {
                eprintln!("All {} data files are valid.", valid_count);
            } else {
                eprintln!("\nValidation errors:");
                for error in &errors {
                    eprintln!("  - {}", error);
                }
                return Err(omen::core::Error::config(format!(
                    "{} validation errors found",
                    errors.len()
                )));
            }
        }
        ReportSubcommand::Render(args) => {
            // Use the HTML report renderer (matches Go 3.x version)
            use omen::report::Renderer;

            let renderer = Renderer::new()?;
            renderer.render_to_file(&args.data, &args.output)?;
            eprintln!("Report rendered to: {}", args.output.display());
        }
        ReportSubcommand::Serve(args) => {
            eprintln!("Starting server at http://{}:{}/", args.host, args.port);
            eprintln!("Serving data from: {}", args.data.display());
            eprintln!("Press Ctrl+C to stop.");

            // Simple HTTP server using std::net
            use std::io::{Read, Write};
            use std::net::TcpListener;

            let addr = format!("{}:{}", args.host, args.port);
            let listener = TcpListener::bind(&addr)?;

            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                if stream.read(&mut buffer).is_ok() {
                    let request = String::from_utf8_lossy(&buffer);

                    let response = if request.starts_with("GET / ")
                        || request.starts_with("GET /index.html ")
                    {
                        // Serve rendered report
                        let report_path = args.data.parent().unwrap_or(path).join("report.html");
                        if report_path.exists() {
                            match std::fs::read_to_string(&report_path) {
                                Ok(html) => format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                                    html.len(),
                                    html
                                ),
                                Err(_) => "HTTP/1.1 500 Internal Server Error\r\n\r\nFailed to read report".to_string(),
                            }
                        } else {
                            "HTTP/1.1 404 Not Found\r\n\r\nReport not found. Run 'omen report render' first.".to_string()
                        }
                    } else {
                        "HTTP/1.1 404 Not Found\r\n\r\nNot Found".to_string()
                    };

                    let _ = stream.write_all(response.as_bytes());
                }
            }
        }
    }

    Ok(())
}

fn run_search(
    path: &PathBuf,
    config: &Config,
    subcommand: SearchSubcommand,
    format: Format,
) -> omen::core::Result<()> {
    use omen::semantic::{SearchConfig, SemanticSearch};

    let search_config = SearchConfig::default();
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

            let output = if let Some(files) = file_filter {
                search.search_in_files(&args.query, &files, Some(args.top_k))?
            } else {
                search.search(&args.query, Some(args.top_k))?
            };

            // Filter by min_score
            let filtered_results: Vec<_> = output
                .results
                .into_iter()
                .filter(|r| r.score >= args.min_score)
                .collect();

            let output = omen::semantic::SearchOutput::new(
                output.query,
                output.total_symbols,
                filtered_results,
            );

            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&output)?),
                Format::Markdown | Format::Text => {
                    println!("Query: {}", output.query);
                    println!("Total symbols indexed: {}", output.total_symbols);
                    println!("Results: {}\n", output.results.len());

                    for (i, result) in output.results.iter().enumerate() {
                        println!(
                            "{}. {} ({}) - score: {:.3}",
                            i + 1,
                            result.symbol_name,
                            result.symbol_type,
                            result.score
                        );
                        println!(
                            "   {}:{}-{}",
                            result.file_path, result.start_line, result.end_line
                        );
                        println!("   {}", result.signature);
                        println!();
                    }
                }
            }
        }
    }

    Ok(())
}
