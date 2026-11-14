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
use omen::cli::{Cli, Command, ImpactDirection, McpSubcommand, OutputFormat, ScoreSubcommand};
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
use omen::commands::security::SecurityRequest;
use omen::commands::stubs::StubsRequest;
use omen::commands::symbol::SymbolRequest;
use omen::config::Config;
use omen::mcp::McpServer;
use omen::output::Format;

/// The scope of a command, with the commit of `--ref` for a local repository.
fn scope_of(cli: &Cli, args: &omen::cli::AnalyzerArgs) -> Scope {
    Scope::from(args).with_ref(commands::repository::local_ref(cli))
}

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

fn run(cli: Cli) -> omen::core::Result<()> {
    // Configure rayon thread pool if -j/--jobs flag is specified
    if let Some(jobs) = cli.jobs {
        ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .ok(); // Ignore if already initialized
    }

    // Resolve repository path (clone if remote)
    let (path, cleanup_path) = commands::repository::resolve_repo_path(&cli)?;

    // Use a closure to ensure cleanup happens even on error
    let result = run_with_path(&cli, &path);

    // Clean up cloned repository if we created one
    if let Some(ref cleanup) = cleanup_path {
        commands::repository::cleanup_repo(cleanup);
    }

    result
}

fn run_with_path(cli: &Cli, path: &PathBuf) -> omen::core::Result<()> {
    commands::repository::check_ref(cli)?;

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
        (OutputFormat::Mermaid, _) => Format::Mermaid,
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
                scope: scope_of(cli, &args.common),
                gate: args.gate.mode,
                max_cyclomatic: args.max_cyclomatic,
                max_cognitive: args.max_cognitive,
            };
            commands::complexity::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Stubs(args) => {
            let request = StubsRequest {
                scope: scope_of(cli, &args.common),
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
            analyzers::run_changes(path, &config, format, &scope_of(cli, args), &mut stdout())?;
        }
        Command::Deadcode(args) => {
            let request = DeadcodeRequest {
                scope: scope_of(cli, &args.common),
                cargo_check: args.cargo_check,
            };
            analyzers::run_deadcode(path, &config, format, &request, &mut stdout())?;
        }
        Command::Satd(args) => {
            analyzers::run_satd(path, &config, format, &scope_of(cli, args), &mut stdout())?;
        }
        Command::Clones(args) => {
            let request = ClonesRequest {
                scope: scope_of(cli, &args.common),
                include_tests: args.include_tests,
            };
            analyzers::run_clones(path, &config, format, &request, &mut stdout())?;
        }
        Command::Defect(args) => {
            let request = WindowedRequest {
                scope: scope_of(cli, &args.common),
                window: WindowRequest {
                    since: args.window.since.clone(),
                    days: args.window.days,
                },
            };
            analyzers::run_defect(path, &config, format, &request, &mut stdout())?;
        }
        Command::Hotspot(args) => {
            let request = WindowedRequest {
                scope: scope_of(cli, &args.common),
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
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Graph(args) => {
            analyzers::run_plain(
                Plain::Graph,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Temporal(args) => {
            analyzers::run_plain(
                Plain::Temporal,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Ownership(args) => {
            analyzers::run_plain(
                Plain::Ownership,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Cohesion(args) => {
            analyzers::run_plain(
                Plain::Cohesion,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Repomap(args) => {
            analyzers::run_plain(
                Plain::Repomap,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Smells(args) => {
            analyzers::run_plain(
                Plain::Smells,
                path,
                &config,
                format,
                &scope_of(cli, args),
                &mut stdout(),
            )?;
        }
        Command::Churn(args) => {
            let request = ChurnRequest {
                scope: scope_of(cli, &args.common),
                window: WindowRequest {
                    since: args.since.clone(),
                    days: args.days,
                },
            };
            analyzers::run_churn(path, &config, format, &request, &mut stdout())?;
        }
        Command::Flags(args) => {
            let request = FlagsRequest {
                scope: scope_of(cli, &args.common),
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
            commands::all::run(path, &config, &scope_of(cli, args), format, &mut stdout())?;
        }
        Command::Context(args) => {
            let request = ContextRequest {
                max_tokens: args.max_tokens,
                git_ref: commands::repository::local_ref(cli).map(str::to_string),
            };
            commands::context::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Report(cmd) => {
            commands::report::run(
                path,
                &config,
                &cmd.subcommand,
                commands::repository::local_ref(cli),
            )?;
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
                scope: scope_of(cli, &args.common),
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
                scope: scope_of(cli, &args.common),
                git_ref: commands::repository::local_ref(cli).map(str::to_string),
            };
            commands::impact::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Symbol(args) => {
            let request = SymbolRequest {
                name: args.name.clone(),
                include_source: !args.no_source,
                max_source_lines: args.max_source_lines,
                min_basis: args.min_basis,
                scope: scope_of(cli, &args.common),
                git_ref: commands::repository::local_ref(cli).map(str::to_string),
            };
            commands::symbol::run(path, &config, &request, format, &mut stdout())?;
        }
        Command::Security(args) => {
            let request = SecurityRequest {
                section: args.section,
                category: args.category.clone(),
                entry: args.entry.clone(),
                sink: args.sink.clone(),
                min_basis: args.min_basis.clone(),
                experimental: args.experimental,
                scope: Scope::from(&args.common),
                gate: args.gate.mode,
                gate_categories: args.gate_category.clone(),
                gate_basis: args.gate_basis.clone(),
                target: args.target.clone(),
            };
            commands::security::run(path, &config, &request, format, &mut stdout())?;
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
