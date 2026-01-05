//! Feature flag detection analyzer.
//!
//! Detects feature flags from common providers and assesses staleness based on
//! git history. Supports LaunchDarkly, Flipper, Split, and generic patterns.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::git::GitRepo;

/// Feature flags analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Expected time-to-live for flags in days.
    pub expected_ttl_days: u32,
    /// Providers to detect (empty = all).
    pub providers: Vec<String>,
    /// Include git history for staleness detection.
    pub include_git: bool,
    /// File spread threshold for high complexity.
    pub file_spread_warning: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            expected_ttl_days: 14,
            providers: Vec::new(),
            include_git: true,
            file_spread_warning: 4,
        }
    }
}

/// Feature flags analyzer.
pub struct Analyzer {
    config: Config,
    patterns: Vec<FlagPattern>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            patterns: build_patterns(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            patterns: build_patterns(),
        }
    }

    pub fn with_expected_ttl(mut self, days: u32) -> Self {
        self.config.expected_ttl_days = days;
        self
    }

    pub fn with_providers(mut self, providers: Vec<String>) -> Self {
        self.config.providers = providers;
        self
    }

    pub fn with_git_history(mut self, include: bool) -> Self {
        self.config.include_git = include;
        self
    }

    /// Analyze a repository for feature flags.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let mut references: Vec<FlagReference> = Vec::new();

        // Collect all flag references
        for entry in WalkBuilder::new(repo_path)
            .hidden(true)
            .git_ignore(true)
            .build()
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Skip non-source files
            if Language::detect(path).is_none() {
                continue;
            }

            // Read file content
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Apply patterns
            for pattern in &self.patterns {
                // Filter by provider if configured
                if !self.config.providers.is_empty()
                    && !self.config.providers.contains(&pattern.provider)
                {
                    continue;
                }

                for cap in pattern.regex.captures_iter(&content) {
                    if let Some(key_match) = cap.name("key") {
                        let key = key_match.as_str().trim_matches(|c| c == '"' || c == '\'' || c == ':');

                        // Calculate line number
                        let line = content[..key_match.start()]
                            .chars()
                            .filter(|&c| c == '\n')
                            .count() as u32
                            + 1;

                        references.push(FlagReference {
                            file: rel_path.clone(),
                            line,
                            key: key.to_string(),
                            provider: pattern.provider.clone(),
                        });
                    }
                }
            }
        }

        // Group by flag key
        let mut flags_map: HashMap<String, Vec<FlagReference>> = HashMap::new();
        for reference in references {
            flags_map
                .entry(reference.key.clone())
                .or_default()
                .push(reference);
        }

        // Build flag analysis
        let mut flags: Vec<FeatureFlag> = Vec::new();

        // Try to open git repo for staleness detection
        let git_repo = if self.config.include_git {
            GitRepo::open(repo_path).ok()
        } else {
            None
        };

        for (key, refs) in flags_map {
            let provider = refs.first().map(|r| r.provider.clone()).unwrap_or_default();
            let file_spread = refs.iter().map(|r| &r.file).collect::<std::collections::HashSet<_>>().len();

            // Calculate staleness from git if available
            let (first_seen, last_seen, age_days, stale) = if let Some(ref repo) = git_repo {
                calculate_staleness(repo, &refs, self.config.expected_ttl_days)
            } else {
                (None, None, 0, false)
            };

            let references: Vec<FlagReferenceOutput> = refs
                .iter()
                .map(|r| FlagReferenceOutput {
                    file: r.file.clone(),
                    line: r.line,
                })
                .collect();

            flags.push(FeatureFlag {
                key,
                provider,
                references,
                first_seen,
                last_seen,
                age_days,
                stale,
                file_spread,
            });
        }

        // Sort by staleness (stale first, then by age)
        flags.sort_by(|a, b| {
            match (a.stale, b.stale) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.age_days.cmp(&a.age_days),
            }
        });

        let summary = calculate_summary(&flags);

        Ok(Analysis {
            generated_at: Utc::now().to_rfc3339(),
            flags,
            stale_count: summary.stale_flags,
            summary,
        })
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "flags"
    }

    fn description(&self) -> &'static str {
        "Find feature flags and assess staleness"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        self.analyze_repo(ctx.root)
    }
}

/// Calculate staleness from git history.
fn calculate_staleness(
    repo: &GitRepo,
    refs: &[FlagReference],
    expected_ttl_days: u32,
) -> (Option<String>, Option<String>, u32, bool) {
    let mut first_seen: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut last_seen: Option<chrono::DateTime<chrono::Utc>> = None;

    // Check git log for files containing this flag
    for reference in refs {
        let path = std::path::PathBuf::from(&reference.file);
        let paths = [path];
        if let Ok(commits) = repo.log(None, Some(&paths)) {
            for commit in commits {
                let commit_time = chrono::TimeZone::timestamp_opt(&Utc, commit.timestamp, 0)
                    .single()
                    .unwrap_or_else(Utc::now);

                if first_seen.is_none() || commit_time < first_seen.unwrap() {
                    first_seen = Some(commit_time);
                }
                if last_seen.is_none() || commit_time > last_seen.unwrap() {
                    last_seen = Some(commit_time);
                }
            }
        }
    }

    let age_days = first_seen
        .map(|fs| (Utc::now() - fs).num_days().max(0) as u32)
        .unwrap_or(0);

    let stale = age_days > expected_ttl_days;

    (
        first_seen.map(|t| t.to_rfc3339()),
        last_seen.map(|t| t.to_rfc3339()),
        age_days,
        stale,
    )
}

