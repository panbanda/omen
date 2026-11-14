//! Configuration loading and management.

use std::path::Path;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

use crate::core::Result;

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Exclude patterns (glob).
    #[serde(rename = "exclude")]
    pub exclude_patterns: Vec<String>,
    /// Complexity thresholds.
    pub complexity: ComplexityConfig,
    /// SATD configuration.
    pub satd: SatdConfig,
    /// Churn configuration.
    pub churn: ChurnConfig,
    /// Clone detection configuration.
    pub duplicates: DuplicatesConfig,
    /// Hotspot configuration.
    pub hotspot: HotspotConfig,
    /// Defect analyzer configuration.
    pub defect: DefectConfig,
    /// Score thresholds.
    pub score: ScoreConfig,
    /// Feature flag configuration.
    pub feature_flags: FeatureFlagsConfig,
    /// Temporal coupling configuration.
    pub temporal: TemporalConfig,
    /// Exclude built/minified assets (e.g. *.min.js) from analysis.
    pub exclude_built_assets: bool,
    /// Changes/JIT analyzer configuration.
    pub changes: ChangesConfig,
    /// Stubs configuration.
    pub stubs: StubsConfig,
    /// Mutation testing configuration.
    pub mutation: MutationConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            exclude_patterns: Vec::new(),
            complexity: ComplexityConfig::default(),
            satd: SatdConfig::default(),
            churn: ChurnConfig::default(),
            duplicates: DuplicatesConfig::default(),
            hotspot: HotspotConfig::default(),
            defect: DefectConfig::default(),
            score: ScoreConfig::default(),
            feature_flags: FeatureFlagsConfig::default(),
            temporal: TemporalConfig::default(),
            exclude_built_assets: true,
            changes: ChangesConfig::default(),
            stubs: StubsConfig::default(),
            mutation: MutationConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from an explicit file path.
    ///
    /// Errors if the file does not exist. Use this for explicit `--config` flags.
    /// Env vars with `OMEN_` prefix override file values.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(crate::core::Error::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        let figment = Figment::from(Serialized::defaults(Self::default()));
        let extension = path.extension().and_then(|value| value.to_str());
        if matches!(extension, Some("yaml" | "yml" | "json")) {
            return Err(crate::core::Error::Config(format!(
                "{}: the configuration file format is TOML only",
                path.display()
            )));
        }
        let config: Self = figment
            .merge(Toml::file_exact(path))
            .merge(Env::prefixed("OMEN_").split("__"))
            .extract()
            .map_err(|e| crate::core::Error::Config(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from directory, looking for omen.toml or .omen/omen.toml.
    ///
    /// Missing files are silently skipped (defaults are used).
    /// Env vars with `OMEN_` prefix override file/default values.
    pub fn load_default(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let config: Self = Figment::from(Serialized::defaults(Self::default()))
            .merge(Toml::file(dir.join("omen.toml")))
            .merge(Toml::file(dir.join(".omen/omen.toml")))
            .merge(Env::prefixed("OMEN_").split("__"))
            .extract()
            .map_err(|e| crate::core::Error::Config(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Reject configuration that cannot mean anything, rather than silently
    /// substituting a value the user did not ask for.
    pub fn validate(&self) -> Result<()> {
        if self.changes.since.is_all() {
            return Err(crate::core::Error::Config(
                "changes.since: \"all\" is not a window of `changes`. Use a duration such as 30d."
                    .to_string(),
            ));
        }
        Ok(())
    }
}

/// A history window: a duration such as `30d`, `3m`, or `1y`, or the value
/// `all`. The `--since` flag of the commands uses the same syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HistoryWindow {
    text: String,
    days: u32,
}

impl HistoryWindow {
    /// Parse a window. A value that is empty, not a window, or 0 days is an
    /// error, and it never means all the history.
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        let days = crate::git::since_to_days(text)?;
        Ok(Self {
            text: text.to_string(),
            days,
        })
    }

    fn built_in(text: &str) -> Self {
        Self::parse(text).expect("the built-in window is valid")
    }

    /// The number of days of the window. `u32::MAX` is all the history.
    pub fn days(&self) -> u32 {
        self.days
    }

    /// True if the window is all the history.
    pub fn is_all(&self) -> bool {
        self.days == u32::MAX
    }
}

impl TryFrom<String> for HistoryWindow {
    type Error = String;

    fn try_from(text: String) -> std::result::Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<HistoryWindow> for String {
    fn from(window: HistoryWindow) -> Self {
        window.text
    }
}

/// Complexity analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ComplexityConfig {
    /// Gate mode of `complexity`.
    pub gate: GateMode,
    /// Maximum cyclomatic complexity of a function.
    pub cyclomatic_max: u32,
    /// Maximum cognitive complexity of a function.
    pub cognitive_max: u32,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            gate: GateMode::Off,
            cyclomatic_max: 20,
            cognitive_max: 30,
        }
    }
}

