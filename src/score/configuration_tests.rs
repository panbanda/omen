use super::*;

/// A git repository with two copies of one function. The copies are shorter
/// than the default `min_tokens` of `[duplicates]`.
fn repository_with_a_short_clone(path: &std::path::Path) {
    let function = "package main\n\nfunc total(items []int) int {\n\tsum := 0\n\tfor _, item := range items {\n\t\tif item > 10 {\n\t\t\tsum += item * 2\n\t\t} else {\n\t\t\tsum += item\n\t\t}\n\t}\n\treturn sum\n}\n";
    std::fs::write(path.join("a.go"), function).unwrap();
    std::fs::write(path.join("b.go"), function.replace("total", "count")).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("run git");
        assert!(output.status.success(), "git {args:?}: {output:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test Author"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "add the functions"]);
}

/// The duplication component and the duplication factor of the defect
/// component use `[duplicates]` of the configuration.
#[test]
fn test_score_uses_the_configured_duplicates() {
    let directory = tempfile::TempDir::new().unwrap();
    repository_with_a_short_clone(directory.path());
    let mut config = crate::config::Config::default();
    config.duplicates.min_tokens = 20;
    let file_set = crate::core::FileSet::from_path(directory.path(), &config).unwrap();
    let ctx = AnalysisContext::new(&file_set, &config, Some(directory.path()))
        .with_git_path(directory.path());

    let clones = |analyzer: &Analyzer| {
        analyzer
            .duplicates_analyzer()
            .analyze(&ctx)
            .unwrap()
            .clones
            .len()
    };
    let duplication = |analyzer: &Analyzer| {
        analyzer
            .defect_analyzer()
            .analyze(&ctx)
            .unwrap()
            .files
            .iter()
            .map(|file| file.contributing_factors["duplication"])
            .fold(0.0_f32, f32::max)
    };
    let default = Analyzer::default();
    let configured = Analyzer::from_config(&config);
    assert_eq!(clones(&default), 0);
    assert_eq!(clones(&configured), 1);
    assert_eq!(duplication(&default), 0.0);
    assert!(duplication(&configured) > 0.0);
}

#[test]
fn test_score_forwards_the_configured_defect_window() {
    // The window used to reach only the score gate; ordinary `omen
    // score`, MCP and each trend sample built the analyzer with defaults.
    let mut config = crate::config::Config::default();
    config.defect.since = crate::config::HistoryWindow::parse("365d").unwrap();
    assert_eq!(Analyzer::from_config(&config).defect_churn_days, 365);
    assert_eq!(
        Analyzer::default().defect_churn_days,
        crate::config::DefectConfig::default().since.days()
    );
}
