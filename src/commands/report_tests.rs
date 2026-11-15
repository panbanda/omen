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

#[test]
fn test_a_file_that_changed_during_analysis_fails_generate_and_keeps_the_old_files() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    std::fs::create_dir_all(&data).expect("data directory");
    std::fs::write(data.join("complexity.json"), "old").expect("old file");
    let ReportSubcommand::Generate(args) = subcommand(&[
        "generate",
        "--data",
        data.to_str().expect("utf-8"),
        "--skip",
        "trend,churn,changes,ownership,temporal,hotspot,defect,tdg",
    ]) else {
        unreachable!("the arguments name `generate`");
    };
    let source = std::sync::Arc::new(crate::commands::testing::ChangingSource::new(&path));

    let error = generate(&path, &Config::default(), &args, None, Some(source))
        .expect_err("a changed file fails the command");

    assert!(
        error.to_string().contains("changed or became unreadable"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(data.join("complexity.json")).expect("read"),
        "old"
    );
    assert!(!data.join("metadata.json").exists());
    assert!(!data.join("security.json").exists());
}

#[test]
fn test_generate_fails_when_an_analyzer_file_cannot_be_written() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    std::fs::create_dir_all(data.join("security.json"))
        .expect("a directory in the place of the file");
    let result = run(
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
    );
    let message = format!("{:?}", result.expect_err("the write fails"));
    assert!(message.contains("security.json"), "{message}");
}

fn git_in(directory: &std::path::Path, arguments: &[&str]) {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git starts");
    assert!(output.status.success(), "{arguments:?}: {output:?}");
}

#[test]
fn test_generate_fails_when_trend_json_cannot_be_written() {
    let (directory, path) = repository();
    git_in(&path, &["init", "-q"]);
    git_in(&path, &["add", "."]);
    git_in(&path, &["commit", "-q", "-m", "first"]);
    let data = directory.path().join("data");
    std::fs::create_dir_all(data.join("trend.json")).expect("a directory in the place of the file");
    let result = run(
        &path,
        &Config::default(),
        &subcommand(&[
            "generate",
            "--data",
            data.to_str().expect("utf-8"),
            "--skip",
            "churn,changes,ownership,temporal,hotspot,defect,tdg",
        ]),
        None,
    );
    let message = format!("{:?}", result.expect_err("the write fails"));
    assert!(message.contains("trend.json"), "{message}");
}

#[test]
fn test_since_wins_over_days_in_the_one_window() {
    let window = generate_window(Some("6m"), Some(30)).expect("window");
    assert_eq!(window.label, "6m");
    assert_eq!(window.since, "6m");
    assert_eq!(window.days, 180);
}

#[test]
fn test_days_alone_gives_the_window_for_every_part() {
    let window = generate_window(None, Some(30)).expect("window");
    assert_eq!(window.label, "30d");
    assert_eq!(window.since, "30d");
    assert_eq!(window.days, 30);
}

#[test]
fn test_the_label_is_the_normalised_window() {
    assert_eq!(
        generate_window(None, Some(30)).expect("window").label,
        "30d"
    );
    assert_eq!(
        generate_window(Some(" 1Y "), None).expect("window").label,
        "1y"
    );
    assert_eq!(
        generate_window(Some(" Forever "), None)
            .expect("window")
            .label,
        "all"
    );
    assert_eq!(generate_window(None, None).expect("window").label, "1y");
}

#[cfg(unix)]
#[test]
fn test_a_symlink_at_the_old_temporary_path_is_not_followed() {
    let directory = tempfile::tempdir().expect("directory");
    let victim = directory.path().join("victim.txt");
    std::fs::write(&victim, "keep").expect("write");
    let target = directory.path().join("metadata.json");
    let old_temporary = directory.path().join("metadata.json.tmp");
    std::os::unix::fs::symlink(&victim, &old_temporary).expect("symlink");
    write_atomically(&target, "{}").expect("write");
    assert_eq!(std::fs::read_to_string(&victim).expect("read"), "keep");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "{}");
}

#[test]
fn test_no_flag_gives_the_default_window_of_one_year() {
    let window = generate_window(None, None).expect("window");
    assert_eq!(window.since, "1y");
    assert_eq!(window.days, 365);
}

#[test]
fn test_a_window_of_zero_days_is_an_error() {
    assert!(generate_window(None, Some(0)).is_err());
}

fn generate_with_data_file_as_directory(name: &str) -> String {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    std::fs::create_dir_all(data.join(name)).expect("a directory in the place of the file");
    let result = run(
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
    );
    format!("{:?}", result.expect_err("the write fails"))
}

#[test]
fn test_generate_fails_when_metadata_json_cannot_be_written() {
    let message = generate_with_data_file_as_directory("metadata.json");
    assert!(message.contains("metadata.json"), "{message}");
}

#[test]
fn test_generate_fails_when_score_json_cannot_be_written() {
    let message = generate_with_data_file_as_directory("score.json");
    assert!(message.contains("score.json"), "{message}");
}

#[test]
fn test_generate_writes_the_one_window_into_metadata() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    run(
        &path,
        &Config::default(),
        &subcommand(&[
            "generate",
            "--data",
            data.to_str().expect("utf-8"),
            "--since",
            "6m",
            "--days",
            "30",
            "--skip",
            "trend,churn,changes,ownership,temporal,hotspot,defect,tdg",
        ]),
        None,
    )
    .expect("generate");
    let text = std::fs::read_to_string(data.join("metadata.json")).expect("metadata.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(value["since"], "6m");
}
