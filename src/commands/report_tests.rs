use super::*;
use crate::commands::testing::repository;

fn write_atomically(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    crate::core::write_atomically(path, text.as_bytes(), 0o644)
}

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

    let error = generate(
        &path,
        &Config::default(),
        &args,
        None,
        Some(source),
        &commit_outputs,
    )
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
fn test_the_progress_total_counts_each_file_that_generate_counts() {
    assert_eq!(progress_total(&[], false), 21);
    assert_eq!(progress_total(&[], true), 22);
    assert_eq!(progress_total(&["trend", "score"], false), 19);
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

fn names_in(directory: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("list")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

const FAST_SKIP: &str = "churn,changes,ownership,temporal,hotspot,defect,tdg";

fn generate_args(data: &std::path::Path, skip: &str) -> crate::cli::ReportGenerateArgs {
    let ReportSubcommand::Generate(args) = subcommand(&[
        "generate",
        "--data",
        data.to_str().expect("utf-8"),
        "--skip",
        skip,
    ]) else {
        unreachable!("the arguments name `generate`");
    };
    args
}

/// A commit that stops after it replaced each file except `metadata.json`, as a process that
/// dies in the middle of a commit.
fn commit_without_metadata(outputs: &[Output]) -> Result<(), String> {
    for output in outputs {
        if let Output::Write(path, text) = output {
            if path.file_name().is_some_and(|name| name != "metadata.json") {
                write_atomically(path, text).map_err(|error| error.to_string())?;
            }
        }
    }
    Err("the process stopped".to_string())
}

#[test]
fn test_validate_and_render_find_a_commit_that_stopped_in_the_middle() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    let args = generate_args(&data, &format!("trend,{FAST_SKIP}"));
    generate(
        &path,
        &Config::default(),
        &args,
        None,
        None,
        &commit_outputs,
    )
    .expect("first run");
    let validate = || {
        run(
            &path,
            &Config::default(),
            &subcommand(&["validate", "--data", data.to_str().expect("utf-8")]),
            None,
        )
    };
    let first_problem = format!("{:?}", validate().expect_err("the trend file is missing"));
    assert!(
        !first_problem.contains("generation"),
        "a complete run has no mix: {first_problem}"
    );
    std::fs::write(
        path.join("shop.py"),
        "def helper():\n    if True:\n        return 3\n    return 1\n",
    )
    .expect("change the source");

    generate(
        &path,
        &Config::default(),
        &args,
        None,
        None,
        &commit_without_metadata,
    )
    .expect_err("the commit stops");

    let message = format!("{:?}", validate().expect_err("a mix is an error"));
    assert!(message.contains("generation"), "{message}");
    let render = crate::report::Renderer::new()
        .expect("renderer")
        .render_to_file(&data, &directory.path().join("report.html"));
    let message = format!("{:?}", render.expect_err("render finds the mix"));
    assert!(message.contains("generation"), "{message}");
}

#[test]
fn test_the_score_is_the_score_of_the_final_directory() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    generate(
        &path,
        &Config::default(),
        &generate_args(&data, &format!("trend,{FAST_SKIP}")),
        None,
        None,
        &commit_outputs,
    )
    .expect("first run");
    let first_score = std::fs::read_to_string(data.join("score.json")).expect("score");

    generate(
        &path,
        &Config::default(),
        &generate_args(&data, &format!("trend,complexity,{FAST_SKIP}")),
        None,
        None,
        &commit_outputs,
    )
    .expect("second run");

    let document = |name: &str| std::fs::read_to_string(data.join(name)).ok();
    let expected = crate::score::compute_from_documents(&document, 2).expect("score");
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("score.json")).expect("score"))
            .expect("JSON");
    let first: serde_json::Value = serde_json::from_str(&first_score).expect("JSON");
    assert_eq!(stored, serde_json::to_value(expected).expect("value"));
    assert_eq!(
        stored, first,
        "the kept complexity.json keeps its part of the score"
    );
    assert_eq!(
        stored["overall_score"].as_f64().map(f64::to_bits),
        first["overall_score"].as_f64().map(f64::to_bits)
    );
}

#[test]
fn test_the_old_metadata_is_gone_before_the_first_file_is_replaced() {
    let directory = tempfile::tempdir().expect("directory");
    let metadata = directory.path().join("metadata.json");
    let analyzer = directory.path().join("satd.json");
    std::fs::write(&metadata, "old metadata").expect("old");
    std::fs::write(&analyzer, "old satd").expect("old");
    let outputs = [
        Output::Write(analyzer.clone(), "new satd".to_string()),
        Output::Write(metadata.clone(), "new metadata".to_string()),
    ];
    let seen = std::cell::Cell::new(None);

    commit_outputs_with(&outputs, &|index| {
        if index == 0 {
            seen.set(Some(metadata.exists()));
        }
        Ok(())
    })
    .expect("the commit works");

    assert_eq!(seen.get(), Some(false), "a stop here leaves no metadata");
    assert_eq!(
        std::fs::read_to_string(&metadata).expect("read"),
        "new metadata"
    );
}