/// SATD analyzer configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SatdConfig {
    /// Markers to detect in addition to the built-in markers.
    pub markers: Vec<String>,
}

/// Churn analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChurnConfig {
    /// History window to analyze, such as `6m`, `1y`, or `all`.
    pub since: HistoryWindow,
}

impl Default for ChurnConfig {
    fn default() -> Self {
        Self {
            since: HistoryWindow::built_in("6m"),
        }
    }
}

/// Clone detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DuplicatesConfig {
    /// Minimum token count for clone detection.
    pub min_tokens: usize,
    /// Minimum similarity threshold (0.0-1.0).
    pub min_similarity: f64,
    /// Exclude test files and Rust `#[cfg(test)]` code from duplication
    /// detection. Overridden by `omen clones --include-tests`.
    pub exclude_tests: bool,
}

impl Default for DuplicatesConfig {
    fn default() -> Self {
        Self {
            min_tokens: 50,
            min_similarity: 0.9,
            exclude_tests: true,
        }
    }
}

/// Hotspot analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotspotConfig {
    /// History window to measure churn over, such as `90d` or `all`. The
    /// window ends at the date of the analyzed revision.
    pub since: HistoryWindow,
}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            since: HistoryWindow::built_in("90d"),
        }
    }
}

/// Defect analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DefectConfig {
    /// History window to measure churn over, such as `30d` or `all`. The
    /// window ends at the date of the analyzed revision.
    pub since: HistoryWindow,
}

impl Default for DefectConfig {
    fn default() -> Self {
        Self {
            since: HistoryWindow::built_in("30d"),
        }
    }
}

/// Score configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScoreConfig {
    /// Gate mode of `score`.
    pub gate: GateMode,
    /// Minimum overall score that the gate accepts.
    pub min: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            gate: GateMode::Off,
            min: 80.0,
        }
    }
}

/// Stubs configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StubsConfig {
    /// Gate mode of `stubs`.
    pub gate: GateMode,
}

/// Mutation testing configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MutationConfig {
    /// Gate mode of `mutation`.
    pub gate: GateMode,
}

/// Feature flag detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeatureFlagsConfig {
    /// Days before a flag is considered stale.
    pub stale_days: u32,
    /// Built-in providers to enable (e.g., "launchdarkly", "flipper", "split", "unleash", "generic", "env").
    /// If empty, no built-in providers are used; you must explicitly list providers to enable them.
    pub providers: Vec<String>,
    /// Custom providers defined via tree-sitter queries.
    pub custom_providers: Vec<CustomProvider>,
}

impl Default for FeatureFlagsConfig {
    fn default() -> Self {
        Self {
            stale_days: 90,
            providers: Vec::new(),
            custom_providers: Vec::new(),
        }
    }
}

/// Custom feature flag provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomProvider {
    /// Provider name.
    pub name: String,
    /// Supported languages.
    pub languages: Vec<String>,
    /// Tree-sitter query.
    pub query: String,
}

/// Output configuration.
/// Temporal coupling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TemporalConfig {
    /// Exclude test files from coupling analysis.
    pub exclude_tests: bool,
}

impl Default for TemporalConfig {
    fn default() -> Self {
        Self {
            exclude_tests: true,
        }
    }
}

/// Changes/JIT analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChangesConfig {
    /// History window to analyze, such as `30d`. The value `all` is not valid.
    pub since: HistoryWindow,
}

impl Default for ChangesConfig {
    fn default() -> Self {
        Self {
            since: HistoryWindow::built_in("30d"),
        }
    }
}

/// What a gated command does when it finds a violation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum GateMode {
    /// Give the report only
    #[default]
    Off,
    /// Give the report and write a warning to stderr
    Warn,
    /// Give the report and exit with status 2
    Error,
}

