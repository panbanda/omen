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

use omen::analyzers::impact::Direction;
use omen::cli::GateMode;
use omen::cli::{
    Cli, Command, ImpactDirection, McpSubcommand, OutputFormat, ReportSubcommand, ScoreSubcommand,
    SearchSubcommand,
};
#[cfg(feature = "mutation")]
use omen::cli::{MutationArgs, MutationSubcommand, MutationTrainArgs};
use omen::commands;
use omen::commands::analyzers::{
    self, since_flag_days, ChurnRequest, ClonesRequest, DeadcodeRequest, FlagsRequest, Plain,
    WindowRequest, WindowedRequest,
};
use omen::commands::benchmark::BenchmarkRequest;
use omen::commands::complexity::ComplexityRequest;
use omen::commands::context::ContextRequest;
use omen::commands::diff::DiffRequest;
use omen::commands::eval::EvalRequest;
use omen::commands::gate::{gate_dispatch, resolve_gate_mode};
use omen::commands::impact::ImpactRequest;
use omen::commands::outline::OutlineRequest;
#[cfg(feature = "mutation")]
use omen::commands::scope::write_report_file;
use omen::commands::scope::{build_context, filtered_file_set, Scope};
use omen::commands::score::{ScoreRequest, TrendRequest};
use omen::commands::stubs::StubsRequest;
use omen::commands::symbol::SymbolRequest;
use omen::config::Config;
use omen::core::progress::is_tty;
use omen::core::{AnalysisContext, Analyzer, FileSet};
use omen::git::{clone_remote, is_remote_repo, CloneOptions};
use omen::mcp::McpServer;
use omen::output::{format_with_limits, Format};
fn logging_filter(verbose: bool, rust_log: Option<&str>) -> EnvFilter {
    let directive = rust_log.unwrap_or(if verbose { "debug" } else { "off" });
    EnvFilter::new(directive)
}

/// The number of results that `search query` gives if `--top` is absent.
const DEFAULT_SEARCH_RESULTS: usize = 10;

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(logging_filter(
            cli.verbose,
            std::env::var("RUST_LOG").ok().as_deref(),
        ))
        .init();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:#}");
            if matches!(e, omen::core::Error::ThresholdViolation { .. }) {
                ExitCode::from(2)
            } else {
                ExitCode::FAILURE
            }
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
    } else if cli.path.is_dir() || !reads_repository(&cli.command) {
        Ok((cli.path.clone(), None))
    } else {
        Err(omen::core::Error::InvalidArgument(absent_path_message(cli)))
    }
}

/// Tells if a command reads the repository at `-p`. A command that does not
/// read it ignores the value. The `match` has no wildcard arm, so a new
/// command needs a decision here.
fn reads_repository(command: &Command) -> bool {
    match command {
        Command::Mcp(command) => match command.subcommand {
            Some(McpSubcommand::Manifest) => false,
            None => true,
        },
        Command::Report(command) => match command.subcommand {
            // `serve` uses the repository path to find the report file.
            ReportSubcommand::Generate(_) | ReportSubcommand::Serve(_) => true,
            ReportSubcommand::Validate(_) | ReportSubcommand::Render(_) => false,
        },
        Command::Benchmark(_) | Command::Eval(_) => false,
        #[cfg(feature = "mutation")]
        Command::Mutation(_) => true,
        Command::Complexity(_)
        | Command::Satd(_)
        | Command::Stubs(_)
        | Command::Deadcode(_)
        | Command::Churn(_)
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
        | Command::Flags(_)
        | Command::Score(_)
        | Command::All(_)
        | Command::Context(_)
        | Command::Search(_)
        | Command::Outline(_)
        | Command::Impact(_)
        | Command::Symbol(_) => true,
    }
}

