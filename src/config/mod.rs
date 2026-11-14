//! Configuration loading and management.

use std::path::Path;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

use crate::core::Result;

/// The keys that 5.0 removed or renamed, each with the text that gives the
/// replacement. A table comes after its keys, thus the error names the key.
const REMOVED_KEYS: &[(&str, &str)] = &[
    (
        "exclude_built_assets",
        "use include_generated, which has the opposite meaning",
    ),
    ("duplicates.exclude_tests", "use exclude_tests"),
    ("temporal.exclude_tests", "use exclude_tests"),
    ("temporal", "use exclude_tests"),
    ("gate.complexity", "use complexity.gate"),
    ("gate.score", "use score.gate"),
    ("gate.stubs", "use stubs.gate"),
    ("gate.mutation", "use mutation.gate"),
    ("gate", "use the gate key in the table of each command"),
    (
        "complexity.cyclomatic_error",
        "use complexity.cyclomatic_max",
    ),
    ("complexity.cognitive_error", "use complexity.cognitive_max"),
    ("satd.custom_markers", "use satd.markers"),
    ("churn.top", "use the --top flag"),
    ("hotspot.top", "use the --top flag"),
    ("hotspot.days", "use hotspot.since, for example \"90d\""),
    ("defect.churn_days", "use defect.since, for example \"30d\""),
    ("changes.days", "use changes.since, for example \"30d\""),
    ("score.fail_under", "use score.min"),
];

/// Main configuration structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// If true, analyze the generated and minified files that omen skips by default.
    pub include_generated: bool,
    /// If true, `clones` and `temporal` skip test files.
    pub exclude_tests: bool,
    /// Changes/JIT analyzer configuration.
    pub changes: ChangesConfig,
    /// Stubs configuration.
    pub stubs: StubsConfig,
    /// Mutation testing configuration.
    pub mutation: MutationConfig,
    /// Security findings: the options of the gate and the rows of the user.
    pub security: SecurityConfig,
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
            include_generated: false,
            exclude_tests: true,
            changes: ChangesConfig::default(),
            stubs: StubsConfig::default(),
            mutation: MutationConfig::default(),
            security: SecurityConfig::default(),
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
        Self::extract(
            figment
                .merge(Toml::file_exact(path))
                .merge(Env::prefixed("OMEN_").split("__")),
        )
    }

    /// Load configuration from directory, looking for omen.toml or .omen/omen.toml.
    ///
    /// Missing files are silently skipped (defaults are used).
    /// Env vars with `OMEN_` prefix override file/default values.
    pub fn load_default(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        Self::extract(
            Figment::from(Serialized::defaults(Self::default()))
                .merge(Toml::file(dir.join("omen.toml")))
                .merge(Toml::file(dir.join(".omen/omen.toml")))
                .merge(Env::prefixed("OMEN_").split("__")),
        )
    }

    /// Read the configuration from the merged sources. A key that 5.0 removed
    /// gives an error that names the replacement, not only the unknown-field
    /// error of serde.
    fn extract(figment: Figment) -> Result<Self> {
        for (key, replacement) in REMOVED_KEYS {
            if figment.contains(key) {
                return Err(crate::core::Error::Config(format!(
                    "{key} was removed in 5.0; {replacement}"
                )));
            }
        }
        let config: Self = figment
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
        if self.feature_flags.stale_days == 0 {
            return Err(crate::core::Error::Config(
                "feature_flags.stale_days = 0: use 1 or more. A value of 0 makes each flag stale."
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SatdConfig {
    /// Markers to detect in addition to the built-in markers.
    pub markers: Vec<String>,
}

/// Churn analyzer configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DuplicatesConfig {
    /// Minimum token count for clone detection.
    pub min_tokens: usize,
    /// Minimum similarity threshold (0.0-1.0).
    pub min_similarity: f64,
}

impl Default for DuplicatesConfig {
    fn default() -> Self {
        Self {
            min_tokens: 50,
            min_similarity: 0.9,
        }
    }
}

/// Hotspot analyzer configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StubsConfig {
    /// Gate mode of `stubs`.
    pub gate: GateMode,
}

/// Mutation testing configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MutationConfig {
    /// Gate mode of `mutation`.
    pub gate: GateMode,
}

/// Feature flag detection configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomProvider {
    /// Provider name.
    pub name: String,
    /// Supported languages.
    pub languages: Vec<String>,
    /// Tree-sitter query.
    pub query: String,
}

