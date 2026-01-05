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
    /// Custom providers.
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
}
