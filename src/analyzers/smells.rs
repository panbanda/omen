//! Architectural smells analyzer.
//!
//! Detects architectural anti-patterns in dependency graphs:
//! - Cyclic dependencies (using Tarjan's SCC algorithm)
//! - Hub-like dependencies (excessive fan-in + fan-out)
//! - God components (high fan-in AND high fan-out)
//! - Unstable dependencies (stable components depending on unstable ones)
//!
//! Based on detection algorithms from Fontana et al. (2017) "Arcan".

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::parser::{extract_imports, Parser};

/// Detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    /// Fan-in + Fan-out threshold for hub detection.
    pub hub_threshold: usize,
    /// Minimum fan-in for god component.
    pub god_fan_in_threshold: usize,
    /// Minimum fan-out for god component.
    pub god_fan_out_threshold: usize,
    /// Max instability difference for unstable dependency.
    pub instability_difference: f64,
    /// I < this is considered stable.
    pub stable_threshold: f64,
    /// I > this is considered unstable.
    pub unstable_threshold: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            hub_threshold: 20,
            god_fan_in_threshold: 10,
            god_fan_out_threshold: 10,
            instability_difference: 0.4,
            stable_threshold: 0.3,
            unstable_threshold: 0.7,
        }
    }
}

/// Smells analyzer configuration.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub thresholds: Thresholds,
}

