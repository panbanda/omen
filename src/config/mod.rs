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
#[serde(default)]
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
    /// Score thresholds.
    pub score: ScoreConfig,
    /// Feature flag configuration.
    pub feature_flags: FeatureFlagsConfig,
    /// Temporal coupling configuration.
    pub temporal: TemporalConfig,
    /// Output configuration.
    pub output: OutputConfig,
    /// Exclude built/minified assets (e.g. *.min.js) from analysis.
    pub exclude_built_assets: bool,
    /// Changes/JIT analyzer configuration.
    pub changes: ChangesConfig,
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
            score: ScoreConfig::default(),
            feature_flags: FeatureFlagsConfig::default(),
            temporal: TemporalConfig::default(),
            output: OutputConfig::default(),
            exclude_built_assets: true,
            changes: ChangesConfig::default(),
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
        let config: Self = Figment::from(Serialized::defaults(Self::default()))
            .merge(Toml::file_exact(path))
            .merge(Env::prefixed("OMEN_").split("__"))
            .extract()
            .map_err(|e| crate::core::Error::Config(e.to_string()))?;
        Ok(config)
    }

    /// Alias for from_file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_file(path)
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
        Ok(config)
    }

    /// Alias for load_default.
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        Self::load_default(dir)
    }

    /// Create default config file content.
    pub fn default_toml() -> &'static str {
        include_str!("default_config.toml")
    }
}

/// Complexity analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ComplexityConfig {
    /// Maximum cyclomatic complexity before warning.
    pub cyclomatic_warn: u32,
    /// Maximum cyclomatic complexity before error.
    pub cyclomatic_error: u32,
    /// Maximum cognitive complexity before warning.
    pub cognitive_warn: u32,
    /// Maximum cognitive complexity before error.
    pub cognitive_error: u32,
    /// Maximum nesting depth.
    pub max_nesting: u32,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            cyclomatic_warn: 10,
            cyclomatic_error: 20,
            cognitive_warn: 15,
            cognitive_error: 30,
            max_nesting: 5,
        }
    }
}

/// SATD analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SatdConfig {
    /// Categories to detect.
    pub categories: Vec<String>,
    /// Custom markers to detect.
    pub custom_markers: Vec<String>,
}

impl Default for SatdConfig {
    fn default() -> Self {
        Self {
            categories: vec![
                "design".to_string(),
                "defect".to_string(),
                "requirement".to_string(),
                "test".to_string(),
                "performance".to_string(),
                "security".to_string(),
            ],
            custom_markers: Vec::new(),
        }
    }
}

/// Churn analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChurnConfig {
    /// Time period to analyze (e.g., "6m", "1y").
    pub since: String,
    /// Number of top files to report.
    pub top: usize,
}

impl Default for ChurnConfig {
    fn default() -> Self {
        Self {
            since: "6m".to_string(),
            top: 20,
        }
    }
}

/// Clone detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotspotConfig {
    /// Number of top hotspots to report.
    pub top: usize,
}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self { top: 20 }
    }
}

/// Score configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ScoreConfig {
    /// Minimum overall score to pass.
    pub fail_under: Option<f64>,
    /// Component thresholds.
    pub thresholds: ScoreThresholds,
}

/// Score component thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ScoreThresholds {
    pub complexity: Option<f64>,
    pub duplication: Option<f64>,
    pub satd: Option<f64>,
    pub tdg: Option<f64>,
    pub coupling: Option<f64>,
    pub smells: Option<f64>,
    pub cohesion: Option<f64>,
}

/// Feature flag detection configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureFlagsConfig {
    /// Days before a flag is considered stale.
    pub stale_days: u32,
    /// Built-in providers to enable (e.g., "launchdarkly", "flipper", "split", "unleash", "generic", "env").
    /// If empty, no built-in providers are used; you must explicitly list providers to enable them.
    pub providers: Vec<String>,
    /// Custom providers defined via tree-sitter queries.
    pub custom_providers: Vec<CustomProvider>,
}

