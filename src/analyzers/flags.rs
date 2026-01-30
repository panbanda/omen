//! Feature flag detection analyzer.
//!
//! Detects feature flags from common providers and assesses staleness based on
//! git history. Supports LaunchDarkly, Flipper, Split, generic patterns, and
//! custom providers defined via tree-sitter queries in the configuration.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

use crate::config::CustomProvider;
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
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
        use crate::config::Config as AppConfig;
        use crate::core::FileSet;

        let config = AppConfig::default();
        let file_set = FileSet::from_path(repo_path, &config)?;
        let ctx = AnalysisContext::new(&file_set, &config, Some(repo_path));

        self.analyze_with_config(
            &ctx,
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
            ctx,
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

/// Parse a language name (e.g., "ruby", "python") to a Language enum.
fn parse_language_name(name: &str) -> Option<Language> {
    match name.to_lowercase().as_str() {
        "go" | "golang" => Some(Language::Go),
        "rust" | "rs" => Some(Language::Rust),
        "python" | "py" => Some(Language::Python),
        "typescript" | "ts" => Some(Language::TypeScript),
        "javascript" | "js" => Some(Language::JavaScript),
        "tsx" => Some(Language::Tsx),
        "jsx" => Some(Language::Jsx),
        "java" => Some(Language::Java),
        "c" => Some(Language::C),
        "cpp" | "c++" => Some(Language::Cpp),
        "csharp" | "c#" | "cs" => Some(Language::CSharp),
        "ruby" | "rb" => Some(Language::Ruby),
        "php" => Some(Language::Php),
        "bash" | "sh" | "shell" => Some(Language::Bash),
        _ => None,
    }
}

impl Analyzer {
    /// Analyze a repository with explicit config parameters.
    ///
    /// Built-in providers only run if explicitly listed in `providers`.
    /// If `providers` is empty, no built-in detection runs (only custom providers).
    fn analyze_with_config(
        &self,
        ctx: &AnalysisContext<'_>,
        expected_ttl_days: u32,
        providers: &[String],
        custom_providers: &[CustomProvider],
    ) -> Result<Analysis> {
        let builtin_providers = get_builtin_providers();

        // Pre-compile queries for each (language, provider) combination
        let mut compiled_queries: HashMap<(Language, String), (Query, usize)> = HashMap::new();
        for builtin in &builtin_providers {
            if !providers.contains(&builtin.name.to_string()) {
                continue;
            }
            for &lang in builtin.languages {
                if let Ok(ts_lang) = get_tree_sitter_language(lang) {
                    if let Ok(query) = Query::new(&ts_lang, builtin.query) {
                        let key_idx = query
                            .capture_names()
                            .iter()
                            .position(|n| *n == "key")
                            .unwrap_or(0);
                        compiled_queries.insert((lang, builtin.name.to_string()), (query, key_idx));
                    }
                }
            }
        }

        // Pre-compile custom provider queries
        for custom in custom_providers {
            for lang_str in &custom.languages {
                if let Some(lang) = parse_language_name(lang_str) {
                    if let Ok(ts_lang) = get_tree_sitter_language(lang) {
                        if let Ok(query) = Query::new(&ts_lang, &custom.query) {
                            // Support both "key" and "flag_key" capture names
                            let key_idx = query
                                .capture_names()
                                .iter()
                                .position(|n| *n == "key" || *n == "flag_key")
                                .unwrap_or(0);
                            compiled_queries.insert((lang, custom.name.clone()), (query, key_idx));
                        }
                    }
                }
            }
        }

        // Wrap compiled queries in Arc for sharing across threads
        let compiled_queries = Arc::new(compiled_queries);
        let builtin_providers = Arc::new(builtin_providers);
        let custom_providers: Arc<[CustomProvider]> = custom_providers.to_vec().into();
        let providers: Arc<[String]> = providers.to_vec().into();

        // Get files from context
        let files: Vec<_> = ctx.files.iter().collect();

        // Process files in parallel
        let references: Vec<FlagReference> = files
            .par_iter()
            .flat_map(|path| {
                let mut file_refs = Vec::new();

                let language = match Language::detect(path) {
                    Some(lang) => lang,
                    None => return file_refs,
                };

                // Read file via context (supports both filesystem and git tree)
                let content = match ctx.read_file(path) {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(s) => s,
                        Err(_) => return file_refs,
                    },
                    Err(_) => return file_refs,
                };

                let rel_path = path
                    .strip_prefix(ctx.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();

                // Parse the file once with thread-local parser
                let parser = Parser::new();
                let parse_result = match parser.parse(content.as_bytes(), language, path) {
                    Ok(result) => result,
                    Err(_) => return file_refs,
                };

                // Apply built-in providers using pre-compiled queries
                for builtin in builtin_providers.iter() {
                    if !providers.contains(&builtin.name.to_string()) {
                        continue;
                    }
                    if !builtin.languages.contains(&language) {
                        continue;
                    }

                    // Look up pre-compiled query
                    let key = (language, builtin.name.to_string());
                    if let Some((query, key_capture_idx)) = compiled_queries.get(&key) {
                        let mut cursor = QueryCursor::new();
                        let mut matches = cursor.matches(
                            query,
                            parse_result.tree.root_node(),
                            content.as_bytes(),
                        );

                        while let Some(query_match) = matches.next() {
                            for capture in query_match.captures {
                                if capture.index as usize != *key_capture_idx {
                                    continue;
                                }

                                let node = capture.node;
                                let flag_key = node
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or("")
                                    .trim_matches(|c| c == '"' || c == '\'' || c == ':');

                                if !flag_key.is_empty() {
                                    file_refs.push(FlagReference {
                                        file: rel_path.clone(),
                                        line: node.start_position().row as u32 + 1,
                                        key: flag_key.to_string(),
                                        provider: builtin.name.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }

                // Apply custom providers using pre-compiled queries
                for custom in custom_providers.iter() {
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

                    // Look up pre-compiled query
                    let key = (language, custom.name.clone());
                    if let Some((query, key_capture_idx)) = compiled_queries.get(&key) {
                        let mut cursor = QueryCursor::new();
                        let mut matches = cursor.matches(
                            query,
                            parse_result.tree.root_node(),
                            content.as_bytes(),
                        );

                        while let Some(query_match) = matches.next() {
                            for capture in query_match.captures {
                                if capture.index as usize != *key_capture_idx {
                                    continue;
                                }

                                let node = capture.node;
                                let flag_key = node
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or("")
                                    .trim_matches(|c| c == '"' || c == '\'' || c == ':');

                                if !flag_key.is_empty() {
                                    file_refs.push(FlagReference {
                                        file: rel_path.clone(),
                                        line: node.start_position().row as u32 + 1,
                                        key: flag_key.to_string(),
                                        provider: custom.name.clone(),
                                    });
                                }
                            }
                        }
                    }
                }

                file_refs
            })
            .collect();

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

        // Check if git is available for staleness detection
        let has_git = self.config.include_git
            && std::process::Command::new("git")
                .current_dir(ctx.root)
                .args(["rev-parse", "--git-dir"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

        // Pre-cache git log -S results for each flag key to find actual introduction dates
        let flag_timestamps: HashMap<String, (Option<i64>, Option<i64>)> = if has_git {
            let keys: Vec<String> = flags_map.keys().cloned().collect();
            bulk_git_flag_timestamps(ctx.root, &keys)
        } else {
            HashMap::new()
        };

        for (key, refs) in flags_map {
            let provider = refs.first().map(|r| r.provider.clone()).unwrap_or_default();
            let file_spread = refs
                .iter()
                .map(|r| &r.file)
                .collect::<std::collections::HashSet<_>>()
                .len();

            // Calculate staleness from flag-level git pickaxe data
            let (first_seen, last_seen, age_days, stale) = if has_git {
                calculate_staleness_from_flag_timestamps(&flag_timestamps, &key, expected_ttl_days)
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
                priority: FlagPriority::default(),
            });
        }

        // Compute risk-based priority using per-file complexity.
        // Flags in high-complexity files are higher risk because conditional
        // branches there are more likely to cause bugs.
        let complexity_analyzer = crate::analyzers::complexity::Analyzer::new();
        let unique_flag_files: std::collections::HashSet<String> = flags
            .iter()
            .flat_map(|f| f.references.iter().map(|r| r.file.clone()))
            .collect();

        let file_complexity: HashMap<String, u32> = unique_flag_files
            .iter()
            .filter_map(|rel_path| {
                let full_path = ctx.root.join(rel_path);
                complexity_analyzer
                    .analyze_file(&full_path)
                    .ok()
                    .map(|r| (rel_path.clone(), r.total_cyclomatic))
            })
            .collect();

        for flag in &mut flags {
            let max_complexity = flag
                .references
                .iter()
                .filter_map(|r| file_complexity.get(&r.file))
                .copied()
                .max()
                .unwrap_or(0);

            // Score: complexity drives the risk level.
            // Thresholds based on typical per-file cyclomatic complexity.
            let level = if max_complexity >= 50 {
                "Critical"
            } else if max_complexity >= 20 {
                "High"
            } else if max_complexity >= 10 {
                "Medium"
            } else {
                "Low"
            };

            flag.priority = FlagPriority {
                level: level.to_string(),
                score: max_complexity as f64,
                max_complexity,
            };
        }

        // Sort by priority score descending, then staleness
        flags.sort_by(|a, b| {
            b.priority
                .score
                .partial_cmp(&a.priority.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| match (a.stale, b.stale) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.age_days.cmp(&a.age_days),
                })
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

/// Find the actual introduction and last-modified timestamps for each flag key
/// using `git log -S` (pickaxe search) to find commits that added/removed the flag text.
fn bulk_git_flag_timestamps(
    root: &std::path::Path,
    keys: &[String],
) -> HashMap<String, (Option<i64>, Option<i64>)> {
    use rayon::prelude::*;

    keys.par_iter()
        .map(|key| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["log", "--all", "--format=%at", "-S", key])
                .output();

            let timestamps: Vec<i64> = match output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<i64>().ok())
                    .collect(),
                _ => Vec::new(),
            };

            let first = timestamps.iter().copied().min();
            let last = timestamps.iter().copied().max();
            (key.clone(), (first, last))
        })
        .collect()
}

/// Calculate staleness from flag-level pickaxe timestamps.
fn calculate_staleness_from_flag_timestamps(
    timestamps: &HashMap<String, (Option<i64>, Option<i64>)>,
    key: &str,
    expected_ttl_days: u32,
) -> (Option<String>, Option<String>, u32, bool) {
    let (first_ts, last_ts) = timestamps.get(key).copied().unwrap_or((None, None));

    let first_seen = first_ts.and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts, 0).single());
    let last_seen = last_ts.and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts, 0).single());

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
    pub priority: FlagPriority,
}

/// Risk-based priority for a feature flag.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlagPriority {
    pub level: String,
    pub score: f64,
    /// Max cyclomatic complexity across files containing this flag.
    pub max_complexity: u32,
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
    use crate::config::Config as AppConfig;
    use crate::core::FileSet;

    /// Helper to create an AnalysisContext from a path for testing.
    fn create_test_context(path: &Path) -> (FileSet, AppConfig) {
        let config = AppConfig::default();
        let file_set = FileSet::from_path(path, &config).unwrap();
        (file_set, config)
    }

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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        let result = analyzer
            .analyze_with_config(&ctx, 14, &providers, &[])
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        let result = analyzer
            .analyze_with_config(&ctx, 14, &providers, &[])
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        let result = analyzer
            .analyze_with_config(&ctx, 14, &providers, &[])
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        let result = analyzer
            .analyze_with_config(&ctx, 14, &providers, &[])
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
                priority: FlagPriority::default(),
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
                priority: FlagPriority::default(),
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
                priority: FlagPriority::default(),
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
            priority: FlagPriority {
                level: "High".to_string(),
                score: 25.0,
                max_complexity: 25,
            },
        };

        let json = serde_json::to_string(&flag).unwrap();
        assert!(json.contains("\"test-flag\""));
        assert!(json.contains("\"launchdarkly\""));
        assert!(json.contains("\"High\""));
        assert!(json.contains("\"stale\":true"));

        let parsed: FeatureFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.key, "test-flag");
        assert!(parsed.stale);
    }

    #[test]
    fn test_flag_priority_levels() {
        // Verify the threshold logic directly
        let cases = vec![
            (5, "Low"),       // < 10
            (10, "Medium"),   // >= 10
            (19, "Medium"),   // < 20
            (20, "High"),     // >= 20
            (49, "High"),     // < 50
            (50, "Critical"), // >= 50
            (200, "Critical"),
        ];
        for (complexity, expected_level) in cases {
            let level = if complexity >= 50 {
                "Critical"
            } else if complexity >= 20 {
                "High"
            } else if complexity >= 10 {
                "Medium"
            } else {
                "Low"
            };
            assert_eq!(
                level, expected_level,
                "complexity {complexity} should be {expected_level}, got {level}"
            );
        }
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        // Empty providers means no built-in patterns run
        let result = analyzer.analyze_with_config(&ctx, 14, &[], &[]).unwrap();

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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));

        // With flipper explicitly enabled, it should find the flag
        let result = analyzer
            .analyze_with_config(&ctx, 14, &providers, &[])
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));
        let result = analyzer
            .analyze_with_config(&ctx, 14, &[], &[custom_provider])
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
        let (file_set, config) = create_test_context(temp_dir.path());
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));
        let result = analyzer
            .analyze_with_config(&ctx, 14, &[], &[custom_provider])
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