/// Smells analyzer.
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

    pub fn with_hub_threshold(mut self, threshold: usize) -> Self {
        self.config.thresholds.hub_threshold = threshold;
        self
    }

    pub fn with_god_thresholds(mut self, fan_in: usize, fan_out: usize) -> Self {
        self.config.thresholds.god_fan_in_threshold = fan_in;
        self.config.thresholds.god_fan_out_threshold = fan_out;
        self
    }

    pub fn with_instability_difference(mut self, diff: f64) -> Self {
        self.config.thresholds.instability_difference = diff;
        self
    }

    /// Analyze a repository for architectural smells.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let parser = Parser::new();

        // Build dependency graph
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();
        let mut file_imports: HashMap<String, Vec<String>> = HashMap::new();

        // Collect files and their imports
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

            // Detect language
            let _lang = match Language::detect(path) {
                Some(l) => l,
                None => continue,
            };

            // Parse file
            let parse_result = match parser.parse_file(path) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Get relative path as node ID
            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Create node if not exists
            let node_idx = *node_indices
                .entry(rel_path.clone())
                .or_insert_with(|| graph.add_node(rel_path.clone()));
            let _ = node_idx; // Silence unused warning

            // Extract imports
            let imports = extract_imports(&parse_result);
            let import_paths: Vec<String> = imports.into_iter().map(|imp| imp.path).collect();

            file_imports.insert(rel_path, import_paths);
        }

        // Add edges based on imports
        for (from_file, imports) in &file_imports {
            let from_idx = node_indices[from_file];

            for import in imports {
                // Try to resolve import to a file in the repo
                if let Some(&to_idx) = node_indices.get(import) {
                    graph.add_edge(from_idx, to_idx, ());
                    continue;
                }

                // Try matching by filename
                for (file_path, &to_idx) in &node_indices {
                    if file_path.ends_with(import) || file_path.contains(import) {
                        graph.add_edge(from_idx, to_idx, ());
                        break;
                    }
                }
            }
        }

        // Calculate component metrics
        let mut components: Vec<ComponentMetrics> = Vec::new();
        for (file_path, &node_idx) in &node_indices {
            let fan_in = graph.edges_directed(node_idx, Direction::Incoming).count();
            let fan_out = graph.edges_directed(node_idx, Direction::Outgoing).count();
            let total = fan_in + fan_out;

            let instability = if total == 0 {
                0.0
            } else {
                fan_out as f64 / total as f64
            };

            let is_hub = total > self.config.thresholds.hub_threshold && fan_in >= 3;
            let is_god = fan_in > self.config.thresholds.god_fan_in_threshold
                && fan_out > self.config.thresholds.god_fan_out_threshold;

            components.push(ComponentMetrics {
                id: file_path.clone(),
                name: file_path.clone(),
                fan_in,
                fan_out,
                instability,
                is_hub,
                is_god,
            });
        }

        // Detect smells
        let mut smells: Vec<Smell> = Vec::new();

        // 1. Detect cyclic dependencies using Tarjan's SCC
        let sccs = tarjan_scc(&graph);
        for scc in sccs {
            if scc.len() > 1 {
                let component_names: Vec<String> =
                    scc.iter().map(|&idx| graph[idx].clone()).collect();

                smells.push(Smell {
                    smell_type: SmellType::CyclicDependency,
                    severity: Severity::Critical,
                    components: component_names.clone(),
                    description: format!(
                        "Cyclic dependency detected between {} components: {}",
                        scc.len(),
                        format_component_list(&component_names)
                    ),
                    suggestion: "Break the cycle by introducing an interface or restructuring the dependency direction".to_string(),
                    metrics: SmellMetrics {
                        fan_in: None,
                        fan_out: None,
                        instability: None,
                        cycle_length: Some(scc.len()),
                    },
                });
            }
        }

        // 2. Detect hub-like dependencies
        for cm in &components {
            if cm.is_hub && !cm.is_god {
                smells.push(Smell {
                    smell_type: SmellType::HubLikeDependency,
                    severity: Severity::High,
                    components: vec![cm.id.clone()],
                    description: format!(
                        "Hub-like component \"{}\" has {} connections (fan-in={}, fan-out={}, threshold={})",
                        cm.name,
                        cm.fan_in + cm.fan_out,
                        cm.fan_in,
                        cm.fan_out,
                        self.config.thresholds.hub_threshold
                    ),
                    suggestion: "Consider splitting this component into smaller, more focused modules".to_string(),
                    metrics: SmellMetrics {
                        fan_in: Some(cm.fan_in),
                        fan_out: Some(cm.fan_out),
                        instability: Some(cm.instability),
                        cycle_length: None,
                    },
                });
            }
        }

        // 3. Detect god components
        for cm in &components {
            if cm.is_god {
                smells.push(Smell {
                    smell_type: SmellType::GodComponent,
                    severity: Severity::Critical,
                    components: vec![cm.id.clone()],
                    description: format!(
                        "God component \"{}\" has excessive coupling (fan-in={}, fan-out={})",
                        cm.name, cm.fan_in, cm.fan_out
                    ),
                    suggestion: "Decompose into smaller components with single responsibility; extract interfaces for consumers".to_string(),
                    metrics: SmellMetrics {
                        fan_in: Some(cm.fan_in),
                        fan_out: Some(cm.fan_out),
                        instability: Some(cm.instability),
                        cycle_length: None,
                    },
                });
            }
        }

        // 4. Detect unstable dependencies
        let component_map: HashMap<String, &ComponentMetrics> =
            components.iter().map(|c| (c.id.clone(), c)).collect();

        for (from_file, imports) in &file_imports {
            let from_cm = match component_map.get(from_file) {
                Some(cm) => cm,
                None => continue,
            };

            for import in imports {
                // Find the target component
                let to_cm = if let Some(cm) = component_map.get(import) {
                    *cm
                } else {
                    // Try to find by suffix
                    let found = node_indices
                        .keys()
                        .find(|k| k.ends_with(import) || k.contains(import));
                    if let Some(key) = found {
                        match component_map.get(key) {
                            Some(cm) => *cm,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                };

                let is_from_stable = from_cm.instability < self.config.thresholds.stable_threshold;
                let is_to_unstable = to_cm.instability > self.config.thresholds.unstable_threshold;

                if is_from_stable && is_to_unstable {
                    let diff = to_cm.instability - from_cm.instability;
                    if diff > self.config.thresholds.instability_difference {
                        smells.push(Smell {
                            smell_type: SmellType::UnstableDependency,
                            severity: Severity::Medium,
                            components: vec![from_cm.id.clone(), to_cm.id.clone()],
                            description: format!(
                                "Stable component \"{}\" (I={:.2}) depends on unstable component \"{}\" (I={:.2})",
                                from_cm.name, from_cm.instability, to_cm.name, to_cm.instability
                            ),
                            suggestion: "Introduce an interface in the stable component that the unstable component implements (Dependency Inversion)".to_string(),
                            metrics: SmellMetrics {
                                fan_in: None,
                                fan_out: None,
                                instability: Some(diff),
                                cycle_length: None,
                            },
                        });
                    }
                }
            }
        }

        // Sort smells by severity (critical first)
        smells.sort_by(|a, b| b.severity.weight().cmp(&a.severity.weight()));

        // Calculate summary
        let summary = calculate_summary(&smells, &components);

        Ok(Analysis {
            generated_at: Utc::now().to_rfc3339(),
            smells,
            components,
            summary,
            thresholds: self.config.thresholds.clone(),
        })
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "smells"
    }

    fn description(&self) -> &'static str {
        "Detect architecture anti-patterns"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        self.analyze_repo(ctx.root)
    }
}

