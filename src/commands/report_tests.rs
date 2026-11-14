use super::*;
use crate::commands::testing::repository;

fn subcommand(arguments: &[&str]) -> ReportSubcommand {
    use clap::Parser;
    let mut full = vec!["omen", "report"];
    full.extend(arguments);
    match crate::cli::Cli::parse_from(full).command {
        crate::cli::Command::Report(command) => command.subcommand,
        _ => unreachable!("the arguments name `report`"),
    }
}

#[test]
fn test_validate_fails_when_the_data_files_are_missing() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    std::fs::create_dir(&data).expect("data directory");
    let result = run(
        &path,
        &Config::default(),
        &subcommand(&["validate", "--data", data.to_str().expect("utf-8")]),
        None,
    );
    assert!(result.is_err());
}

#[test]
fn test_generate_writes_the_artifacts_named_like_the_commands() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    run(
        &path,
        &Config::default(),
        &subcommand(&[
            "generate",
            "--data",
            data.to_str().expect("utf-8"),
            "--skip",
            "trend,churn,changes,ownership,temporal,hotspot,defect,tdg",
        ]),
        None,
    )
    .expect("generate");
    for name in ["metadata", "complexity", "satd", "clones", "score"] {
        assert!(data.join(format!("{name}.json")).is_file(), "{name}.json");
    }
}
