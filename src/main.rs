//! Omen CLI - Multi-language code analysis for AI assistants.

use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::ThreadPoolBuilder;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use omen::cli::{
    Cli, Command, ComplexityArgs, McpSubcommand, MutationArgs, MutationSubcommand,
    MutationTrainArgs, OutputFormat, ReportSubcommand, ScoreArgs, ScoreSubcommand,
    SearchSubcommand,
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
    // Configure rayon thread pool if -j/--jobs flag is specified
    if let Some(jobs) = cli.jobs {
        ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .ok(); // Ignore if already initialized
    }

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
                    // Output MCP server manifest (standalone use only, registry publishing disabled) omen:ignore
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
        Command::Complexity(args) => {
            if args.check {
                run_complexity_check(path, &config, args)?;
            } else {
                run_analyzer::<omen::analyzers::complexity::Analyzer>(path, &config, format)?;
            }
        }
        Command::Satd(_)
        | Command::Deadcode(_)
        | Command::Clones(_)
        | Command::Defect(_)
        | Command::Changes(_)
        | Command::Diff(_)
        | Command::Tdg(_)
        | Command::Graph(_)
        | Command::Hotspot(_)
        | Command::Temporal(_)
        | Command::Ownership(_)
        | Command::Cohesion(_)
        | Command::Repomap(_)
        | Command::Smells(_)
        | Command::LintHotspot(_) => {
            dispatch_analyzer(&cli.command, path, &config, format)?;
        }
        Command::Churn(args) => {
            run_churn_analyzer(path, &config, format, args.days)?;
        }
        Command::Flags(args) => {
            // Merge CLI --provider option into config
            let mut config = config.clone();
            if let Some(ref provider) = args.provider {
                config.feature_flags.providers = vec![provider.clone()];
            }
            if args.stale_days > 0 {
                config.feature_flags.stale_days = args.stale_days;
            }
            run_analyzer::<omen::analyzers::flags::Analyzer>(path, &config, format)?;
        }
        Command::Score(cmd) => {
            if cmd.args.check {
                run_score_check(path, &config, &cmd.args)?;
            } else {
                match &cmd.subcommand {
                    Some(ScoreSubcommand::Trend(args)) => {
                        // Score trend analysis
                        let trend_data =
                            omen::score::analyze_trend(path, &config, &args.since, args.period)?;
                        match format {
                            Format::Json => {
                                println!("{}", serde_json::to_string_pretty(&trend_data)?);
                            }
                            Format::Markdown => {
                                println!("# Score Trend Analysis\n");
                                println!(
                                    "**Period**: {} ({})",
                                    args.since,
                                    format!("{:?}", args.period).to_lowercase()
                                );
                                println!("**Data Points**: {}\n", trend_data.points.len());

                                if !trend_data.points.is_empty() {
                                    println!("## Overall Trend\n");
                                    println!("- **Start Score**: {}", trend_data.start_score);
                                    println!("- **End Score**: {}", trend_data.end_score);
                                    println!(
                                        "- **Change**: {:+}",
                                        trend_data.end_score - trend_data.start_score
                                    );
                                    println!("- **Slope**: {:.2} points/period", trend_data.slope);
                                    println!("- **R-squared**: {:.3}\n", trend_data.r_squared);

                                    if !trend_data.component_trends.is_empty() {
                                        println!("## Component Trends\n");
                                        println!("| Component | Slope | Correlation |");
                                        println!("|-----------|-------|-------------|");
                                        for (name, stats) in &trend_data.component_trends {
                                            println!(
                                                "| {} | {:.2} | {:.3} |",
                                                name, stats.slope, stats.correlation
                                            );
                                        }
                                        println!();
                                    }

                                    println!("## History\n");
                                    println!("| Date | Score |");
                                    println!("|------|-------|");
                                    for point in &trend_data.points {
                                        println!("| {} | {} |", point.date, point.score);
                                    }
                                } else {
                                    println!(
                                        "No historical data available for the specified period."
                                    );
                                }
                            }
                            Format::Text => {
                                println!(
                                    "Score Trend: {} - {}",
                                    trend_data.start_score, trend_data.end_score
                                );
                                println!(
                                    "Change: {:+}",
                                    trend_data.end_score - trend_data.start_score
                                );
                                println!("Slope: {:.2}", trend_data.slope);
                            }
                        }
                    }
                    None => {
                        run_analyzer::<omen::score::Analyzer>(path, &config, format)?;
                    }
                }
            }
        }
        Command::All(_) => {
            use serde_json::{json, Value};
            let file_set = FileSet::from_path(path, &config)?;
            let git_root = omen::git::GitRepo::open(path)
                .ok()
                .map(|r| r.root().to_path_buf());
            let ctx = AnalysisContext::new(&file_set, &config, Some(path));
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
                        run_and_collect!(&ctx, omen::analyzers::complexity::Analyzer, "complexity"),
                        run_and_collect!(&ctx, omen::analyzers::satd::Analyzer, "satd"),
                        run_and_collect!(&ctx, omen::analyzers::deadcode::Analyzer, "deadcode"),
                        run_and_collect!(&ctx, omen::analyzers::cohesion::Analyzer, "cohesion"),
                        run_and_collect!(&ctx, omen::analyzers::graph::Analyzer, "graph"),
                        run_and_collect!(&ctx, omen::analyzers::repomap::Analyzer, "repomap"),
                        run_and_collect!(&ctx, omen::analyzers::smells::Analyzer, "smells"),
                        run_and_collect!(&ctx, omen::analyzers::flags::Analyzer, "flags"),
                        run_and_collect!(&ctx, omen::analyzers::duplicates::Analyzer, "duplicates"),
                    ]
                });

                let handle_b = s.spawn(|| -> Vec<Value> {
                    vec![
                        run_and_collect!(&ctx, omen::analyzers::churn::Analyzer, "churn"),
                        run_and_collect!(&ctx, omen::analyzers::temporal::Analyzer, "temporal"),
                        run_and_collect!(&ctx, omen::analyzers::ownership::Analyzer, "ownership"),
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
            results.push(run_and_collect!(
                &ctx,
                omen::analyzers::hotspot::Analyzer,
                "hotspot"
            ));
            results.push(run_and_collect!(
                &ctx,
                omen::analyzers::tdg::Analyzer,
                "tdg"
            ));
            results.push(run_and_collect!(
                &ctx,
                omen::analyzers::defect::Analyzer,
                "defect"
            ));
            results.push(run_and_collect!(
                &ctx,
                omen::analyzers::changes::Analyzer,
                "changes"
            ));
            results.push(run_and_collect!(&ctx, omen::score::Analyzer, "score"));

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
        Command::Mutation(ref cmd) => match &cmd.subcommand {
            Some(MutationSubcommand::Train(args)) => {
                run_mutation_train(&args.path, args)?;
            }
            None => {
                run_mutation(path, &config, &cmd.args, format)?;
            }
        },
    }

    Ok(())
}

/// Build a `FileSet` and `AnalysisContext` for the given path, including git
/// root discovery. This eliminates the repeated file-set + context + git-root
/// boilerplate that appears in every command handler.
fn build_context<'a>(
    path: &'a PathBuf,
    file_set: &'a FileSet,
    config: &'a Config,
) -> AnalysisContext<'a> {
    let mut ctx = AnalysisContext::new(file_set, config, Some(path));
    if let Ok(repo) = omen::git::GitRepo::open(path) {
        let git_root = repo.root().to_path_buf();
        ctx = ctx.with_git_path(Box::leak(Box::new(git_root)));
    }
    ctx
}