/// Format a list of components for display.
fn format_component_list(components: &[String]) -> String {
    if components.is_empty() {
        return String::new();
    }
    if components.len() <= 3 {
        return components.join(" -> ");
    }
    format!(
        "{} -> ... -> {}",
        components[0],
        components[components.len() - 1]
    )
}

/// Calculate summary statistics.
fn calculate_summary(smells: &[Smell], components: &[ComponentMetrics]) -> Summary {
    let mut summary = Summary {
        total_smells: smells.len(),
        total_components: components.len(),
        ..Default::default()
    };

    for smell in smells {
        match smell.smell_type {
            SmellType::CyclicDependency => summary.cyclic_count += 1,
            SmellType::HubLikeDependency | SmellType::Hub => summary.hub_count += 1,
            SmellType::UnstableDependency => summary.unstable_count += 1,
            SmellType::GodComponent | SmellType::GodClass => summary.god_count += 1,
            SmellType::FeatureEnvy => {}
        }

        match smell.severity {
            Severity::Critical => summary.critical_count += 1,
            Severity::High => summary.high_count += 1,
            Severity::Medium => summary.medium_count += 1,
            Severity::Low => {}
        }
    }

    if !components.is_empty() {
        let total_instability: f64 = components.iter().map(|c| c.instability).sum();
        summary.average_instability = total_instability / components.len() as f64;
    }

    summary
}

/// Architectural smell analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: String,
    pub smells: Vec<Smell>,
    pub components: Vec<ComponentMetrics>,
    pub summary: Summary,
    pub thresholds: Thresholds,
}

/// A detected architectural smell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Smell {
    pub smell_type: SmellType,
    pub severity: Severity,
    pub components: Vec<String>,
    pub description: String,
    pub suggestion: String,
    pub metrics: SmellMetrics,
}

/// Quantitative metrics about a smell.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SmellMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_in: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_out: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_length: Option<usize>,
}

/// Type of architectural smell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmellType {
    CyclicDependency,
    GodClass,
    UnstableDependency,
    FeatureEnvy,
    Hub,
    HubLikeDependency,
    GodComponent,
}

/// Severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    pub fn weight(&self) -> u32 {
        match self {
            Severity::Critical => 4,
            Severity::High => 3,
            Severity::Medium => 2,
            Severity::Low => 1,
        }
    }
}

/// Component instability metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentMetrics {
    pub id: String,
    pub name: String,
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f64,
    pub is_hub: bool,
    pub is_god: bool,
}

/// Summary statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_smells: usize,
    pub cyclic_count: usize,
    pub hub_count: usize,
    pub unstable_count: usize,
    pub god_count: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub total_components: usize,
    pub average_instability: f64,
}

