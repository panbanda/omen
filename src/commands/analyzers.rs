//! The commands that run one analyzer and print its report.
//!
//! The functions that build an analyzer from the configuration are public. The
//! MCP server calls them, so a command and its tool read the same values.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use indicatif::{ProgressBar, ProgressStyle};

use super::scope::{build_context, build_context_from, scoped_file_set, scoped_inputs, Scope};
use crate::analyzers;
use crate::cli::ChurnWindow;
use crate::config::Config;
use crate::core::progress::is_tty;
use crate::core::{Analyzer, Error, Result};
use crate::output::{format_with_limits, Format};

pub fn satd_analyzer(config: &Config) -> analyzers::satd::Analyzer {
    analyzers::satd::Analyzer::new().with_custom_markers(&config.satd.markers)
}

/// The clone analyzer. A value that is `None` comes from the configuration.
pub fn clones_analyzer(
    config: &Config,
    min_tokens: Option<usize>,
    similarity: Option<f64>,
    include_tests: bool,
) -> analyzers::duplicates::Analyzer {
    let configured = &config.duplicates;
    analyzers::duplicates::Analyzer::from_config(config)
        .with_min_tokens(min_tokens.unwrap_or(configured.min_tokens))
        .with_similarity_threshold(similarity.unwrap_or(configured.min_similarity))
        .with_exclude_tests(!include_tests && config.exclude_tests)
}

/// DEPRECATED(remove-in: 6.0): use `defect_analyzer_with_config`. This
/// constructor uses the default `[duplicates]` table.
#[deprecated(since = "5.2.0", note = "use `defect_analyzer_with_config`")]
pub fn defect_analyzer(days: u32) -> analyzers::defect::Analyzer {
    analyzers::defect::Analyzer::new().with_churn_days(days)
}

pub fn defect_analyzer_with_config(config: &Config, days: u32) -> analyzers::defect::Analyzer {
    analyzers::defect::Analyzer::from_config(config).with_churn_days(days)
}

pub fn hotspot_analyzer(days: u32) -> analyzers::hotspot::Analyzer {
    analyzers::hotspot::Analyzer::new().with_days(days)
}

pub fn churn_analyzer(days: u32) -> analyzers::churn::Analyzer {
    analyzers::churn::Analyzer::new().with_days(days)
}

pub fn changes_analyzer(days: u32) -> analyzers::changes::Analyzer {
    analyzers::changes::Analyzer::new().with_days(days)
}

/// The message for a `changes` window of all the history. The configuration
/// gives the same message for `changes.since`.
pub const CHANGES_ALL_MESSAGE: &str =
    "\"all\" is not a window of `changes`. Use a duration such as 30d.";

/// The days of the `changes` window: the `--since` value, or `changes.since`
/// of the configuration. The value `all` is an error.
pub fn changes_window(since: Option<&str>, config: &Config) -> Result<u32> {
    let Some(since) = since else {
        return Ok(config.changes.since.days());
    };
    match since_flag_days(since)? {
        u32::MAX => Err(Error::InvalidArgument(format!(
            "--since: {CHANGES_ALL_MESSAGE}"
        ))),
        days => Ok(days),
    }
}

pub fn score_analyzer(config: &Config) -> crate::score::Analyzer {
    crate::score::Analyzer::from_config(config)
}

/// The configuration with the options of `flags` applied.
pub fn flags_config(config: &Config, provider: Option<&str>, stale_days: Option<u32>) -> Config {
    let mut config = config.clone();
    if let Some(provider) = provider {
        config.feature_flags.providers = vec![provider.to_string()];
    }
    if let Some(stale_days) = stale_days {
        config.feature_flags.stale_days = stale_days;
    }
    config
}

/// The days of `churn.since` in the configuration.
pub fn configured_churn_days(config: &Config) -> u32 {
    config.churn.since.days()
}

/// The number of results that `churn` and `hotspot` give if `--top` is absent.
pub const DEFAULT_TOP: usize = 20;

/// The window of `defect` and `hotspot`. An input that is neither a duration
/// nor "all" is an error and does not widen to all history.
pub fn churn_window(since: Option<&str>, configured_days: u32) -> Result<u32> {
    ChurnWindow::resolve(since, configured_days)
        .map(|window| window.as_days())
        .map_err(Error::config)
}

/// The days of a `--since` value. Text that is not a time window is an error,
/// not all the history.
pub fn since_flag_days(since: &str) -> Result<u32> {
    crate::git::since_to_days(since)
        .map_err(|error| Error::InvalidArgument(format!("--since: {error}")))
}

pub fn run_analyzer<A: Analyzer + Default>(
    path: &PathBuf,
    config: &Config,
    format: Format,
    scope: Option<&Scope>,
    out: &mut dyn Write,
) -> Result<A::Output> {
    run_analyzer_instance(path, config, format, scope, A::default(), out)
}