/// Output format.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable text.
    #[default]
    Text,
    /// JSON format.
    Json,
    /// Markdown format.
    Markdown,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" | "txt" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "md" | "markdown" => Ok(Self::Markdown),
            _ => Err(format!("Unknown format: {s}. Use 'text', 'json', or 'md'")),
        }
    }
}

#[cfg(test)]
// The `figment::Jail` closure must return the large `figment::Error`.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use figment::Jail;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.complexity.cyclomatic_max, 20);
        assert_eq!(config.churn.since.days(), 180);
        assert!(config.exclude_built_assets);
    }

    #[test]
    fn test_exclude_built_assets_default_true() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert!(config.exclude_built_assets);
            Ok(())
        });
    }

    #[test]
    fn test_exclude_built_assets_opt_out() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "exclude_built_assets = false")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert!(!config.exclude_built_assets);
            Ok(())
        });
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!(
            "markdown".parse::<OutputFormat>().unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!(
            "md".parse::<OutputFormat>().unwrap(),
            OutputFormat::Markdown
        );
        assert!("unknown".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_output_format_text() {
        assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!("txt".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!("TEXT".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
    }

    #[test]
    fn test_config_from_file() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[complexity]\ncyclomatic_max = 25")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.complexity.cyclomatic_max, 25);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_default_omen_toml() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[duplicates]\nmin_tokens = 100")?;
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.duplicates.min_tokens, 100);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_default_dot_omen() {
        Jail::expect_with(|jail| {
            std::fs::create_dir(jail.directory().join(".omen")).unwrap();
            jail.create_file(".omen/omen.toml", "[hotspot]\nsince = \"30d\"")?;
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.hotspot.since.days(), 30);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_default_no_file() {
        Jail::expect_with(|_jail| {
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.complexity.cyclomatic_max, 20);
            Ok(())
        });
    }

    #[test]
    fn test_from_file_errors_on_missing_file() {
        let result = Config::from_file("/nonexistent/path/omen.toml");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"), "expected 'not found' in: {err}");
    }

    #[test]
    fn test_env_var_overrides_file_value() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[complexity]\ncyclomatic_max = 15")?;
            jail.set_env("OMEN_COMPLEXITY__CYCLOMATIC_MAX", "5");
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.complexity.cyclomatic_max, 5);
            Ok(())
        });
    }

    #[test]
    fn test_env_var_overrides_default_no_file() {
        Jail::expect_with(|jail| {
            jail.set_env("OMEN_COMPLEXITY__CYCLOMATIC_MAX", "42");
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.complexity.cyclomatic_max, 42);
            Ok(())
        });
    }

    #[test]
    fn test_complexity_config_default() {
        let config = ComplexityConfig::default();
        assert_eq!(config.cyclomatic_max, 20);
        assert_eq!(config.cognitive_max, 30);
    }

    #[test]
    fn test_satd_config_default() {
        let config = SatdConfig::default();
        assert!(config.markers.is_empty());
    }

    #[test]
    fn test_churn_config_default() {
        let config = ChurnConfig::default();
        assert_eq!(config.since.days(), 180);
    }

    #[test]
    fn test_duplicates_config_default() {
        let config = DuplicatesConfig::default();
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.min_similarity, 0.9);
        assert!(config.exclude_tests);
    }

    #[test]
    fn test_hotspot_config_default() {
        let config = HotspotConfig::default();
        assert_eq!(config.since.days(), 90);
    }

    #[test]
    fn test_score_config_default() {
        let config = ScoreConfig::default();
        assert_eq!(config.min, 80.0);
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Text);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("complexity"));
        assert!(json.contains("cyclomatic_max"));
    }

    #[test]
    fn test_config_with_exclude_patterns() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "omen.toml",
                "exclude = [\"target/**\", \"node_modules/**\"]",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.exclude_patterns.len(), 2);
            assert!(config.exclude_patterns.contains(&"target/**".to_string()));
            Ok(())
        });
    }

    #[test]
    fn test_unknown_config_key_is_rejected_with_key_name() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[churn]\nunknown_window = 3")?;
            let error = Config::from_file("omen.toml").unwrap_err().to_string();
            assert!(error.contains("unknown_window"), "{error}");
            Ok(())
        });
    }

    #[test]
    fn test_a_config_file_that_is_not_toml_is_an_error_that_says_toml_only() {
        Jail::expect_with(|jail| {
            for name in ["omen.yaml", "omen.yml", "omen.json"] {
                jail.create_file(name, "")?;
                let error = Config::from_file(name).unwrap_err().to_string();
                assert!(error.contains("TOML only"), "{name}: {error}");
            }
            Ok(())
        });
    }

    #[test]
    fn test_each_gated_command_table_has_a_gate_mode() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "omen.toml",
                "[complexity]\ngate = \"error\"\n[score]\ngate = \"warn\"\n[stubs]\ngate = \"error\"\n[mutation]\ngate = \"warn\"\n",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.complexity.gate, GateMode::Error);
            assert_eq!(config.score.gate, GateMode::Warn);
            assert_eq!(config.stubs.gate, GateMode::Error);
            assert_eq!(config.mutation.gate, GateMode::Warn);
            Ok(())
        });
    }

    #[test]
    fn test_renamed_keys_of_the_tables_load() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "omen.toml",
                "[score]\nmin = 90.5\n[satd]\nmarkers = [\"REVISIT\"]\n[feature_flags]\nstale_days = 30\n",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.score.min, 90.5);
            assert_eq!(config.satd.markers, ["REVISIT"]);
            assert_eq!(config.feature_flags.stale_days, 30);
            Ok(())
        });
    }

    #[test]
    fn test_feature_flags_config_default() {
        let config = FeatureFlagsConfig::default();
        assert_eq!(config.stale_days, 90);
        assert!(config.providers.is_empty());
        assert!(config.custom_providers.is_empty());
    }

    #[test]
    fn test_window_defaults() {
        let config = Config::default();
        assert_eq!(config.churn.since.days(), 180);
        assert_eq!(config.hotspot.since.days(), 90);
        assert_eq!(config.defect.since.days(), 30);
        assert_eq!(config.changes.since.days(), 30);
    }

    #[test]
    fn test_each_window_key_accepts_the_units_of_the_since_flag() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "omen.toml",
                "[churn]\nsince = \"1y\"\n[hotspot]\nsince = \"2w\"\n[defect]\nsince = \"10d\"\n[changes]\nsince = \"3m\"\n",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.churn.since.days(), 365);
            assert_eq!(config.hotspot.since.days(), 14);
            assert_eq!(config.defect.since.days(), 10);
            assert_eq!(config.changes.since.days(), 90);
            Ok(())
        });
    }

    #[test]
    fn test_all_is_a_window_where_the_since_flag_accepts_it() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "omen.toml",
                "[churn]\nsince = \"all\"\n[hotspot]\nsince = \"all\"\n[defect]\nsince = \"all\"\n",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.churn.since.days(), u32::MAX);
            assert_eq!(config.hotspot.since.days(), u32::MAX);
            assert_eq!(config.defect.since.days(), u32::MAX);
            Ok(())
        });
    }

    #[test]
    fn test_all_is_not_a_window_of_changes() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[changes]\nsince = \"all\"\n")?;
            let error = Config::from_file("omen.toml").unwrap_err().to_string();
            assert!(error.contains("changes.since"), "{error}");
            Ok(())
        });
    }

    #[test]
    fn test_a_window_that_is_empty_or_not_a_window_is_rejected_with_its_key() {
        Jail::expect_with(|jail| {
            for table in ["churn", "hotspot", "defect", "changes"] {
                for value in ["0d", "nonsense", "", "2024-01-01"] {
                    jail.create_file("omen.toml", &format!("[{table}]\nsince = \"{value}\"\n"))?;
                    let error = Config::from_file("omen.toml").unwrap_err().to_string();
                    assert!(
                        error.contains(&format!("{table}.since")),
                        "{table} {value:?}: {error}"
                    );
                    assert!(
                        error.contains(&format!("{value:?}")),
                        "{table} {value:?}: {error}"
                    );
                }
            }
            Ok(())
        });
    }

    #[test]
    fn test_env_var_overrides_a_nested_window() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[hotspot]\nsince = \"30d\"\n")?;
            jail.set_env("OMEN_HOTSPOT__SINCE", "1y");
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.hotspot.since.days(), 365);
            Ok(())
        });
    }
}
