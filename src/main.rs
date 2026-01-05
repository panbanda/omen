//! Omen CLI - Multi-language code analysis for AI assistants.

use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use omen::cli::{Cli, Command, OutputFormat};
use omen::config::Config;
use omen::core::{AnalysisContext, Analyzer, FileSet};
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

fn run(cli: Cli) -> omen::core::Result<()> {
    let config = match &cli.config {
        Some(path) => Config::from_file(path)?,
        None => Config::load_default(&cli.path)?,
    };

    let format = match cli.format {
        OutputFormat::Json => Format::Json,
        OutputFormat::Markdown => Format::Markdown,
        OutputFormat::Text => Format::Text,
    };

    match cli.command {
        Command::Mcp(_args) => {
            let server = McpServer::new(cli.path.clone(), config);
            server.run_stdio()?;
        }
        Command::Complexity(_args) => {
            run_analyzer::<omen::analyzers::complexity::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Satd(_args) => {
            run_analyzer::<omen::analyzers::satd::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Deadcode(_args) => {
            run_analyzer::<omen::analyzers::deadcode::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Churn(_args) => {
            run_analyzer::<omen::analyzers::churn::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Clones(_args) => {
            run_analyzer::<omen::analyzers::duplicates::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Defect(_args) => {
            run_analyzer::<omen::analyzers::defect::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Changes(_args) => {
            run_analyzer::<omen::analyzers::changes::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Diff(_args) => {
            // TODO: Implement diff analyzer
            eprintln!("Diff analyzer not yet implemented");
        }
        Command::Tdg(_args) => {
            run_analyzer::<omen::analyzers::tdg::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Graph(_args) => {
            run_analyzer::<omen::analyzers::graph::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Hotspot(_args) => {
            run_analyzer::<omen::analyzers::hotspot::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Temporal(_args) => {
            run_analyzer::<omen::analyzers::temporal::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Ownership(_args) => {
            run_analyzer::<omen::analyzers::ownership::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Cohesion(_args) => {
            run_analyzer::<omen::analyzers::cohesion::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Repomap(_args) => {
            run_analyzer::<omen::analyzers::repomap::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Smells(_args) => {
            run_analyzer::<omen::analyzers::smells::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Flags(_args) => {
            run_analyzer::<omen::analyzers::flags::Analyzer>(&cli.path, &config, format)?;
        }
        Command::Score(_args) => {
            run_analyzer::<omen::score::Analyzer>(&cli.path, &config, format)?;
        }
        Command::All(_args) => {
            // TODO: Run all analyzers
            eprintln!("All analyzers not yet implemented");
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
    let ctx = AnalysisContext::new(&file_set, config, Some(path));
    let analyzer = A::default();
    let result = analyzer.analyze(&ctx)?;
    format.format(&result, &mut stdout())?;
    Ok(())
}
