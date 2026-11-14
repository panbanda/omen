//! The repository that a command reads: a local directory or a remote clone.

use std::path::PathBuf;

use crate::cli::{Cli, Command, McpSubcommand, ReportSubcommand};
use crate::git::{clone_remote, is_remote_repo, CloneOptions};
/// Resolve the repository path, cloning if it's a remote reference.
/// Returns (resolved_path, cleanup_path) where cleanup_path is Some if we cloned a temp repo.
pub fn resolve_repo_path(cli: &Cli) -> crate::core::Result<(PathBuf, Option<PathBuf>)> {
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
        Err(crate::core::Error::InvalidArgument(absent_path_message(
            cli,
        )))
    }
}

/// Tells if a command reads the repository at `-p`. A command that does not
/// read it ignores the value. The `match` has no wildcard arm, so a new
/// command needs a decision here.
pub fn reads_repository(command: &Command) -> bool {
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
pub fn absent_path_message(cli: &Cli) -> String {
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
pub fn cleanup_repo(path: &PathBuf) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        eprintln!("Warning: Failed to clean up cloned repository: {}", e);
    }
}

/// The value of `--ref` for a repository on the disk. A remote repository is
/// a clone at that commit, thus its files on the disk are the commit.
pub fn local_ref(cli: &Cli) -> Option<&str> {
    cli.git_ref
        .as_deref()
        .filter(|_| !is_remote_repo(&cli.path.to_string_lossy()))
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