/// Calculate summary statistics.
fn calculate_summary(flags: &[FeatureFlag]) -> AnalysisSummary {
    let mut by_provider: HashMap<String, usize> = HashMap::new();
    let mut stale_flags = 0;

    for flag in flags {
        *by_provider.entry(flag.provider.clone()).or_insert(0) += 1;
        if flag.stale {
            stale_flags += 1;
        }
    }

    AnalysisSummary {
        total_flags: flags.len(),
        stale_flags,
        by_provider,
    }
}

/// A pattern for detecting feature flags.
struct FlagPattern {
    provider: String,
    regex: Regex,
}

/// Build detection patterns for common providers.
fn build_patterns() -> Vec<FlagPattern> {
    let mut patterns = Vec::new();

    // LaunchDarkly patterns
    if let Ok(re) = Regex::new(r#"(?:variation|boolVariation|stringVariation|intVariation|floatVariation|jsonVariation)\s*\(\s*["'](?P<key>[^"']+)["']"#) {
        patterns.push(FlagPattern {
            provider: "launchdarkly".to_string(),
            regex: re,
        });
    }

    // Flipper (Ruby) patterns
    if let Ok(re) = Regex::new(r#"Flipper(?:\[|\.enabled\?\s*\(\s*):?(?P<key>[a-zA-Z_][a-zA-Z0-9_]*)"#) {
        patterns.push(FlagPattern {
            provider: "flipper".to_string(),
            regex: re,
        });
    }

    // Split patterns
    if let Ok(re) = Regex::new(r#"(?:getTreatment|get_treatment)\s*\([^,]*,\s*["'](?P<key>[^"']+)["']"#) {
        patterns.push(FlagPattern {
            provider: "split".to_string(),
            regex: re,
        });
    }

    // Unleash patterns
    if let Ok(re) = Regex::new(r#"(?:isEnabled|is_enabled)\s*\(\s*["'](?P<key>[^"']+)["']"#) {
        patterns.push(FlagPattern {
            provider: "unleash".to_string(),
            regex: re,
        });
    }

    // Generic feature flag patterns
    if let Ok(re) = Regex::new(r#"(?i)(?:feature_flag|featureFlag|is_feature_enabled|isFeatureEnabled|feature_enabled\?|check_feature)\s*\(?["':]+(?P<key>[a-zA-Z_][a-zA-Z0-9_-]*)["']?\)?"#) {
        patterns.push(FlagPattern {
            provider: "generic".to_string(),
            regex: re,
        });
    }

    // ENV-based feature flags
    if let Ok(re) = Regex::new(r#"(?:ENV|process\.env|os\.environ)\s*\[?\s*["'](?P<key>FEATURE_[A-Z_]+)["']"#) {
        patterns.push(FlagPattern {
            provider: "env".to_string(),
            regex: re,
        });
    }

    patterns
}

/// Internal flag reference during collection.
struct FlagReference {
    file: String,
    line: u32,
    key: String,
    provider: String,
}

/// Feature flag analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: String,
    pub flags: Vec<FeatureFlag>,
    pub stale_count: usize,
    pub summary: AnalysisSummary,
}

/// A detected feature flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub key: String,
    pub provider: String,
    pub references: Vec<FlagReferenceOutput>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub age_days: u32,
    pub stale: bool,
    pub file_spread: usize,
}

/// A reference to a flag in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagReferenceOutput {
    pub file: String,
    pub line: u32,
}

