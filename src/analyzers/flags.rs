//! Feature flag detection analyzer.
//!
//! Detects feature flags from common providers and assesses staleness based on
//! git history. Supports LaunchDarkly, Flipper, Split, generic patterns, and
//! custom providers defined via tree-sitter queries in the configuration.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

use crate::config::CustomProvider;
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::git::GitRepo;
use crate::parser::{get_tree_sitter_language, Parser};

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
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self { config }
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

    /// Analyze a repository for feature flags using only the analyzer's internal config.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        self.analyze_with_config(
            repo_path,
            self.config.expected_ttl_days,
            &self.config.providers,
            &[],
        )
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
        let expected_ttl_days = if ctx.config.feature_flags.stale_days > 0 {
            ctx.config.feature_flags.stale_days
        } else {
            self.config.expected_ttl_days
        };

        let providers = if !ctx.config.feature_flags.providers.is_empty() {
            &ctx.config.feature_flags.providers
        } else {
            &self.config.providers
        };

        self.analyze_with_config(
            ctx.root,
            expected_ttl_days,
            providers,
            &ctx.config.feature_flags.custom_providers,
        )
    }
}

/// Built-in provider definition using tree-sitter queries.
struct BuiltinProvider {
    name: &'static str,
    languages: &'static [Language],
    query: &'static str,
}