pub fn run_analyzer_instance<A: Analyzer>(
    path: &PathBuf,
    config: &Config,
    format: Format,
    scope: Option<&Scope>,
    analyzer: A,
    out: &mut dyn Write,
) -> Result<A::Output> {
    let (file_set, source) = match scope {
        Some(scope) => scoped_inputs(path, config, scope)?,
        None => (crate::core::FileSet::from_path(path, config)?, None),
    };

    let spinner = if is_tty() {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("valid template"),
        );
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(spinner)
    } else {
        None
    };

    if let Some(ref spinner) = spinner {
        spinner.set_message(format!("Analyzing {} files...", file_set.len()));
    }

    let mut ctx = build_context_from(path, &file_set, config, source);

    let progress_counter = Arc::new(AtomicUsize::new(0));
    let total_files = file_set.len();
    let spinner_clone = spinner.clone();
    let counter_clone = progress_counter.clone();
    ctx = ctx.with_progress(move |current, _total| {
        counter_clone.store(current, Ordering::Relaxed);
        if let Some(ref spinner) = spinner_clone {
            spinner.set_message(format!("Analyzing... {}/{} files", current, total_files));
        }
    });

    let analyzer_name = analyzer.name();
    let result = analyzer.analyze(&ctx)?;

    if let Some(spinner) = spinner {
        spinner.finish_and_clear();
    }

    let top = scope
        .and_then(|scope| scope.top)
        .or_else(|| (analyzer_name == "hotspot").then_some(DEFAULT_TOP));
    let offset = scope.and_then(|scope| scope.offset);
    let mut value = serde_json::to_value(&result)?;
    let exclude_tests = scope.is_some_and(|scope| scope.exclude_tests);
    super::test_rows::mark(&mut value, analyzer_name, &ctx, exclude_tests);
    let mut out = out;
    format_with_limits(value, format, top, offset, &mut out)?;
    Ok(result)
}

/// The commands that run an analyzer without options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plain {
    Tdg,
    Graph,
    Ownership,
    Cohesion,
    Repomap,
    Smells,
}

pub fn run_plain(
    which: Plain,
    path: &PathBuf,
    config: &Config,
    format: Format,
    scope: &Scope,
    out: &mut dyn Write,
) -> Result<()> {
    let scope = Some(scope);
    match which {
        Plain::Tdg => {
            run_analyzer::<analyzers::tdg::Analyzer>(path, config, format, scope, out).map(drop)
        }
        Plain::Graph => {
            run_analyzer::<analyzers::graph::Analyzer>(path, config, format, scope, out).map(drop)
        }
        Plain::Ownership => {
            run_analyzer::<analyzers::ownership::Analyzer>(path, config, format, scope, out)
                .map(drop)
        }
        Plain::Cohesion => {
            run_analyzer::<analyzers::cohesion::Analyzer>(path, config, format, scope, out)
                .map(drop)
        }
        Plain::Repomap => {
            run_analyzer::<analyzers::repomap::Analyzer>(path, config, format, scope, out).map(drop)
        }
        Plain::Smells => {
            run_analyzer::<analyzers::smells::Analyzer>(path, config, format, scope, out).map(drop)
        }
    }
}

pub fn run_satd(
    path: &PathBuf,
    config: &Config,
    format: Format,
    scope: &Scope,
    out: &mut dyn Write,
) -> Result<()> {
    run_analyzer_instance(
        path,
        config,
        format,
        Some(scope),
        satd_analyzer(config),
        out,
    )
    .map(drop)
}

pub struct ClonesRequest {
    pub scope: Scope,
    pub include_tests: bool,
}

pub fn run_clones(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &ClonesRequest,
    out: &mut dyn Write,
) -> Result<()> {
    run_analyzer_instance(
        path,
        config,
        format,
        Some(&request.scope),
        clones_analyzer(config, None, None, request.include_tests),
        out,
    )
    .map(drop)
}

/// A churn window as the flag `--since` gives it.
#[derive(Clone, Debug, Default)]
pub struct WindowRequest {
    pub since: Option<String>,
}

pub struct WindowedRequest {
    pub scope: Scope,
    pub window: WindowRequest,
}

pub fn run_defect(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &WindowedRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let window = churn_window(request.window.since.as_deref(), config.defect.since.days())?;
    run_analyzer_instance(
        path,
        config,
        format,
        Some(&request.scope),
        defect_analyzer_with_config(config, window),
        out,
    )
    .map(drop)
}

pub fn run_temporal(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &WindowedRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let days = churn_window(
        request.window.since.as_deref(),
        analyzers::temporal::DEFAULT_DAYS,
    )?;
    run_analyzer_instance(
        path,
        config,
        format,
        Some(&request.scope),
        analyzers::temporal::Analyzer::new().with_days(days),
        out,
    )
    .map(drop)
}

pub fn run_hotspot(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &WindowedRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let window = churn_window(request.window.since.as_deref(), config.hotspot.since.days())?;
    run_analyzer_instance(
        path,
        config,
        format,
        Some(&request.scope),
        hotspot_analyzer(window),
        out,
    )
    .map(drop)
}