/// Summary statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_flags: usize,
    pub stale_flags: usize,
    pub by_provider: HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.config.expected_ttl_days, 14);
        assert!(analyzer.config.providers.is_empty());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.expected_ttl_days, 14);
        assert!(config.include_git);
        assert_eq!(config.file_spread_warning, 4);
    }

    #[test]
    fn test_analyzer_with_expected_ttl() {
        let analyzer = Analyzer::new().with_expected_ttl(30);
        assert_eq!(analyzer.config.expected_ttl_days, 30);
    }

    #[test]
    fn test_analyzer_with_providers() {
        let analyzer = Analyzer::new().with_providers(vec!["launchdarkly".to_string()]);
        assert_eq!(analyzer.config.providers.len(), 1);
        assert_eq!(analyzer.config.providers[0], "launchdarkly");
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "flags");
        assert!(analyzer.description().contains("feature flags"));
    }

    #[test]
    fn test_build_patterns() {
        let patterns = build_patterns();
        assert!(!patterns.is_empty());

        let providers: Vec<&str> = patterns.iter().map(|p| p.provider.as_str()).collect();
        assert!(providers.contains(&"launchdarkly"));
        assert!(providers.contains(&"flipper"));
        assert!(providers.contains(&"split"));
    }

    #[test]
    fn test_launchdarkly_pattern() {
        let patterns = build_patterns();
        let ld_pattern = patterns.iter().find(|p| p.provider == "launchdarkly").unwrap();

        let test_cases = vec![
            (r#"client.boolVariation("my-flag", user, false)"#, Some("my-flag")),
            (r#"variation('feature-x', ctx, true)"#, Some("feature-x")),
            (r#"stringVariation("test_flag", user, "")"#, Some("test_flag")),
            (r#"something else"#, None),
        ];

        for (input, expected) in test_cases {
            let cap = ld_pattern.regex.captures(input);
            match expected {
                Some(key) => {
                    assert!(cap.is_some(), "Expected match for: {}", input);
                    let key_match = cap.unwrap().name("key").unwrap().as_str();
                    assert_eq!(key_match, key);
                }
                None => {
                    assert!(cap.is_none(), "Expected no match for: {}", input);
                }
            }
        }
    }

    #[test]
    fn test_flipper_pattern() {
        let patterns = build_patterns();
        let flipper_pattern = patterns.iter().find(|p| p.provider == "flipper").unwrap();

        let test_cases = vec![
            (r#"Flipper.enabled?(:my_feature)"#, Some("my_feature")),
            (r#"Flipper[:feature_x]"#, Some("feature_x")),
        ];

        for (input, expected) in test_cases {
            let cap = flipper_pattern.regex.captures(input);
            match expected {
                Some(key) => {
                    assert!(cap.is_some(), "Expected match for: {}", input);
                    let key_match = cap.unwrap().name("key").unwrap().as_str();
                    assert_eq!(key_match, key);
                }
                None => {
                    assert!(cap.is_none(), "Expected no match for: {}", input);
                }
            }
        }
    }

    #[test]
    fn test_calculate_summary_empty() {
        let summary = calculate_summary(&[]);
        assert_eq!(summary.total_flags, 0);
        assert_eq!(summary.stale_flags, 0);
        assert!(summary.by_provider.is_empty());
    }

    #[test]
    fn test_calculate_summary_with_flags() {
        let flags = vec![
            FeatureFlag {
                key: "flag1".to_string(),
                provider: "launchdarkly".to_string(),
                references: vec![],
                first_seen: None,
                last_seen: None,
                age_days: 10,
                stale: false,
                file_spread: 1,
            },
            FeatureFlag {
                key: "flag2".to_string(),
                provider: "launchdarkly".to_string(),
                references: vec![],
                first_seen: None,
                last_seen: None,
                age_days: 30,
                stale: true,
                file_spread: 2,
            },
            FeatureFlag {
                key: "flag3".to_string(),
                provider: "flipper".to_string(),
                references: vec![],
                first_seen: None,
                last_seen: None,
                age_days: 5,
                stale: false,
                file_spread: 1,
            },
        ];

        let summary = calculate_summary(&flags);
        assert_eq!(summary.total_flags, 3);
        assert_eq!(summary.stale_flags, 1);
        assert_eq!(summary.by_provider.get("launchdarkly"), Some(&2));
        assert_eq!(summary.by_provider.get("flipper"), Some(&1));
    }

    #[test]
    fn test_feature_flag_serialization() {
        let flag = FeatureFlag {
            key: "test-flag".to_string(),
            provider: "launchdarkly".to_string(),
            references: vec![
                FlagReferenceOutput {
                    file: "src/main.rs".to_string(),
                    line: 10,
                },
            ],
            first_seen: Some("2024-01-01T00:00:00Z".to_string()),
            last_seen: Some("2024-01-15T00:00:00Z".to_string()),
            age_days: 15,
            stale: true,
            file_spread: 1,
        };

        let json = serde_json::to_string(&flag).unwrap();
        assert!(json.contains("\"test-flag\""));
        assert!(json.contains("\"launchdarkly\""));
        assert!(json.contains("\"stale\":true"));

        let parsed: FeatureFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.key, "test-flag");
        assert!(parsed.stale);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            flags: vec![],
            stale_count: 0,
            summary: AnalysisSummary::default(),
        };

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("generated_at"));

        let parsed: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.flags.len(), 0);
    }

    #[test]
    fn test_env_pattern() {
        let patterns = build_patterns();
        let env_pattern = patterns.iter().find(|p| p.provider == "env").unwrap();

        let test_cases = vec![
            (r#"ENV["FEATURE_NEW_UI"]"#, Some("FEATURE_NEW_UI")),
            (r#"process.env['FEATURE_DARK_MODE']"#, Some("FEATURE_DARK_MODE")),
        ];

        for (input, expected) in test_cases {
            let cap = env_pattern.regex.captures(input);
            match expected {
                Some(key) => {
                    assert!(cap.is_some(), "Expected match for: {}", input);
                    let key_match = cap.unwrap().name("key").unwrap().as_str();
                    assert_eq!(key_match, key);
                }
                None => {
                    assert!(cap.is_none(), "Expected no match for: {}", input);
                }
            }
        }
    }
}
