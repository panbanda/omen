//! Omen CLI - Multi-language code analysis for AI assistants.

use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use rayon::ThreadPoolBuilder;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use omen::analyzers::impact::Direction;
#[cfg(feature = "mutation")]
use omen::cli::MutationSubcommand;
use omen::cli::{
    Cli, Command, ImpactDirection, McpSubcommand, OutputFormat, ReportSubcommand, ScoreSubcommand,
};
use omen::commands;
use omen::commands::analyzers::{
    self, ChurnRequest, ClonesRequest, DeadcodeRequest, FlagsRequest, Plain, WindowRequest,
    WindowedRequest,
};
use omen::commands::benchmark::BenchmarkRequest;
use omen::commands::complexity::ComplexityRequest;
use omen::commands::context::ContextRequest;
use omen::commands::diff::DiffRequest;
use omen::commands::eval::EvalRequest;
use omen::commands::impact::ImpactRequest;
use omen::commands::outline::OutlineRequest;
use omen::commands::scope::Scope;
use omen::commands::score::{ScoreRequest, TrendRequest};
use omen::commands::stubs::StubsRequest;
use omen::commands::symbol::SymbolRequest;
use omen::config::Config;
use omen::git::{clone_remote, is_remote_repo, CloneOptions};
use omen::mcp::McpServer;
use omen::output::Format;

fn logging_filter(verbose: bool, rust_log: Option<&str>) -> EnvFilter {
    let directive = rust_log.unwrap_or(if verbose { "debug" } else { "off" });
    EnvFilter::new(directive)
}

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
            commands::report::run(path, &config, &cmd.subcommand, local_ref(cli))?;
        }
        Command::Search(ref cmd) => {
            commands::search::run(path, &config, cmd.subcommand.clone(), format, &mut stdout())?;
        }
        #[cfg(feature = "mutation")]
        Command::Mutation(ref cmd) => match &cmd.subcommand {
            Some(MutationSubcommand::Train(args)) => {
                commands::mutation::run_train(path, args, &mut stdout())?;
            }
            None => {
                commands::mutation::run(path, &config, &cmd.args, format, &mut stdout())?;
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
/// The value of `--ref` for a repository on the disk. A remote repository is
/// a clone at that commit, thus its files on the disk are the commit.
fn local_ref(cli: &Cli) -> Option<&str> {
    cli.git_ref
        .as_deref()
        .filter(|_| !is_remote_repo(&cli.path.to_string_lossy()))
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
