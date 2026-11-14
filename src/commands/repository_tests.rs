use super::*;
use clap::Parser;

fn cli(arguments: &[&str]) -> Cli {
    let mut full = vec!["omen"];
    full.extend(arguments);
    Cli::parse_from(full)
}

#[test]
fn test_a_local_directory_is_the_repository() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().to_str().expect("utf-8");
    let (resolved, cleanup) =
        resolve_repo_path(&cli(&["complexity", "-p", path])).expect("resolve");
    assert_eq!(resolved, directory.path());
    assert!(cleanup.is_none());
}

#[test]
fn test_a_command_that_reads_the_repository_refuses_a_path_that_is_not_a_directory() {
    let error = resolve_repo_path(&cli(&["complexity", "-p", "/no/such/directory"]))
        .expect_err("not a directory");
    assert!(error.to_string().contains("is not a directory"), "{error}");
}

#[test]
fn test_a_command_that_ignores_the_repository_accepts_any_path() {
    assert!(resolve_repo_path(&cli(&["mcp", "manifest", "-p", "/no/such/directory"])).is_ok());
}

#[test]
fn test_the_message_names_the_flag_for_a_period_or_a_port() {
    let trend = cli(&["score", "trend", "-p", "monthly"]);
    assert!(absent_path_message(&trend).contains("`--period`"));
    let serve = cli(&["report", "serve", "-p", "3000"]);
    assert!(absent_path_message(&serve).contains("`--port`"));
}

#[test]
fn test_local_ref_is_the_ref_for_a_local_path_only() {
    assert_eq!(
        local_ref(&cli(&["impact", "run", "--ref", "main"])),
        Some("main")
    );
    assert_eq!(local_ref(&cli(&["impact", "run"])), None);
}