/// Changes/JIT analyzer configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The options of `omen security`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SecurityConfig {
    /// Gate mode of `omen security`. The gate counts the sinks that the change adds.
    /// The `--gate` flag overrides it.
    pub gate: GateMode,
    /// An edge of a path has this resolution basis or a stronger one. The name is
    /// a resolution basis, for example `same_package` or `imported_name`.
    pub min_basis: String,
    /// The sink categories that the gate counts. An empty list means each category.
    pub gate_categories: Vec<String>,
    /// The gate counts a sink with this basis or a stronger one: `name_only`,
    /// `builtin`, or `imported_symbol`.
    pub gate_basis: String,
    /// More sinks, for example the wrappers of a company.
    pub sinks: Vec<SecuritySinkConfig>,
    /// More entry points, for example the handlers of an in-house framework.
    pub entry_points: Vec<SecurityEntryConfig>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            gate: GateMode::Off,
            min_basis: "same_package".to_string(),
            gate_categories: Vec::new(),
            gate_basis: "builtin".to_string(),
            sinks: Vec::new(),
            entry_points: Vec::new(),
        }
    }
}

/// A sink that the user adds: a call of `callee` in `module`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecuritySinkConfig {
    /// A sink category, for example `command_execution`.
    pub category: String,
    /// The languages of the rule, for example `["python"]`.
    pub languages: Vec<String>,
    /// The module of the callee, as an import path. An empty text means a name that the
    /// language gives without an import.
    #[serde(default)]
    pub module: String,
    /// The name of the callee. `|` separates alternatives.
    pub callee: String,
}