// Keep backward compatibility with old struct name
pub type AnalysisSummary = Summary;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.config.thresholds.hub_threshold, 20);
        assert_eq!(analyzer.config.thresholds.god_fan_in_threshold, 10);
    }

    #[test]
    fn test_thresholds_default() {
        let thresholds = Thresholds::default();
        assert_eq!(thresholds.hub_threshold, 20);
        assert_eq!(thresholds.god_fan_in_threshold, 10);
        assert_eq!(thresholds.god_fan_out_threshold, 10);
        assert!((thresholds.instability_difference - 0.4).abs() < 0.001);
        assert!((thresholds.stable_threshold - 0.3).abs() < 0.001);
        assert!((thresholds.unstable_threshold - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_analyzer_with_hub_threshold() {
        let analyzer = Analyzer::new().with_hub_threshold(30);
        assert_eq!(analyzer.config.thresholds.hub_threshold, 30);
    }

    #[test]
    fn test_analyzer_with_god_thresholds() {
        let analyzer = Analyzer::new().with_god_thresholds(15, 15);
        assert_eq!(analyzer.config.thresholds.god_fan_in_threshold, 15);
        assert_eq!(analyzer.config.thresholds.god_fan_out_threshold, 15);
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "smells");
        assert!(analyzer.description().contains("anti-patterns"));
    }

    #[test]
    fn test_severity_weight() {
        assert!(Severity::Critical.weight() > Severity::High.weight());
        assert!(Severity::High.weight() > Severity::Medium.weight());
        assert!(Severity::Medium.weight() > Severity::Low.weight());
    }

    #[test]
    fn test_smell_types() {
        assert_ne!(SmellType::CyclicDependency, SmellType::GodComponent);
        assert_eq!(SmellType::CyclicDependency, SmellType::CyclicDependency);
    }

    #[test]
    fn test_format_component_list_empty() {
        assert_eq!(format_component_list(&[]), "");
    }

    #[test]
    fn test_format_component_list_short() {
        let components = vec!["a.rs".to_string(), "b.rs".to_string()];
        assert_eq!(format_component_list(&components), "a.rs -> b.rs");
    }

    #[test]
    fn test_format_component_list_long() {
        let components = vec![
            "a.rs".to_string(),
            "b.rs".to_string(),
            "c.rs".to_string(),
            "d.rs".to_string(),
        ];
        assert_eq!(format_component_list(&components), "a.rs -> ... -> d.rs");
    }

    #[test]
    fn test_calculate_summary_empty() {
        let summary = calculate_summary(&[], &[]);
        assert_eq!(summary.total_smells, 0);
        assert_eq!(summary.total_components, 0);
        assert_eq!(summary.average_instability, 0.0);
    }

    #[test]
    fn test_calculate_summary_with_smells() {
        let smells = vec![
            Smell {
                smell_type: SmellType::CyclicDependency,
                severity: Severity::Critical,
                components: vec!["a".to_string(), "b".to_string()],
                description: String::new(),
                suggestion: String::new(),
                metrics: SmellMetrics::default(),
            },
            Smell {
                smell_type: SmellType::HubLikeDependency,
                severity: Severity::High,
                components: vec!["c".to_string()],
                description: String::new(),
                suggestion: String::new(),
                metrics: SmellMetrics::default(),
            },
        ];

        let components = vec![
            ComponentMetrics {
                id: "a".to_string(),
                name: "a".to_string(),
                fan_in: 5,
                fan_out: 5,
                instability: 0.5,
                is_hub: false,
                is_god: false,
            },
            ComponentMetrics {
                id: "b".to_string(),
                name: "b".to_string(),
                fan_in: 2,
                fan_out: 8,
                instability: 0.8,
                is_hub: false,
                is_god: false,
            },
        ];

        let summary = calculate_summary(&smells, &components);
        assert_eq!(summary.total_smells, 2);
        assert_eq!(summary.cyclic_count, 1);
        assert_eq!(summary.hub_count, 1);
        assert_eq!(summary.critical_count, 1);
        assert_eq!(summary.high_count, 1);
        assert_eq!(summary.total_components, 2);
        assert!((summary.average_instability - 0.65).abs() < 0.01);
    }

    #[test]
    fn test_component_metrics() {
        let cm = ComponentMetrics {
            id: "test.rs".to_string(),
            name: "test.rs".to_string(),
            fan_in: 5,
            fan_out: 10,
            instability: 10.0 / 15.0,
            is_hub: false,
            is_god: false,
        };

        assert_eq!(cm.fan_in, 5);
        assert_eq!(cm.fan_out, 10);
        assert!((cm.instability - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_smell_serialization() {
        let smell = Smell {
            smell_type: SmellType::CyclicDependency,
            severity: Severity::Critical,
            components: vec!["a.rs".to_string(), "b.rs".to_string()],
            description: "Test cycle".to_string(),
            suggestion: "Break it".to_string(),
            metrics: SmellMetrics {
                cycle_length: Some(2),
                ..Default::default()
            },
        };

        let json = serde_json::to_string(&smell).unwrap();
        assert!(json.contains("\"CyclicDependency\""));
        assert!(json.contains("\"Critical\""));

        let parsed: Smell = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.smell_type, SmellType::CyclicDependency);
        assert_eq!(parsed.severity, Severity::Critical);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            smells: vec![],
            components: vec![],
            summary: Summary::default(),
            thresholds: Thresholds::default(),
        };

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("generated_at"));

        let parsed: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.smells.len(), 0);
    }

    #[test]
    fn test_instability_calculation() {
        // Fully stable (all incoming)
        let stable = ComponentMetrics {
            id: "s".to_string(),
            name: "s".to_string(),
            fan_in: 10,
            fan_out: 0,
            instability: 0.0,
            is_hub: false,
            is_god: false,
        };
        assert_eq!(stable.instability, 0.0);

        // Fully unstable (all outgoing)
        let unstable = ComponentMetrics {
            id: "u".to_string(),
            name: "u".to_string(),
            fan_in: 0,
            fan_out: 10,
            instability: 1.0,
            is_hub: false,
            is_god: false,
        };
        assert_eq!(unstable.instability, 1.0);
    }
}