/// Get built-in providers with tree-sitter queries.
fn get_builtin_providers() -> Vec<BuiltinProvider> {
    vec![
        // Flipper (Ruby) - element reference: Flipper[:flag] or Flipper['flag']
        BuiltinProvider {
            name: "flipper",
            languages: &[Language::Ruby],
            query: r#"
                ; Flipper[:symbol] or Flipper["string"]
                (element_reference
                    object: (constant) @_receiver
                    (simple_symbol) @key
                    (#eq? @_receiver "Flipper"))

                (element_reference
                    object: (constant) @_receiver
                    (string (string_content) @key)
                    (#eq? @_receiver "Flipper"))

                ; Flipper.enable(:symbol) and similar method calls
                (call
                    receiver: (constant) @_receiver
                    method: (identifier) @_method
                    arguments: (argument_list
                        (simple_symbol) @key)
                    (#eq? @_receiver "Flipper")
                    (#match? @_method "^(enabled\\?|enable|disable|add|remove|exist\\?)$"))

                ; Flipper.enable("string") and similar method calls
                (call
                    receiver: (constant) @_receiver
                    method: (identifier) @_method
                    arguments: (argument_list
                        (string (string_content) @key))
                    (#eq? @_receiver "Flipper")
                    (#match? @_method "^(enabled\\?|enable|disable|add|remove|exist\\?)$"))
            "#,
        },
        // LaunchDarkly (JavaScript/TypeScript)
        BuiltinProvider {
            name: "launchdarkly",
            languages: &[
                Language::JavaScript,
                Language::TypeScript,
                Language::Tsx,
                Language::Jsx,
            ],
            query: r#"
                ; client.variation("flag-key", ...)
                (call_expression
                    function: (member_expression
                        property: (property_identifier) @_method)
                    arguments: (arguments
                        (string (string_fragment) @key))
                    (#match? @_method "^(variation|boolVariation|stringVariation|intVariation|floatVariation|jsonVariation)$"))
            "#,
        },
        // Split (JavaScript/TypeScript)
        BuiltinProvider {
            name: "split",
            languages: &[
                Language::JavaScript,
                Language::TypeScript,
                Language::Tsx,
                Language::Jsx,
            ],
            query: r#"
                ; client.getTreatment(user, "flag-key")
                (call_expression
                    function: (member_expression
                        property: (property_identifier) @_method)
                    arguments: (arguments
                        (_)
                        (string (string_fragment) @key))
                    (#match? @_method "^(getTreatment|getTreatments)$"))
            "#,
        },
        // Unleash (JavaScript/TypeScript)
        BuiltinProvider {
            name: "unleash",
            languages: &[
                Language::JavaScript,
                Language::TypeScript,
                Language::Tsx,
                Language::Jsx,
            ],
            query: r#"
                ; client.isEnabled("flag-key")
                (call_expression
                    function: (member_expression
                        property: (property_identifier) @_method)
                    arguments: (arguments
                        (string (string_fragment) @key))
                    (#eq? @_method "isEnabled"))
            "#,
        },
        // Unleash (Python)
        BuiltinProvider {
            name: "unleash",
            languages: &[Language::Python],
            query: r#"
                ; client.is_enabled("flag-key")
                (call
                    function: (attribute
                        attribute: (identifier) @_method)
                    arguments: (argument_list
                        (string (string_content) @key))
                    (#eq? @_method "is_enabled"))
            "#,
        },
        // ENV-based feature flags (Ruby)
        BuiltinProvider {
            name: "env",
            languages: &[Language::Ruby],
            query: r#"
                ; ENV["FEATURE_X"] or ENV['FEATURE_X']
                (element_reference
                    object: (constant) @_receiver
                    (string (string_content) @key)
                    (#eq? @_receiver "ENV")
                    (#match? @key "^FEATURE_"))
            "#,
        },
        // ENV-based feature flags (JavaScript/TypeScript)
        BuiltinProvider {
            name: "env",
            languages: &[
                Language::JavaScript,
                Language::TypeScript,
                Language::Tsx,
                Language::Jsx,
            ],
            query: r#"
                ; process.env.FEATURE_X or process.env["FEATURE_X"]
                (member_expression
                    object: (member_expression
                        object: (identifier) @_process
                        property: (property_identifier) @_env)
                    property: (property_identifier) @key
                    (#eq? @_process "process")
                    (#eq? @_env "env")
                    (#match? @key "^FEATURE_"))

                (subscript_expression
                    object: (member_expression
                        object: (identifier) @_process
                        property: (property_identifier) @_env)
                    index: (string (string_fragment) @key)
                    (#eq? @_process "process")
                    (#eq? @_env "env")
                    (#match? @key "^FEATURE_"))
            "#,
        },
        // ENV-based feature flags (Python)
        BuiltinProvider {
            name: "env",
            languages: &[Language::Python],
            query: r#"
                ; os.environ["FEATURE_X"] or os.environ.get("FEATURE_X")
                (subscript
                    value: (attribute
                        object: (identifier) @_os
                        attribute: (identifier) @_environ)
                    subscript: (string (string_content) @key)
                    (#eq? @_os "os")
                    (#eq? @_environ "environ")
                    (#match? @key "^FEATURE_"))

                (call
                    function: (attribute
                        object: (attribute
                            object: (identifier) @_os
                            attribute: (identifier) @_environ)
                        attribute: (identifier) @_get)
                    arguments: (argument_list
                        (string (string_content) @key))
                    (#eq? @_os "os")
                    (#eq? @_environ "environ")
                    (#eq? @_get "get")
                    (#match? @key "^FEATURE_"))
            "#,
        },
    ]
}

impl Analyzer {
    /// Analyze a repository with explicit config parameters.
    ///
    /// Built-in providers only run if explicitly listed in `providers`.
    /// If `providers` is empty, no built-in detection runs (only custom providers).
    fn analyze_with_config(
        &self,
        repo_path: &Path,
        expected_ttl_days: u32,
        providers: &[String],
        custom_providers: &[CustomProvider],
    ) -> Result<Analysis> {
        let mut references: Vec<FlagReference> = Vec::new();
        let parser = Parser::new();
        let builtin_providers = get_builtin_providers();

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

            let language = match Language::detect(path) {
                Some(lang) => lang,
                None => continue,
            };

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Get tree-sitter language for parsing
            let ts_lang = match get_tree_sitter_language(language) {
                Ok(lang) => lang,
                Err(_) => continue,
            };

            // Parse the file once
            let parse_result = match parser.parse(content.as_bytes(), language, path) {
                Ok(result) => result,
                Err(_) => continue,
            };

            // Apply built-in providers using tree-sitter queries
            for builtin in &builtin_providers {
                // Check if this provider is enabled
                if !providers.contains(&builtin.name.to_string()) {
                    continue;
                }

                // Check if this language is supported
                if !builtin.languages.contains(&language) {
                    continue;
                }

                // Compile and run the query
                if let Ok(query) = Query::new(&ts_lang, builtin.query) {
                    let mut cursor = QueryCursor::new();
                    let mut matches =
                        cursor.matches(&query, parse_result.tree.root_node(), content.as_bytes());

                    // Find the capture index for "key"
                    let key_capture_idx = query.capture_names().iter().position(|n| *n == "key");

                    while let Some(query_match) = matches.next() {
                        for capture in query_match.captures {
                            if let Some(idx) = key_capture_idx {
                                if capture.index as usize != idx {
                                    continue;
                                }
                            }

                            let node = capture.node;
                            let key = node
                                .utf8_text(content.as_bytes())
                                .unwrap_or("")
                                .trim_matches(|c| c == '"' || c == '\'' || c == ':');

                            if !key.is_empty() {
                                references.push(FlagReference {
                                    file: rel_path.clone(),
                                    line: node.start_position().row as u32 + 1,
                                    key: key.to_string(),
                                    provider: builtin.name.to_string(),
                                });
                            }
                        }
                    }
                }
            }

            // Apply custom providers using tree-sitter queries
            for custom in custom_providers {
                // Check if this language is supported by this provider
                let lang_name = format!("{:?}", language).to_lowercase();
                if !custom.languages.is_empty()
                    && !custom
                        .languages
                        .iter()
                        .any(|l| l.to_lowercase() == lang_name)
                {
                    continue;
                }

                // Compile and run the query
                if let Ok(query) = Query::new(&ts_lang, &custom.query) {
                    let mut cursor = QueryCursor::new();
                    let mut matches =
                        cursor.matches(&query, parse_result.tree.root_node(), content.as_bytes());

                    // Find the capture index for "flag_key" or "key"
                    let key_capture_idx = query
                        .capture_names()
                        .iter()
                        .position(|n| *n == "flag_key" || *n == "key");

                    while let Some(query_match) = matches.next() {
                        for capture in query_match.captures {
                            if let Some(idx) = key_capture_idx {
                                if capture.index as usize != idx {
                                    continue;
                                }
                            }

                            let node = capture.node;
                            let key = node
                                .utf8_text(content.as_bytes())
                                .unwrap_or("")
                                .trim_matches(|c| c == '"' || c == '\'' || c == ':');

                            if !key.is_empty() {
                                references.push(FlagReference {
                                    file: rel_path.clone(),
                                    line: node.start_position().row as u32 + 1,
                                    key: key.to_string(),
                                    provider: custom.name.clone(),
                                });
                            }
                        }
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
            let file_spread = refs
                .iter()
                .map(|r| &r.file)
                .collect::<std::collections::HashSet<_>>()
                .len();

            // Calculate staleness from git if available
            let (first_seen, last_seen, age_days, stale) = if let Some(ref repo) = git_repo {
                calculate_staleness(repo, &refs, expected_ttl_days)
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
        flags.sort_by(|a, b| match (a.stale, b.stale) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.age_days.cmp(&a.age_days),
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
    fn test_builtin_providers_exist() {
        let providers = get_builtin_providers();
        assert!(!providers.is_empty());

        let names: Vec<&str> = providers.iter().map(|p| p.name).collect();
        assert!(names.contains(&"flipper"));
        assert!(names.contains(&"launchdarkly"));
        assert!(names.contains(&"split"));
        assert!(names.contains(&"unleash"));
        assert!(names.contains(&"env"));
    }

    #[test]
    fn test_flipper_symbol_detection() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, "Flipper[:my_feature]").unwrap();

        let analyzer = Analyzer::new();
        let providers = vec!["flipper".to_string()];

        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &providers, &[])
            .unwrap();

        assert_eq!(result.flags.len(), 1);
        assert_eq!(result.flags[0].key, "my_feature");
        assert_eq!(result.flags[0].provider, "flipper");
    }

    #[test]
    fn test_flipper_string_detection() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, r#"Flipper["my_feature"]"#).unwrap();

        let analyzer = Analyzer::new();
        let providers = vec!["flipper".to_string()];

        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &providers, &[])
            .unwrap();

        assert_eq!(result.flags.len(), 1);
        assert_eq!(result.flags[0].key, "my_feature");
    }

    #[test]
    fn test_flipper_enable_detection() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, "Flipper.enable(:test_flag)").unwrap();

        let analyzer = Analyzer::new();
        let providers = vec!["flipper".to_string()];

        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &providers, &[])
            .unwrap();

        assert_eq!(result.flags.len(), 1);
        assert_eq!(result.flags[0].key, "test_flag");
    }

    #[test]
    fn test_flipper_enabled_detection() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, "Flipper.enabled?(:check_flag)").unwrap();

        let analyzer = Analyzer::new();
        let providers = vec!["flipper".to_string()];

        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &providers, &[])
            .unwrap();

        assert_eq!(result.flags.len(), 1);
        assert_eq!(result.flags[0].key, "check_flag");
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
            references: vec![FlagReferenceOutput {
                file: "src/main.rs".to_string(),
                line: 10,
            }],
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
    fn test_empty_providers_no_builtin_detection() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, "Flipper[:ld_flag]").unwrap();

        let analyzer = Analyzer::new();

        // Empty providers means no built-in patterns run
        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &[], &[])
            .unwrap();

        assert_eq!(result.flags.len(), 0);
    }

    #[test]
    fn test_explicit_provider_enabling() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");
        std::fs::write(&file_path, "Flipper[:flipper_flag]").unwrap();

        let analyzer = Analyzer::new();
        let providers = vec!["flipper".to_string()];

        // With flipper explicitly enabled, it should find the flag
        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &providers, &[])
            .unwrap();

        assert_eq!(result.flags.len(), 1);
        assert_eq!(result.flags[0].provider, "flipper");
    }

    #[test]
    fn test_custom_provider_with_treesitter_query() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rb");

        // Ruby code with custom feature flag pattern
        std::fs::write(
            &file_path,
            r#"
class FlagService
  def enabled?(flag_name)
    MyFeature.enabled?("custom_feature_x")
  end
end
"#,
        )
        .unwrap();

        let custom_provider = CustomProvider {
            name: "my_feature_system".to_string(),
            languages: vec!["ruby".to_string()],
            // Tree-sitter query to find MyFeature.enabled? calls
            query: r#"
                (call
                    receiver: (constant) @receiver
                    method: (identifier) @method
                    arguments: (argument_list
                        (string (string_content) @key)))
                (#eq? @receiver "MyFeature")
                (#eq? @method "enabled?")
            "#
            .to_string(),
        };

        let analyzer = Analyzer::new();
        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &[], &[custom_provider])
            .unwrap();

        // Should find the custom feature flag
        let custom_flags: Vec<_> = result
            .flags
            .iter()
            .filter(|f| f.provider == "my_feature_system")
            .collect();
        assert_eq!(custom_flags.len(), 1);
        assert_eq!(custom_flags[0].key, "custom_feature_x");
    }

    #[test]
    fn test_custom_provider_language_filtering() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        // Create a Python file
        let py_file = temp_dir.path().join("test.py");
        std::fs::write(&py_file, r#"flag = is_enabled("python_flag")"#).unwrap();

        // Create a Ruby file
        let rb_file = temp_dir.path().join("test.rb");
        std::fs::write(&rb_file, r#"flag = is_enabled("ruby_flag")"#).unwrap();

        // Custom provider that only applies to Ruby
        let custom_provider = CustomProvider {
            name: "ruby_only".to_string(),
            languages: vec!["ruby".to_string()],
            query:
                r#"(call method: (identifier) @method arguments: (argument_list (string) @key))"#
                    .to_string(),
        };

        let analyzer = Analyzer::new();
        let result = analyzer
            .analyze_with_config(temp_dir.path(), 14, &[], &[custom_provider])
            .unwrap();

        // Should only find flags from Ruby files for this provider
        let ruby_only_flags: Vec<_> = result
            .flags
            .iter()
            .filter(|f| f.provider == "ruby_only")
            .collect();

        // The custom provider should only match the Ruby file
        for flag in &ruby_only_flags {
            assert!(
                flag.references.iter().all(|r| r.file.ends_with(".rb")),
                "Expected only Ruby files, got: {:?}",
                flag.references
            );
        }
    }
}