/// An entry point that the user adds: each function with one of the names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecurityEntryConfig {
    /// `http_handler`, `cli_main`, `message_consumer`, `scheduled_job`,
    /// `serverless_handler`, or `ffi_export`.
    pub kind: String,
    /// The name of the framework in the output.
    pub framework: String,
    pub languages: Vec<String>,
    /// The names of the functions.
    pub symbols: Vec<String>,
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
        assert!(!config.include_generated);
        assert!(config.exclude_tests);
    }

    #[test]
    fn test_include_generated_default_false() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert!(!config.include_generated);
            Ok(())
        });
    }

    #[test]
    fn test_include_generated_opt_in() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "include_generated = true")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert!(config.include_generated);
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
    fn test_exclude_tests_is_one_top_level_key() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "exclude_tests = false")?;
            let config = Config::from_file("omen.toml").unwrap();
            assert!(!config.exclude_tests);
            Ok(())
        });
    }

    #[test]
    fn test_env_var_overrides_a_top_level_key() {
        Jail::expect_with(|jail| {
            jail.set_env("OMEN_EXCLUDE_TESTS", "false");
            let config = Config::load_default(".").unwrap();
            assert!(!config.exclude_tests);
            Ok(())
        });
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
    fn test_stale_days_of_0_is_rejected_with_its_key() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[feature_flags]\nstale_days = 0\n")?;
            let error = Config::from_file("omen.toml").unwrap_err().to_string();
            assert!(error.contains("feature_flags.stale_days"), "{error}");
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

    /// Each key that 5.0 removed or renamed, the file text that sets it, and
    /// the text that the error must contain.
    const REMOVED_KEY_CASES: &[(&str, &str, &str)] = &[
        (
            "exclude_built_assets = false",
            "exclude_built_assets",
            "use include_generated",
        ),
        (
            "[duplicates]\nexclude_tests = false",
            "duplicates.exclude_tests",
            "use exclude_tests",
        ),
        (
            "[temporal]\nexclude_tests = false",
            "temporal.exclude_tests",
            "use exclude_tests",
        ),
        (
            "[gate]\ncomplexity = \"error\"",
            "gate.complexity",
            "use complexity.gate",
        ),
        ("[gate]\nscore = \"error\"", "gate.score", "use score.gate"),
        ("[gate]\nstubs = \"error\"", "gate.stubs", "use stubs.gate"),
        (
            "[gate]\nmutation = \"error\"",
            "gate.mutation",
            "use mutation.gate",
        ),
        (
            "[complexity]\ncyclomatic_error = 25",
            "complexity.cyclomatic_error",
            "use complexity.cyclomatic_max",
        ),
        (
            "[complexity]\ncognitive_error = 25",
            "complexity.cognitive_error",
            "use complexity.cognitive_max",
        ),
        (
            "[satd]\ncustom_markers = [\"X\"]",
            "satd.custom_markers",
            "use satd.markers",
        ),
        ("[churn]\ntop = 5", "churn.top", "use the --top flag"),
        ("[hotspot]\ntop = 5", "hotspot.top", "use the --top flag"),
        ("[hotspot]\ndays = 5", "hotspot.days", "use hotspot.since"),
        (
            "[defect]\nchurn_days = 5",
            "defect.churn_days",
            "use defect.since",
        ),
        ("[changes]\ndays = 5", "changes.days", "use changes.since"),
        (
            "[score]\nfail_under = 70.0",
            "score.fail_under",
            "use score.min",
        ),
    ];

    #[test]
    fn test_a_removed_key_gives_an_error_that_names_the_new_key() {
        Jail::expect_with(|jail| {
            for (text, old, replacement) in REMOVED_KEY_CASES {
                jail.create_file("omen.toml", text)?;
                let message = format!("{old} was removed in 5.0");
                for error in [
                    Config::from_file("omen.toml").unwrap_err().to_string(),
                    Config::load_default(".").unwrap_err().to_string(),
                ] {
                    assert!(error.contains(&message), "{old}: {error}");
                    assert!(error.contains(replacement), "{old}: {error}");
                }
            }
            Ok(())
        });
    }

    #[test]
    fn test_a_removed_key_in_the_environment_gives_the_same_error() {
        Jail::expect_with(|jail| {
            jail.set_env("OMEN_GATE__COMPLEXITY", "error");
            let error = Config::load_default(".").unwrap_err().to_string();
            assert!(
                error.contains("gate.complexity was removed in 5.0; use complexity.gate"),
                "{error}"
            );
            Ok(())
        });
    }

    #[test]
    fn test_a_key_that_never_existed_keeps_the_unknown_field_error() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", "[churn]\nunknown_window = 3")?;
            let error = Config::from_file("omen.toml").unwrap_err().to_string();
            assert!(!error.contains("was removed"), "{error}");
            Ok(())
        });
    }
    const EXAMPLE: &str = include_str!("../../omen.example.toml");

    fn leaf_paths(prefix: &str, value: &serde_json::Value, paths: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, inner) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    leaf_paths(&path, inner, paths);
                }
            }
            _ => paths.push(prefix.to_string()),
        }
    }

    #[test]
    fn test_the_example_file_equals_the_default_configuration() {
        Jail::expect_with(|jail| {
            jail.create_file("omen.toml", EXAMPLE)?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config, Config::default());
            Ok(())
        });
    }

    #[test]
    fn test_each_key_of_the_configuration_is_in_the_example_file() {
        let example: serde_json::Value = Figment::from(Toml::string(EXAMPLE)).extract().unwrap();
        let defaults = serde_json::to_value(Config::default()).unwrap();
        let (mut in_example, mut in_defaults) = (Vec::new(), Vec::new());
        leaf_paths("", &example, &mut in_example);
        leaf_paths("", &defaults, &mut in_defaults);
        in_example.sort();
        in_defaults.sort();
        assert_eq!(in_example, in_defaults);
    }
}
