//! Configuration loading and management.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::Result;

/// Main configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Exclude patterns (glob).
    #[serde(alias = "exclude")]
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
    /// Output configuration.
    pub output: OutputConfig,
}

impl Config {
    /// Load configuration from file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Alias for from_file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_file(path)
    }

    /// Load configuration from directory, looking for omen.toml or .omen/omen.toml.
    pub fn load_default(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();

        // Check omen.toml first
        let omen_toml = dir.join("omen.toml");
        if omen_toml.exists() {
            return Self::load(omen_toml);
        }

        // Check .omen/omen.toml
        let dot_omen_toml = dir.join(".omen/omen.toml");
        if dot_omen_toml.exists() {
            return Self::load(dot_omen_toml);
        }

        // Return default config if no file found
        Ok(Self::default())
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
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.complexity.cyclomatic_warn, 10);
        assert_eq!(config.churn.top, 20);
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
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("omen.toml");
        std::fs::write(
            &config_path,
            r#"
[complexity]
cyclomatic_warn = 15
cyclomatic_error = 25
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(config.complexity.cyclomatic_warn, 15);
        assert_eq!(config.complexity.cyclomatic_error, 25);
    }

    #[test]
    fn test_config_load_alias() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.toml");
        std::fs::write(&config_path, "[churn]\ntop = 50").unwrap();

        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.churn.top, 50);
    }

    #[test]
    fn test_config_load_default_omen_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("omen.toml");
        std::fs::write(&config_path, "[duplicates]\nmin_tokens = 100").unwrap();

        let config = Config::load_default(temp_dir.path()).unwrap();
        assert_eq!(config.duplicates.min_tokens, 100);
    }

    #[test]
    fn test_config_load_default_dot_omen() {
        let temp_dir = TempDir::new().unwrap();
        let dot_omen = temp_dir.path().join(".omen");
        std::fs::create_dir(&dot_omen).unwrap();
        std::fs::write(dot_omen.join("omen.toml"), "[hotspot]\ntop = 30").unwrap();

        let config = Config::load_default(temp_dir.path()).unwrap();
        assert_eq!(config.hotspot.top, 30);
    }

    #[test]
    fn test_config_load_default_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::load_default(temp_dir.path()).unwrap();
        // Should return default config
        assert_eq!(config.complexity.cyclomatic_warn, 10);
    }

    #[test]
    fn test_config_load_from_dir_alias() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::load_from_dir(temp_dir.path()).unwrap();
        assert_eq!(config.churn.since, "6m");
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
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("omen.toml");
        std::fs::write(
            &config_path,
            r#"
exclude = ["target/**", "node_modules/**"]
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(config.exclude_patterns.len(), 2);
        assert!(config.exclude_patterns.contains(&"target/**".to_string()));
    }

    #[test]
    fn test_feature_flags_config_default() {
        let config = FeatureFlagsConfig::default();
        assert_eq!(config.stale_days, 0);
        assert!(config.providers.is_empty());
        assert!(config.custom_providers.is_empty());
    }
}