/// Dispatch a command variant to its corresponding analyzer. This consolidates
/// the 15 command arms that all follow the same `run_analyzer::<T>` pattern.
fn dispatch_analyzer(
    command: &Command,
    path: &PathBuf,
    config: &Config,
    format: Format,
) -> omen::core::Result<()> {
    match command {
        Command::Satd(_) => run_analyzer::<omen::analyzers::satd::Analyzer>(path, config, format),
        Command::Deadcode(_) => {
            run_analyzer::<omen::analyzers::deadcode::Analyzer>(path, config, format)
        }
        Command::Clones(_) => {
            run_analyzer::<omen::analyzers::duplicates::Analyzer>(path, config, format)
        }
        Command::Defect(_) => {
            run_analyzer::<omen::analyzers::defect::Analyzer>(path, config, format)
        }
        Command::Changes(_) | Command::Diff(_) => {
            run_analyzer::<omen::analyzers::changes::Analyzer>(path, config, format)
        }
        Command::Tdg(_) => run_analyzer::<omen::analyzers::tdg::Analyzer>(path, config, format),
        Command::Graph(_) => run_analyzer::<omen::analyzers::graph::Analyzer>(path, config, format),
        Command::Hotspot(_) | Command::LintHotspot(_) => {
            run_analyzer::<omen::analyzers::hotspot::Analyzer>(path, config, format)
        }
        Command::Temporal(_) => {
            run_analyzer::<omen::analyzers::temporal::Analyzer>(path, config, format)
        }
        Command::Ownership(_) => {
            run_analyzer::<omen::analyzers::ownership::Analyzer>(path, config, format)
        }
        Command::Cohesion(_) => {
            run_analyzer::<omen::analyzers::cohesion::Analyzer>(path, config, format)
        }
        Command::Repomap(_) => {
            run_analyzer::<omen::analyzers::repomap::Analyzer>(path, config, format)
        }
        Command::Smells(_) => {
            run_analyzer::<omen::analyzers::smells::Analyzer>(path, config, format)
        }
        _ => unreachable!("dispatch_analyzer called with non-dispatched command"),
    }
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

    let mut ctx = build_context(path, &file_set, config);

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

fn run_complexity_check(
    path: &PathBuf,
    config: &Config,
    args: &ComplexityArgs,
) -> omen::core::Result<()> {
    let file_set = FileSet::from_path(path, config)?;
    let ctx = build_context(path, &file_set, config);

    let analyzer = omen::analyzers::complexity::Analyzer::default();
    let result = analyzer.analyze(&ctx)?;

    let max_cyclomatic = args
        .max_cyclomatic
        .unwrap_or(config.complexity.cyclomatic_error);
    let max_cognitive = args
        .max_cognitive
        .unwrap_or(config.complexity.cognitive_error);

    match result.check_thresholds(max_cyclomatic, max_cognitive) {
        Ok(()) => {
            eprintln!(
                "All {} functions within thresholds (cyclomatic <= {}, cognitive <= {})",
                result.summary.total_functions, max_cyclomatic, max_cognitive
            );
            Ok(())
        }
        Err(violations) => {
            eprintln!(
                "Complexity threshold exceeded in {} function(s):\n",
                violations.len()
            );
            for v in &violations {
                eprintln!(
                    "  {}:{} - {}: cyclomatic={}, cognitive={}",
                    v.file, v.line, v.name, v.cyclomatic, v.cognitive
                );
            }
            eprintln!(
                "\nThresholds: cyclomatic <= {}, cognitive <= {}",
                max_cyclomatic, max_cognitive
            );
            Err(omen::core::Error::threshold_violation(
                format!(
                    "{} function(s) exceed complexity thresholds",
                    violations.len()
                ),
                violations.len() as f64,
            ))
        }
    }
}

fn run_score_check(path: &PathBuf, config: &Config, args: &ScoreArgs) -> omen::core::Result<()> {
    let file_set = FileSet::from_path(path, config)?;
    let ctx = build_context(path, &file_set, config);

    let analyzer = omen::score::Analyzer::default();
    let result = analyzer.analyze(&ctx)?;

    let min_score = args
        .fail_under
        .unwrap_or_else(|| config.score.fail_under.unwrap_or(80.0));

    match result.check_threshold(min_score) {
        Ok(()) => {
            eprintln!(
                "Score {:.1} ({}) meets minimum {:.1}",
                result.overall_score, result.grade, min_score
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn run_churn_analyzer(
    path: &PathBuf,
    config: &Config,
    format: Format,
    days: u32,
) -> omen::core::Result<()> {
    let file_set = FileSet::from_path(path, config)?;
    let ctx = build_context(path, &file_set, config);
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
    let ctx = build_context(path, &file_set, config);

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
            // Canonicalize path to handle "." and get actual directory name
            let repo_name = std::fs::canonicalize(path)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "unknown".to_string());
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
                "trend",
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

            let completed = std::sync::atomic::AtomicU64::new(0);
            let output_dir = &args.output;

            // Helper: run an analyzer and save its JSON output
            macro_rules! run_analyzer {
                ($analyzer:expr, $name:expr, $filename:expr) => {{
                    if !skip_list.contains(&$name) {
                        let result: Value = match $analyzer.analyze(&ctx) {
                            Ok(r) => serde_json::to_value(&r)
                                .unwrap_or(json!({"error": "serialization failed"})),
                            Err(e) => json!({"error": e.to_string()}),
                        };
                        let output_path = output_dir.join(format!("{}.json", $filename));
                        let _ = std::fs::write(
                            &output_path,
                            serde_json::to_string_pretty(&result).unwrap_or_default(),
                        );
                        let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        if let Some(ref bar) = progress {
                            bar.set_position(done);
                        } else {
                            eprintln!("Generated: {}", output_path.display());
                        }
                    }
                }};
            }

            // Phase 1: Run all analyzers in parallel groups.
            // Group A: File-parsing analyzers (CPU-bound, share tree-sitter work)
            // Group B: Git-heavy analyzers (I/O-bound, overlap with CPU work)
            // Group C: Mixed analyzers (benefit from warm OS caches)
            let churn_days = args
                .days
                .unwrap_or_else(|| omen::git::parse_since_to_days(&args.since).unwrap_or(u32::MAX));

            std::thread::scope(|s| {
                // Group A: file-based analyzers
                s.spawn(|| {
                    run_analyzer!(
                        omen::analyzers::complexity::Analyzer::default(),
                        "complexity",
                        "complexity"
                    );
                    run_analyzer!(omen::analyzers::satd::Analyzer::default(), "satd", "satd");
                    run_analyzer!(
                        omen::analyzers::deadcode::Analyzer::default(),
                        "deadcode",
                        "deadcode"
                    );
                    run_analyzer!(
                        omen::analyzers::duplicates::Analyzer::default(),
                        "duplicates",
                        "duplicates"
                    );
                    run_analyzer!(
                        omen::analyzers::cohesion::Analyzer::default(),
                        "cohesion",
                        "cohesion"
                    );
                    run_analyzer!(
                        omen::analyzers::repomap::Analyzer::default(),
                        "repomap",
                        "repomap"
                    );
                });

                // Group B: git-heavy analyzers (ownership is the longest at ~57s)
                s.spawn(|| {
                    run_analyzer!(
                        omen::analyzers::ownership::Analyzer::default(),
                        "ownership",
                        "ownership"
                    );
                    run_analyzer!(
                        omen::analyzers::churn::Analyzer::new().with_days(churn_days),
                        "churn",
                        "churn"
                    );
                    run_analyzer!(
                        omen::analyzers::temporal::Analyzer::default(),
                        "temporal",
                        "temporal"
                    );
                    run_analyzer!(
                        omen::analyzers::changes::Analyzer::default(),
                        "changes",
                        "changes"
                    );
                });

                // Group C: mixed file+git analyzers
                s.spawn(|| {
                    run_analyzer!(
                        omen::analyzers::graph::Analyzer::default(),
                        "graph",
                        "graph"
                    );
                    run_analyzer!(
                        omen::analyzers::smells::Analyzer::default(),
                        "smells",
                        "smells"
                    );
                    run_analyzer!(
                        omen::analyzers::flags::Analyzer::default(),
                        "flags",
                        "flags"
                    );
                    run_analyzer!(
                        omen::analyzers::defect::Analyzer::default(),
                        "defect",
                        "defect"
                    );
                    run_analyzer!(
                        omen::analyzers::hotspot::Analyzer::default(),
                        "hotspots",
                        "hotspots"
                    );
                    run_analyzer!(omen::analyzers::tdg::Analyzer::default(), "tdg", "tdg");
                });
            });

            // Phase 2: Score (reads pre-generated JSON files, nearly instant)
            if !skip_list.contains(&"score") {
                if let Some(ref bar) = progress {
                    bar.set_message("score...");
                }
                let result: Value =
                    match omen::score::compute_from_data_dir(output_dir, ctx.files.files().len()) {
                        Ok(r) => serde_json::to_value(&r)
                            .unwrap_or(json!({"error": "serialization failed"})),
                        Err(e) => json!({"error": e.to_string()}),
                    };
                let output_path = output_dir.join("score.json");
                std::fs::write(&output_path, serde_json::to_string_pretty(&result)?)?;
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if let Some(ref bar) = progress {
                    bar.set_position(done);
                } else {
                    eprintln!("Generated: {}", output_path.display());
                }
            }

            // Phase 3: Trend data (analyzes historical commits)
            if !skip_list.contains(&"trend") {
                if let Some(ref bar) = progress {
                    bar.set_message("trend...");
                }
                match omen::score::analyze_trend(
                    path,
                    config,
                    &args.since,
                    omen::cli::TrendPeriod::Monthly,
                ) {
                    Ok(trend_data) => {
                        let output_path = output_dir.join("trend.json");
                        if let Err(e) =
                            std::fs::write(&output_path, serde_json::to_string_pretty(&trend_data)?)
                        {
                            eprintln!("Warning: failed to write trend.json: {}", e);
                        } else {
                            let done =
                                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            if let Some(ref bar) = progress {
                                bar.set_position(done);
                            } else {
                                eprintln!("Generated: {}", output_path.display());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: trend analysis failed: {}", e);
                    }
                }
            }

            if let Some(bar) = progress {
                bar.finish_with_message("done");
            }
            eprintln!("Report data generated in: {}", output_dir.display());
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
                "trend",
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

fn run_mutation(
    path: &PathBuf,
    config: &Config,
    args: &MutationArgs,
    format: Format,
) -> omen::core::Result<()> {
    use omen::analyzers::mutation;
    use omen::analyzers::mutation::ml_predictor::{SurvivabilityPredictor, TrainingData};
    use omen::analyzers::mutation::MutantStatus;

    let mut file_set = FileSet::from_path(path, config)?;

    // Load predictor model if --skip-predicted is specified omen:ignore
    let predictor = if args.skip_predicted.is_some() {
        let model_path = args
            .model
            .clone()
            .unwrap_or_else(|| path.join(SurvivabilityPredictor::default_model_path()));
        let p = SurvivabilityPredictor::load_or_default(&model_path);
        if !p.is_trained() {
            eprintln!(
                "Warning: No trained model found at {}. Run 'omen mutation train' first.",
                model_path.display()
            );
        }
        Some(p)
    } else {
        None
    };

    // Apply glob filter if specified
    if let Some(ref pattern) = args.common.glob {
        file_set = file_set.filter_by_glob(pattern);
    }

    // Apply exclude filter if specified
    if let Some(ref pattern) = args.common.exclude {
        file_set = file_set.exclude_by_glob(pattern);
    }

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

    if let Some(ref s) = spinner {
        s.set_message(format!("Analyzing {} files...", file_set.len()));
    }

    // Parse operators
    let operators: Vec<String> = args
        .operators
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .collect();

    // Build analyzer
    let mut analyzer = mutation::Analyzer::new()
        .operators(operators)
        .test_command(args.test_command.clone())
        .timeout(args.timeout)
        .dry_run(args.dry_run);

    if args.check {
        analyzer = analyzer.min_score(Some(args.min_score));
    }

    // Configure ML-based filtering if --skip-predicted is set omen:ignore
    if let Some(threshold) = args.skip_predicted {
        if let Some(p) = predictor {
            analyzer = analyzer.skip_predicted(Some(threshold)).predictor(p);
        }
    }

    let mut ctx = build_context(path, &file_set, config);

    // Add progress callback
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

    // Output results
    match format {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Format::Markdown => {
            println!("# Mutation Testing Report\n");
            println!("## Summary\n");
            println!("- **Total Files**: {}", result.summary.total_files);
            println!("- **Total Mutants**: {}", result.summary.total_mutants);
            println!("- **Killed**: {}", result.summary.killed);
            println!("- **Survived**: {}", result.summary.survived);
            println!("- **Timeout**: {}", result.summary.timeout);
            println!("- **Error**: {}", result.summary.error);
            if result.summary.skipped > 0 {
                println!("- **Skipped**: {} (ML predicted)", result.summary.skipped);
            }
            println!(
                "- **Mutation Score**: {:.1}%",
                result.summary.mutation_score * 100.0
            );
            println!("- **Duration**: {}ms\n", result.summary.duration_ms);

            if !result.summary.by_operator.is_empty() {
                println!("## By Operator\n");
                println!("| Operator | Total | Killed | Survived |");
                println!("|----------|-------|--------|----------|");
                for (op, stats) in &result.summary.by_operator {
                    println!(
                        "| {} | {} | {} | {} |",
                        op, stats.total, stats.killed, stats.survived
                    );
                }
                println!();
            }

            if !result.files.is_empty() {
                println!("## Files\n");
                for file in &result.files {
                    println!("### {} (score: {:.1}%)\n", file.path, file.score * 100.0);
                    if file.skipped > 0 {
                        println!(
                            "- Killed: {}, Survived: {}, Skipped: {}, Timeout: {}, Error: {}\n",
                            file.killed, file.survived, file.skipped, file.timeout, file.error
                        );
                    } else {
                        println!(
                            "- Killed: {}, Survived: {}, Timeout: {}, Error: {}\n",
                            file.killed, file.survived, file.timeout, file.error
                        );
                    }
                }
            }
        }
        Format::Text => {
            println!("Mutation Testing Report");
            println!("=======================");
            println!("Files: {}", result.summary.total_files);
            println!("Mutants: {}", result.summary.total_mutants);
            if result.summary.skipped > 0 {
                println!(
                    "Killed: {} | Survived: {} | Skipped: {} | Timeout: {} | Error: {}",
                    result.summary.killed,
                    result.summary.survived,
                    result.summary.skipped,
                    result.summary.timeout,
                    result.summary.error
                );
            } else {
                println!(
                    "Killed: {} | Survived: {} | Timeout: {} | Error: {}",
                    result.summary.killed,
                    result.summary.survived,
                    result.summary.timeout,
                    result.summary.error
                );
            }
            println!(
                "Mutation Score: {:.1}%",
                result.summary.mutation_score * 100.0
            );
            println!("Duration: {}ms", result.summary.duration_ms);
        }
    }

    // Check mode: fail if score below threshold
    if args.check && result.summary.mutation_score < args.min_score {
        return Err(omen::core::Error::analysis(format!(
            "Mutation score {:.1}% is below minimum threshold {:.1}%",
            result.summary.mutation_score * 100.0,
            args.min_score * 100.0
        )));
    }

    // Save results to history if --record flag is set
    if args.record && !args.dry_run {
        use std::io::Write;

        let history_path = path.join(".omen/mutation-history.jsonl");

        // Ensure .omen directory exists
        if let Some(parent) = history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Append results to history file (JSONL format - one JSON object per line)
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path);

        match file {
            Ok(mut f) => {
                let mut count = 0;
                for file_result in &result.files {
                    let source = std::fs::read_to_string(&file_result.path).unwrap_or_default();

                    for mutation_result in &file_result.mutants {
                        let was_killed = match mutation_result.status {
                            MutantStatus::Killed => true,
                            MutantStatus::Survived => false,
                            _ => continue,
                        };

                        let lines: Vec<&str> = source.lines().collect();
                        let line_idx = mutation_result.mutant.line.saturating_sub(1) as usize;
                        let start = line_idx.saturating_sub(5);
                        let end = (line_idx + 6).min(lines.len());
                        let source_context = lines[start..end].join("\n");

                        let record = TrainingData {
                            mutant: mutation_result.mutant.clone(),
                            source_context,
                            was_killed,
                            execution_time_ms: mutation_result.duration_ms,
                        };

                        if let Ok(json) = serde_json::to_string(&record) {
                            let _ = writeln!(f, "{}", json);
                            count += 1;
                        }
                    }
                }
                eprintln!("Saved {} results to {}", count, history_path.display());
            }
            Err(e) => {
                eprintln!("Warning: Failed to open history file: {}", e);
            }
        }
    }

    Ok(())
}

fn run_mutation_train(path: &std::path::Path, args: &MutationTrainArgs) -> omen::core::Result<()> {
    use omen::analyzers::mutation::ml_predictor::{SurvivabilityPredictor, TrainingData};
    use std::io::BufRead;

    let history_path = args
        .history
        .clone()
        .unwrap_or_else(|| path.join(".omen/mutation-history.jsonl"));

    let model_path = args
        .model
        .clone()
        .unwrap_or_else(|| path.join(SurvivabilityPredictor::default_model_path()));

    // Check if history file exists
    if !history_path.exists() {
        return Err(omen::core::Error::analysis(format!(
            "History file not found: {}\nRun 'omen mutation --record' first to collect training data.",
            history_path.display()
        )));
    }

    // Read training data from history file (JSONL format)
    let file = std::fs::File::open(&history_path)
        .map_err(|e| omen::core::Error::analysis(format!("Failed to open history file: {}", e)))?;

    let reader = std::io::BufReader::new(file);
    let mut training_data: Vec<TrainingData> = Vec::new();

    for line in reader.lines() {
        let line =
            line.map_err(|e| omen::core::Error::analysis(format!("Failed to read line: {}", e)))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TrainingData>(&line) {
            Ok(data) => training_data.push(data),
            Err(e) => {
                eprintln!("Warning: Skipping malformed line: {}", e);
            }
        }
    }

    if training_data.is_empty() {
        return Err(omen::core::Error::analysis(
            "No valid training data found in history file".to_string(),
        ));
    }

    println!(
        "Training model from {} historical results...",
        training_data.len()
    );

    // Train the predictor
    let mut predictor = SurvivabilityPredictor::new();
    predictor
        .train(&training_data)
        .map_err(|e| omen::core::Error::analysis(format!("Training failed: {}", e)))?;

    // Ensure output directory exists
    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            omen::core::Error::analysis(format!("Failed to create directory: {}", e))
        })?;
    }

    // Save the model
    predictor
        .save(&model_path)
        .map_err(|e| omen::core::Error::analysis(format!("Failed to save model: {}", e)))?;

    println!("Model saved to {}", model_path.display());
    println!("\nOperator kill rates learned:");
    for (op, rate) in predictor.operator_kill_rates() {
        println!("  {}: {:.1}%", op, rate * 100.0);
    }

    Ok(())
}