pub struct DeadcodeRequest {
    pub scope: Scope,
    /// Run `cargo check`, which executes build scripts.
    pub cargo_check: bool,
}

pub fn run_deadcode(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &DeadcodeRequest,
    out: &mut dyn Write,
) -> Result<()> {
    if request.cargo_check && request.scope.git_ref.is_some() {
        return Err(Error::InvalidArgument(
            "`--cargo-check` runs `cargo check` on the working tree. Remove `--cargo-check` or `--ref`."
                .to_string(),
        ));
    }
    run_analyzer_instance(
        path,
        config,
        format,
        Some(&request.scope),
        analyzers::deadcode::Analyzer::new().with_cargo_check(request.cargo_check),
        out,
    )
    .map(drop)
}

pub struct FlagsRequest {
    pub scope: Scope,
    pub provider: Option<String>,
    pub stale_days: Option<u32>,
}

pub fn run_flags(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &FlagsRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let config = flags_config(config, request.provider.as_deref(), request.stale_days);
    run_analyzer::<analyzers::flags::Analyzer>(path, &config, format, Some(&request.scope), out)
        .map(drop)
}

pub struct ChurnRequest {
    pub scope: Scope,
    pub window: WindowRequest,
}

pub fn run_churn(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &ChurnRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let days = if let Some(since) = &request.window.since {
        since_flag_days(since)?
    } else {
        configured_churn_days(config)
    };
    let scope = &request.scope;
    let file_set = scoped_file_set(path, config, scope)?;
    let ctx = build_context(path, &file_set, config);
    let result = churn_analyzer(days).analyze(&ctx)?;
    let mut value = serde_json::to_value(&result)?;
    super::test_rows::mark(&mut value, "churn", &ctx, scope.exclude_tests);
    let mut out = out;
    format_with_limits(
        value,
        format,
        scope.top.or(Some(DEFAULT_TOP)),
        scope.offset,
        &mut out,
    )
}

pub fn run_changes(
    path: &PathBuf,
    config: &Config,
    format: Format,
    request: &WindowedRequest,
    out: &mut dyn Write,
) -> Result<()> {
    let days = changes_window(request.window.since.as_deref(), config)?;
    let scope = &request.scope;
    let file_set = scoped_file_set(path, config, scope)?;
    let ctx = build_context(path, &file_set, config);
    let result = changes_analyzer(days).analyze(&ctx)?;
    let mut out = out;
    format_with_limits(
        serde_json::to_value(&result)?,
        format,
        scope.top,
        scope.offset,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    /// The one-argument constructor of 5.0 stays until 6.0.
    #[test]
    #[allow(deprecated)]
    fn test_defect_analyzer_keeps_the_signature_of_5_0() {
        let constructor: fn(u32) -> analyzers::defect::Analyzer = defect_analyzer;
        let _ = constructor(30);
        let _ = defect_analyzer_with_config(&Config::default(), 30);
    }

    #[test]
    fn test_plain_analyzer_writes_its_report() {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run_plain(
            Plain::Smells,
            &path,
            &Config::default(),
            Format::Json,
            &Scope::default(),
            &mut out,
        )
        .expect("smells");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert!(value["smells"].is_array());
    }

    #[test]
    fn test_satd_command_writes_the_items() {
        let (directory, path) = repository();
        std::fs::write(directory.path().join("debt.py"), "# TODO: fix this\n").expect("write");
        let mut out = Vec::new();
        run_satd(
            &path,
            &Config::default(),
            Format::Json,
            &Scope::default(),
            &mut out,
        )
        .expect("satd");
        let value: serde_json::Value = serde_json::from_slice(&out).expect("json");
        assert_eq!(value["items"].as_array().expect("items").len(), 1);
    }

    #[test]
    fn test_flags_config_applies_the_options() {
        let config = flags_config(&Config::default(), Some("unleash"), Some(7));
        assert_eq!(config.feature_flags.providers, ["unleash"]);
        assert_eq!(config.feature_flags.stale_days, 7);
    }

    #[test]
    fn test_window_uses_since_over_the_configuration_and_rejects_garbage() {
        assert_eq!(churn_window(Some("30d"), 90).unwrap(), 30);
        assert_eq!(churn_window(None, 90).unwrap(), 90);
        assert!(churn_window(Some("garbage"), 90).is_err());
    }

    #[test]
    fn test_changes_window_uses_since_over_the_configuration_and_rejects_all() {
        let config = Config::default();
        assert_eq!(changes_window(None, &config).unwrap(), 30);
        assert_eq!(changes_window(Some("3m"), &config).unwrap(), 90);
        let error = changes_window(Some("all"), &config)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a window of `changes`"), "{error}");
        assert!(changes_window(Some("garbage"), &config).is_err());
    }

    #[test]
    fn test_since_flag_days_rejects_text_that_is_not_a_window() {
        assert!(since_flag_days("garbage").is_err());
        assert_eq!(since_flag_days("30d").unwrap(), 30);
    }
}