#[test]
fn test_a_failed_commit_puts_the_old_metadata_back() {
    let directory = tempfile::tempdir().expect("directory");
    let metadata = directory.path().join("metadata.json");
    let analyzer = directory.path().join("satd.json");
    std::fs::write(&metadata, "old metadata").expect("old");
    std::fs::write(&analyzer, "old satd").expect("old");
    let outputs = [
        Output::Write(analyzer.clone(), "new satd".to_string()),
        Output::Write(metadata.clone(), "new metadata".to_string()),
    ];

    commit_outputs_with(&outputs, &|index| {
        if index == 0 {
            Err(std::io::Error::other("injected fault"))
        } else {
            Ok(())
        }
    })
    .expect_err("the commit fails");

    assert_eq!(
        std::fs::read_to_string(&metadata).expect("read"),
        "old metadata"
    );
    assert_eq!(
        std::fs::read_to_string(&analyzer).expect("read"),
        "old satd"
    );
}

#[test]
fn test_render_and_validate_fail_without_metadata() {
    let (directory, path) = repository();
    let data = directory.path().join("data");
    generate(
        &path,
        &Config::default(),
        &generate_args(&data, &format!("trend,{FAST_SKIP}")),
        None,
        None,
        &commit_outputs,
    )
    .expect("generate");
    std::fs::remove_file(data.join("metadata.json")).expect("remove");

    let render = crate::report::Renderer::new()
        .expect("renderer")
        .render_to_file(&data, &directory.path().join("report.html"));
    let message = format!("{:?}", render.expect_err("render needs metadata"));
    assert!(message.contains("metadata.json"), "{message}");
    let validate = run(
        &path,
        &Config::default(),
        &subcommand(&["validate", "--data", data.to_str().expect("utf-8")]),
        None,
    );
    assert!(validate.is_err());
}

#[cfg(unix)]
#[test]
fn test_a_rollback_keeps_the_mode_of_each_old_file() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("directory");
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");
    std::fs::write(&first, "old first").expect("old");
    std::fs::set_permissions(&first, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let outputs = [
        Output::Write(first.clone(), "new first".to_string()),
        Output::Write(second.clone(), "new second".to_string()),
    ];

    let error = commit_outputs_with(&outputs, &|index| {
        if index == 1 {
            Err(std::io::Error::other("injected fault"))
        } else {
            Ok(())
        }
    })
    .expect_err("the commit fails");

    assert!(error.contains("injected fault"), "{error}");
    assert_eq!(std::fs::read_to_string(&first).expect("read"), "old first");
    let mode = std::fs::metadata(&first)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(!second.exists());
}

#[cfg(unix)]
#[test]
fn test_a_failed_rollback_is_in_the_error() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let directory = tempfile::tempdir().expect("directory");
    let first = directory.path().join("first.json");
    std::fs::write(&first, "old first").expect("old");
    let outputs = [Output::Write(first.clone(), "new first".to_string())];
    let locked = directory.path().to_path_buf();

    let error = commit_outputs_with(&outputs, &|_| {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))?;
        Err(std::io::Error::other("injected fault"))
    })
    .expect_err("the commit fails");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
        .expect("unlock");

    assert!(error.contains("injected fault"), "{error}");
    assert!(error.contains("rollback"), "{error}");
    assert!(error.contains("first.json"), "{error}");
}

#[test]
fn test_a_failed_write_leaves_every_old_file_unchanged() {
    let (directory, path) = repository();
    git_in(&path, &["init", "-q"]);
    git_in(&path, &["add", "."]);
    git_in(&path, &["commit", "-q", "-m", "first"]);
    let data = directory.path().join("data");
    std::fs::create_dir_all(data.join("security.json"))
        .expect("a directory in the place of a file");
    let old = ["metadata", "complexity", "satd", "score", "trend"];
    for name in old {
        std::fs::write(data.join(format!("{name}.json")), format!("old {name}")).expect("old");
    }

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
    assert!(message.contains("security.json"), "{message}");
    for name in old {
        assert_eq!(
            std::fs::read_to_string(data.join(format!("{name}.json"))).expect("read"),
            format!("old {name}"),
            "{name}"
        );
    }
    assert_eq!(
        names_in(&data),
        [
            "complexity.json",
            "metadata.json",
            "satd.json",
            "score.json",
            "security.json",
            "trend.json"
        ]
    );
}

#[test]
fn test_a_failed_trend_does_not_leave_the_old_trend_beside_new_data() {
    let (directory, git_path) = repository();
    git_in(&git_path, &["init", "-q"]);
    git_in(&git_path, &["add", "."]);
    git_in(&git_path, &["commit", "-q", "-m", "first"]);
    let data = directory.path().join("data");
    let arguments = [
        "generate",
        "--data",
        data.to_str().expect("utf-8"),
        "--skip",
        "churn,changes,ownership,temporal,hotspot,defect,tdg",
    ];
    run(&git_path, &Config::default(), &subcommand(&arguments), None).expect("first run");
    assert!(data.join("trend.json").is_file());
    let first_metadata = std::fs::read_to_string(data.join("metadata.json")).expect("read");

    let (_other, plain_path) = repository();
    run(
        &plain_path,
        &Config::default(),
        &subcommand(&arguments),
        None,
    )
    .expect("a trend failure is a warning");

    assert!(!data.join("trend.json").exists());
    let second_metadata = std::fs::read_to_string(data.join("metadata.json")).expect("read");
    assert_ne!(first_metadata, second_metadata);
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