/// The message for a `-p` value that is not a directory. Before 5.0, `-p` was
/// the period of `score trend` and the port of `report serve`, so the message
/// names the flag that has that meaning now.
fn absent_path_message(cli: &Cli) -> String {
    let message = format!(
        "The repository path `{}` is not a directory.",
        cli.path.display()
    );
    let replacement = match &cli.command {
        Command::Score(command) if command.subcommand.is_some() => Some(("--period", "period")),
        Command::Report(command) if matches!(command.subcommand, ReportSubcommand::Serve(_)) => {
            Some(("--port", "port"))
        }
        _ => None,
    };
    match replacement {
        Some((flag, meaning)) => {
            format!("{message} `-p` sets the repository path. Use `{flag}` to set the {meaning}.")
        }
        None => message,
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

    let format = match (cli.format, cli.compact) {
        (OutputFormat::Json, true) => Format::JsonCompact,
        (OutputFormat::Json, false) => Format::Json,
        (OutputFormat::Markdown, _) => Format::Markdown,
        (OutputFormat::Text, _) => Format::Text,
        (OutputFormat::Sarif, _) => Format::Sarif,
    };

    match &cli.command {
        Command::Mcp(cmd) => {
            match cmd.subcommand {
                Some(McpSubcommand::Manifest) => {
                    // Output MCP server manifest (standalone use only, registry publishing disabled) omen:ignore
                    // Tool names are derived from McpServer::tool_names() — the single source of
                    // truth kept adjacent to handle_tools_list in src/mcp/mod.rs.
                    let manifest = serde_json::json!({
                        "$schema": "https://registry.modelcontextprotocol.io/schemas/server.json",
                        "name": "panbanda/omen",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Code analysis tools for AI assistants",
                        "tools": omen::mcp::McpServer::tool_names()
                    });
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                }
                None => {
                    let server = McpServer::new_with_external_paths(
                        path.clone(),
                        config,
                        cmd.args.allow_external_paths,
                    );
                    server.run_stdio()?;
                }
            }
        }
        Command::Complexity(args) => {
            let request = ComplexityRequest {
                scope: Scope::from(&args.common),
                gate: args.gate.mode,
                max_cyclomatic: args.max_cyclomatic,
                max_cognitive: args.max_cognitive,
            };
            commands::complexity::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Stubs(args) => {
            let request = StubsRequest {
                scope: Scope::from(&args.common),
                gate: args.gate.mode,
                gate_severity: args.gate_severity,
            };
            commands::stubs::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Diff(args) => {
            let request = DiffRequest {
                target: args.target.clone(),
                top: args.pagination.top,
                offset: args.pagination.offset,
                max_bytes: args.max_bytes,
                max_nodes: args.max_nodes,
            };
            commands::diff::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Changes(args) => {
            analyzers::run_changes(path, &config, format, &Scope::from(args), &mut stdout())?;
        }
        Command::Deadcode(args) => {
            let request = DeadcodeRequest {
                scope: Scope::from(&args.common),
                cargo_check: args.cargo_check,
            };
            analyzers::run_deadcode(path, &config, format, &request, &mut stdout())?;
        }
        Command::Satd(args) => {
            analyzers::run_satd(path, &config, format, &Scope::from(args), &mut stdout())?;
        }
        Command::Clones(args) => {
            let request = ClonesRequest {
                scope: Scope::from(&args.common),
                include_tests: args.include_tests,
            };
            analyzers::run_clones(path, &config, format, &request, &mut stdout())?;
        }
        Command::Defect(args) => {
            let request = WindowedRequest {
                scope: Scope::from(&args.common),
                window: WindowRequest {
                    since: args.window.since.clone(),
                    days: args.window.days,
                },
            };
            analyzers::run_defect(path, &config, format, &request, &mut stdout())?;
        }
        Command::Hotspot(args) => {
            let request = WindowedRequest {
                scope: Scope::from(&args.common),
                window: WindowRequest {
                    since: args.window.since.clone(),
                    days: args.window.days,
                },
            };
            analyzers::run_hotspot(path, &config, format, &request, &mut stdout())?;
        }
        Command::Tdg(args) => {
            analyzers::run_plain(
                Plain::Tdg,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Graph(args) => {
            analyzers::run_plain(
                Plain::Graph,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Temporal(args) => {
            analyzers::run_plain(
                Plain::Temporal,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Ownership(args) => {
            analyzers::run_plain(
                Plain::Ownership,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Cohesion(args) => {
            analyzers::run_plain(
                Plain::Cohesion,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Repomap(args) => {
            analyzers::run_plain(
                Plain::Repomap,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Smells(args) => {
            analyzers::run_plain(
                Plain::Smells,
                path,
                &config,
                format,
                &Scope::from(args),
                &mut stdout(),
            )?;
        }
        Command::Churn(args) => {
            let request = ChurnRequest {
                scope: Scope::from(&args.common),
                window: WindowRequest {
                    since: args.since.clone(),
                    days: args.days,
                },
            };
            analyzers::run_churn(path, &config, format, &request, &mut stdout())?;
        }
        Command::Flags(args) => {
            let request = FlagsRequest {
                scope: Scope::from(&args.common),
                provider: args.provider.clone(),
                stale_days: args.stale_days,
            };
            analyzers::run_flags(path, &config, format, &request, &mut stdout())?;
        }
        Command::Score(cmd) => match &cmd.subcommand {
            // The gate belongs to the bare `score` command. A subcommand has no gate.
            Some(ScoreSubcommand::Trend(args)) => {
                if cmd.args.gate.mode.is_some() {
                    return Err(omen::core::Error::InvalidArgument(
                        "`score trend` has no gate. Remove the `--gate` flag.".to_string(),
                    ));
                }
                let request = TrendRequest {
                    since: args.since.clone(),
                    period: args.period,
                    samples: args.samples,
                };
                commands::score::run_trend(path, &config, &request, format, &mut stdout())?;
            }
            None => {
                let request = ScoreRequest {
                    gate: cmd.args.gate.mode,
                    fail_under: cmd.args.fail_under,
                };
                commands::score::run(path, &config, &request, format, &mut stdout())?;
            }
        },
        Command::All(args) => {
            commands::all::run(path, &config, &Scope::from(args), format, &mut stdout())?;
        }
        Command::Context(args) => {
            let request = ContextRequest {
                max_tokens: args.max_tokens,
            };
            commands::context::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Report(cmd) => {
            run_report(path, &config, &cmd.subcommand, local_ref(cli))?;
        }
        Command::Search(ref cmd) => {
            run_search(path, &config, cmd.subcommand.clone(), format)?;
        }
        #[cfg(feature = "mutation")]
        Command::Mutation(ref cmd) => match &cmd.subcommand {
            Some(MutationSubcommand::Train(args)) => {
                run_mutation_train(path, args)?;
            }
            None => {
                run_mutation(path, &config, &cmd.args, format)?;
            }
        },
        Command::Outline(args) => {
            let request = OutlineRequest {
                file: args.file.clone(),
                scope: Scope::from(&args.common),
            };
            commands::outline::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Impact(args) => {
            let request = ImpactRequest {
                symbol: args.symbol.clone(),
                depth: args.depth,
                direction: match args.direction {
                    ImpactDirection::Callers => Direction::Callers,
                    ImpactDirection::Callees => Direction::Callees,
                    ImpactDirection::Both => Direction::Both,
                },
                min_basis: args.min_basis,
                scope: Scope::from(&args.common),
                git_ref: local_ref(cli).map(str::to_string),
            };
            commands::impact::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Symbol(args) => {
            let request = SymbolRequest {
                name: args.name.clone(),
                include_source: !args.no_source,
                max_source_lines: args.max_source_lines,
                min_basis: args.min_basis,
                scope: Scope::from(&args.common),
                git_ref: local_ref(cli).map(str::to_string),
            };
            commands::symbol::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Eval(args) => {
            let request = EvalRequest {
                registration: args.registration.clone(),
                runs: args.runs.clone(),
                output: args.output.output.clone(),
            };
            commands::eval::run(&request, format, &mut stdout())?;
        }
        Command::Benchmark(args) => {
            let request = BenchmarkRequest {
                baseline: args.baseline.clone(),
                candidate: args.candidate.clone(),
                baseline_label: args.baseline_label.clone(),
                candidate_label: args.candidate_label.clone(),
                corpus: args.corpus.clone(),
                repetitions: args.repetitions,
                timeout_seconds: args.timeout,
                real_root: args.real_root.clone(),
                enforce: args.enforce,
            };
            commands::benchmark::run(&request, format, &mut stdout())?;
        }
    }

    Ok(())
}
fn run_report(
    path: &PathBuf,
    config: &Config,
    subcommand: &ReportSubcommand,
    git_ref: Option<&str>,
) -> omen::core::Result<()> {
    use serde_json::{json, Value};

    match subcommand {
        ReportSubcommand::Generate(args) => {
            let since_days = since_flag_days(&args.since)?;
            std::fs::create_dir_all(&args.data)?;

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
            let metadata_path = args.data.join("metadata.json");
            std::fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;

            let changes_path = args.data.join("diff.json");
            match std::fs::remove_file(&changes_path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into())
                }
                _ => {}
            }
            if let Some(reference) = git_ref {
                let changes = omen::changeset::build::compute(path, Some(reference), config)?;
                std::fs::write(&changes_path, serde_json::to_string_pretty(&changes)?)?;
            }

            let skip_list: Vec<&str> = args
                .skip
                .as_deref()
                .map(|s| s.split(',').collect())
                .unwrap_or_default();

            // Each name is a command name. It is also the artifact file stem and the
            // `--skip` value.
            let analyzer_names = [
                "complexity",
                "satd",
                "stubs",
                "deadcode",
                "churn",
                "clones",
                "defect",
                "changes",
                "tdg",
                "graph",
                "hotspot",
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
            let output_dir = &args.data;

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
            let churn_days = args.days.unwrap_or(since_days);

            std::thread::scope(|s| {
                // Group A: file-based analyzers
                s.spawn(|| {
                    run_analyzer!(
                        omen::analyzers::complexity::Analyzer::default(),
                        "complexity",
                        "complexity"
                    );
                    run_analyzer!(omen::analyzers::satd::Analyzer::default(), "satd", "satd");
                    run_analyzer!(omen::analyzers::stubs::Analyzer::new(), "stubs", "stubs");
                    run_analyzer!(
                        omen::analyzers::deadcode::Analyzer::default(),
                        "deadcode",
                        "deadcode"
                    );
                    run_analyzer!(
                        omen::analyzers::duplicates::Analyzer::default(),
                        "clones",
                        "clones"
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
                        omen::analyzers::defect::Analyzer::from_config(config),
                        "defect",
                        "defect"
                    );
                    run_analyzer!(
                        omen::analyzers::hotspot::Analyzer::from_config(config),
                        "hotspot",
                        "hotspot"
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
                let samples = args.samples.or_else(|| {
                    // All the history has no number of days. Use 50 years.
                    let days = if since_days == u32::MAX {
                        (365 * 50) as f64
                    } else {
                        since_days as f64
                    };
                    Some(omen::score::default_sample_count(days))
                });
                match omen::score::analyze_trend(
                    path,
                    config,
                    &args.since,
                    omen::cli::TrendPeriod::Monthly,
                    samples,
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
                "stubs",
                "deadcode",
                "churn",
                "clones",
                "defect",
                "changes",
                "tdg",
                "graph",
                "hotspot",
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
            use omen::report::Renderer;

            let renderer = Renderer::new()?;
            let output = args
                .output
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from(".omen/report.html"));
            renderer.render_to_file(&args.data, &output)?;
            eprintln!("Report rendered to: {}", output.display());
            eprintln!(
                "Compressed:         {}",
                Renderer::gz_path(&output).display()
            );
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

                let mr = omen::semantic::multi_repo::multi_repo_search(
                    &all_projects,
                    &args.query,
                    fetch,
                    args.min_score,
                )?;
                omen::semantic::SearchOutput::new(args.query.clone(), mr.total_symbols, mr.results)
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

            let output = omen::semantic::SearchOutput::new(
                output.query,
                output.total_symbols,
                filtered_results,
            );

            match format {
                Format::Json | Format::JsonCompact => format.format(&output, &mut stdout())?,
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
                            result.file, result.start_line, result.end_line
                        );
                        println!("   {}", result.signature);
                        println!();
                    }
                }
                Format::Sarif => format.format(&output, &mut stdout())?,
            }
        }
    }

    Ok(())
}

#[cfg(feature = "mutation")]
fn run_mutation(
    path: &PathBuf,
    config: &Config,
    args: &MutationArgs,
    format: Format,
) -> omen::core::Result<()> {
    use omen::analyzers::mutation;
    use omen::analyzers::mutation::ml_predictor::{SurvivabilityPredictor, TrainingData};
    use omen::analyzers::mutation::MutantStatus;

    // Route glob/exclude/changed-since through the same shared helper every
    // other analyzer uses, instead of hand-rolling glob/exclude here. This
    // also means mutation now honors --changed-since, which it previously
    // ignored entirely.
    let file_set = filtered_file_set(path, config, Some(&args.common))?;

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

    // Build analyzer. Note: the gate threshold is intentionally
    // *not* wired through `mutation::Analyzer::min_score` here (that would
    // make `analyze()` itself return `Err` before the report is written to
    // stdout, unlike every other gated command). Instead the CLI evaluates
    // the threshold itself below, after the report has been emitted.
    let mut analyzer = mutation::Analyzer::new()
        .operators(operators)
        .test_command(args.test_command.clone())
        .timeout(args.timeout)
        .dry_run(args.dry_run)
        .allow_dirty(args.allow_dirty);

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

    write_report_file(
        args.output.output.as_deref(),
        &serde_json::to_value(&result)?,
    )?;

    // Output results
    match format {
        Format::Json | Format::JsonCompact => {
            let value = serde_json::to_value(&result)?;
            format_with_limits(
                value,
                format,
                args.common.pagination.top,
                args.common.pagination.offset,
                &mut stdout(),
            )?;
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
                // `by_operator` is a hash map, so it has no fixed order.
                let mut by_operator: Vec<_> = result.summary.by_operator.iter().collect();
                by_operator.sort_by_key(|(operator, _)| *operator);
                for (op, stats) in by_operator {
                    println!(
                        "| {} | {} | {} | {} |",
                        op, stats.total, stats.killed, stats.survived
                    );
                }
                println!();
            }

            if !result.files.is_empty() {
                println!("## Files\n");
                // Apply the same --top/--offset pagination the JSON/SARIF
                // branches get from `format_with_limits`, so `mutation -f
                // markdown --top N` doesn't dump every file while JSON does
                // not. Summary totals above are computed from the full
                // `result`, not this paginated slice.
                let offset = args.common.pagination.offset.unwrap_or(0);
                let top = args.common.pagination.top.unwrap_or(0); // 0 = unlimited
                let limit = if top == 0 { usize::MAX } else { top };
                for file in result.files.iter().skip(offset).take(limit) {
                    println!("### {} (score: {:.1}%)\n", file.file, file.score * 100.0);
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
        Format::Sarif => {
            let value = serde_json::to_value(&result)?;
            format_with_limits(
                value,
                format,
                args.common.pagination.top,
                args.common.pagination.offset,
                &mut stdout(),
            )?;
        }
    }

    // The gate fails if the mutation score is below `--min-score`.
    let gate = resolve_gate_mode(args.gate.mode, config.mutation.gate);
    if gate != GateMode::Off && result.summary.mutation_score < args.min_score {
        let message = format!(
            "Mutation score {:.1}% is below minimum threshold {:.1}%",
            result.summary.mutation_score * 100.0,
            args.min_score * 100.0
        );
        eprintln!("{message}");
        gate_dispatch(
            gate,
            omen::core::Error::threshold_violation(message, result.summary.mutation_score),
        )?;
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
                    let source =
                        std::fs::read_to_string(path.join(&file_result.file)).unwrap_or_default();

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

/// The value of `--ref` for a repository on the disk. A remote repository is
/// a clone at that commit, thus its files on the disk are the commit.
fn local_ref(cli: &Cli) -> Option<&str> {
    cli.git_ref
        .as_deref()
        .filter(|_| !is_remote_repo(&cli.path.to_string_lossy()))
}

#[cfg(feature = "mutation")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbose_enables_debug_logging_without_rust_log() {
        assert_eq!(logging_filter(true, None).to_string(), "debug");
        assert_eq!(logging_filter(false, None).to_string(), "off");
        assert_eq!(logging_filter(true, Some("warn")).to_string(), "warn");
    }
}