/// Custom feature flag provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Default output format.
    pub format: OutputFormat,
    /// Color output.
    pub color: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
            color: true,
        }
    }
}

/// Changes/JIT analyzer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChangesConfig {
    /// Number of days of history to analyze.
    pub days: u32,
}

impl Default for ChangesConfig {
    fn default() -> Self {
        Self { days: 30 }
    }
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
mod tests {
    use super::*;
    use figment::Jail;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.complexity.cyclomatic_warn, 10);
        assert_eq!(config.churn.top, 20);
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
            jail.create_file(
                "omen.toml",
                "[complexity]\ncyclomatic_warn = 15\ncyclomatic_error = 25",
            )?;
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.complexity.cyclomatic_warn, 15);
            assert_eq!(config.complexity.cyclomatic_error, 25);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_alias() {
        Jail::expect_with(|jail| {
            jail.create_file("test.toml", "[churn]\ntop = 50")?;
            let config = Config::load("test.toml").unwrap();
            assert_eq!(config.churn.top, 50);
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
            jail.create_file(".omen/omen.toml", "[hotspot]\ntop = 30")?;
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.hotspot.top, 30);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_default_no_file() {
        Jail::expect_with(|_jail| {
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.complexity.cyclomatic_warn, 10);
            Ok(())
        });
    }

    #[test]
    fn test_config_load_from_dir_alias() {
        Jail::expect_with(|_jail| {
            let config = Config::load_from_dir(".").unwrap();
            assert_eq!(config.churn.since, "6m");
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
            jail.create_file("omen.toml", "[complexity]\ncyclomatic_warn = 15")?;
            jail.set_env("OMEN_COMPLEXITY__CYCLOMATIC_WARN", "5");
            let config = Config::from_file("omen.toml").unwrap();
            assert_eq!(config.complexity.cyclomatic_warn, 5);
            Ok(())
        });
    }

    #[test]
    fn test_env_var_overrides_default_no_file() {
        Jail::expect_with(|jail| {
            jail.set_env("OMEN_COMPLEXITY__CYCLOMATIC_WARN", "42");
            let config = Config::load_default(".").unwrap();
            assert_eq!(config.complexity.cyclomatic_warn, 42);
            Ok(())
        });
    }

    #[test]
    fn test_config_default_toml() {
        let content = Config::default_toml();
        assert!(!content.is_empty());
    }

    #[test]
    fn test_complexity_config_default() {
        let config = ComplexityConfig::default();
        assert_eq!(config.cyclomatic_warn, 10);
        assert_eq!(config.cyclomatic_error, 20);
        assert_eq!(config.cognitive_warn, 15);
        assert_eq!(config.cognitive_error, 30);
        assert_eq!(config.max_nesting, 5);
    }

    #[test]
    fn test_satd_config_default() {
        let config = SatdConfig::default();
        assert!(config.categories.contains(&"design".to_string()));
        assert!(config.categories.contains(&"defect".to_string()));
        assert!(config.custom_markers.is_empty());
    }

    #[test]
    fn test_churn_config_default() {
        let config = ChurnConfig::default();
        assert_eq!(config.since, "6m");
        assert_eq!(config.top, 20);
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
        assert_eq!(config.top, 20);
    }

    #[test]
    fn test_score_config_default() {
        let config = ScoreConfig::default();
        assert!(config.fail_under.is_none());
    }

    #[test]
    fn test_output_config_default() {
        let config = OutputConfig::default();
        assert_eq!(config.format, OutputFormat::Text);
        assert!(config.color);
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
        assert!(json.contains("cyclomatic_warn"));
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
    fn test_feature_flags_config_default() {
        let config = FeatureFlagsConfig::default();
        assert_eq!(config.stale_days, 0);
        assert!(config.providers.is_empty());
        assert!(config.custom_providers.is_empty());
    }
}
