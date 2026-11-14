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

#[test]
fn test_generate_writes_security_json_and_validate_expects_it() {
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
    let text = std::fs::read_to_string(data.join("security.json")).expect("security.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert!(value["summary"]["sinks"].is_number());

    std::fs::remove_file(data.join("security.json")).expect("remove");
    let validate = run(
        &path,
        &Config::default(),
        &subcommand(&["validate", "--data", data.to_str().expect("utf-8")]),
        None,
    );
    let message = format!(
        "{:?}",
        validate.expect_err("the other files are missing too")
    );
    assert!(message.contains("validation errors"), "{message}");
    assert!(EXPECTED_DATA_FILES.contains(&"security"));
}

#[test]
fn test_an_analyzer_file_replaces_the_old_file_in_one_step() {
    let directory = tempfile::tempdir().expect("directory");
    let target = directory.path().join("security.json");
    std::fs::write(&target, "previous").expect("write");
    write_atomically(&target, "{\"summary\": {}}").expect("write");
    assert_eq!(
        std::fs::read_to_string(&target).expect("read"),
        "{\"summary\": {}}"
    );
    let names: Vec<String> = std::fs::read_dir(directory.path())
        .expect("list")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["security.json"]);
}
