//! Architectural smells analyzer.
//!
//! Detects architectural anti-patterns in dependency graphs:
//! - Cyclic dependencies (using Tarjan's SCC algorithm, including self-loops)
//! - Hub-like dependencies (excessive fan-in + fan-out)
//! - Central connectors (high fan-in AND high fan-out coupling)
//! - Unstable dependencies (stable components depending on unstable ones)
//!
//! Based on detection algorithms from Fontana et al. (2017) "Arcan".
//!
//! **Note on terminology**: This implementation uses "Central Connector" instead of
//! Arcan's "God Component". Arcan's God Component detection is based on Lines of Code
//! (LOC) metrics, whereas our Central Connector detection uses bidirectional coupling
//! metrics (fan-in + fan-out). Both indicate components that may need decomposition,
//! but they measure different aspects of over-centralization.

use std::collections::HashMap;

use chrono::Utc;
use petgraph::algo::tarjan_scc;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::analyzers::import_resolver::{
    build_dependency_graph, extract_resolved_imports, ImportIndex,
};
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    /// Fan-in + Fan-out threshold for hub detection.
    pub hub_threshold: usize,
    /// Minimum fan-in for central connector detection.
    pub central_connector_fan_in_threshold: usize,
    /// Minimum fan-out for central connector detection.
    pub central_connector_fan_out_threshold: usize,
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
            central_connector_fan_in_threshold: 10,
            central_connector_fan_out_threshold: 10,
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

    pub fn with_central_connector_thresholds(mut self, fan_in: usize, fan_out: usize) -> Self {
        self.config.thresholds.central_connector_fan_in_threshold = fan_in;
        self.config.thresholds.central_connector_fan_out_threshold = fan_out;
        self
    }

    pub fn with_instability_difference(mut self, diff: f64) -> Self {
        self.config.thresholds.instability_difference = diff;
        self
    }

    /// Analyze a repository for architectural smells.
    /// Uses ctx.read_file() to support both filesystem and git tree sources.
    pub fn analyze_repo(&self, ctx: &AnalysisContext<'_>) -> Result<Analysis> {
        // Phase 1: Get files from context (already filtered by language)
        let files: Vec<_> = ctx.files.iter().collect();
        let file_paths: Vec<std::path::PathBuf> = files.iter().map(|p| (*p).clone()).collect();

        // Build the shared import index, then extract+resolve every file's
        // imports and build the graph through the same shared functions
        // `graph` uses. Sharing this whole pipeline (not just the resolver)
        // is what guarantees the two analyzers can never disagree on edges
        // or cycles for the same input (issue #479).
        let file_index = ImportIndex::new(&file_paths, ctx.root);
        let file_imports = extract_resolved_imports(&file_paths, ctx, &file_index, false);
        let (graph, node_indices) = build_dependency_graph(&file_imports, false);

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
            let is_central_connector = fan_in
                > self.config.thresholds.central_connector_fan_in_threshold
                && fan_out > self.config.thresholds.central_connector_fan_out_threshold;

            components.push(ComponentMetrics {
                id: file_path.clone(),
                name: file_path.clone(),
                fan_in,
                fan_out,
                instability,
                is_hub,
                is_central_connector,
            });
        }

        // Detect smells
        let mut smells: Vec<Smell> = Vec::new();

        // 1. Detect cyclic dependencies using Tarjan's SCC
        // Also detect self-loops (files importing themselves)
        let sccs = tarjan_scc(&graph);
        for scc in sccs {
            let is_cycle = scc.len() > 1 || (scc.len() == 1 && graph.contains_edge(scc[0], scc[0]));
            if is_cycle {
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
            if cm.is_hub && !cm.is_central_connector {
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

        // 3. Detect central connectors (high bidirectional coupling)
        // Note: This differs from Arcan's "God Component" which uses LOC metrics.
        // Central Connector detects components with high fan-in AND fan-out,
        // indicating they serve as communication hubs that may need decomposition.
        for cm in &components {
            if cm.is_central_connector {
                smells.push(Smell {
                    smell_type: SmellType::CentralConnector,
                    severity: Severity::Critical,
                    components: vec![cm.id.clone()],
                    description: format!(
                        "Central connector \"{}\" has excessive bidirectional coupling (fan-in={}, fan-out={})",
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

            // `build_dependency_graph` collapses a repeated import into a
            // single edge; this loop must be driven off that same edge set,
            // or importing the same target twice pushes two identical
            // UnstableDependency smells for one real edge.
            let mut unique_imports: Vec<&String> = imports.iter().collect();
            unique_imports.sort();
            unique_imports.dedup();

            for import in unique_imports {
                // Imports are already resolved to on-disk paths (Phase 2).
                let to_cm = match component_map.get(import) {
                    Some(cm) => *cm,
                    None => continue,
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
        self.analyze_repo(ctx)
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
            SmellType::CentralConnector => summary.central_connector_count += 1,
            // Backward compatibility with old smell types omen:ignore
            SmellType::GodComponent | SmellType::GodClass => summary.central_connector_count += 1,
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

/// Architectural smell analysis result. omen:ignore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: String,
    pub smells: Vec<Smell>,
    pub components: Vec<ComponentMetrics>,
    pub summary: Summary,
    pub thresholds: Thresholds,
}

/// A detected architectural smell. omen:ignore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Smell {
    pub smell_type: SmellType,
    pub severity: Severity,
    pub components: Vec<String>,
    pub description: String,
    pub suggestion: String,
    pub metrics: SmellMetrics,
}

/// Quantitative metrics about a smell. omen:ignore
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

/// Type of architectural smell. omen:ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmellType {
    CyclicDependency,
    UnstableDependency,
    FeatureEnvy,
    Hub,
    HubLikeDependency,
    /// High bidirectional coupling (high fan-in AND fan-out).
    /// Note: This differs from Arcan's "God Component" which uses LOC metrics.
    CentralConnector,
    // Backward compatibility aliases
    #[serde(alias = "GodComponent")]
    GodComponent,
    #[serde(alias = "GodClass")]
    GodClass,
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
    /// High bidirectional coupling (high fan-in AND fan-out).
    pub is_central_connector: bool,
}

/// Summary statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_smells: usize,
    pub cyclic_count: usize,
    pub hub_count: usize,
    pub unstable_count: usize,
    pub central_connector_count: usize,
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
        assert_eq!(
            analyzer
                .config
                .thresholds
                .central_connector_fan_in_threshold,
            10
        );
    }

    #[test]
    fn test_thresholds_default() {
        let thresholds = Thresholds::default();
        assert_eq!(thresholds.hub_threshold, 20);
        assert_eq!(thresholds.central_connector_fan_in_threshold, 10);
        assert_eq!(thresholds.central_connector_fan_out_threshold, 10);
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
    fn test_analyzer_with_central_connector_thresholds() {
        let analyzer = Analyzer::new().with_central_connector_thresholds(15, 15);
        assert_eq!(
            analyzer
                .config
                .thresholds
                .central_connector_fan_in_threshold,
            15
        );
        assert_eq!(
            analyzer
                .config
                .thresholds
                .central_connector_fan_out_threshold,
            15
        );
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
        assert_ne!(SmellType::CyclicDependency, SmellType::CentralConnector);
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
                is_central_connector: false,
            },
            ComponentMetrics {
                id: "b".to_string(),
                name: "b".to_string(),
                fan_in: 2,
                fan_out: 8,
                instability: 0.8,
                is_hub: false,
                is_central_connector: false,
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
            is_central_connector: false,
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
            is_central_connector: false,
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
            is_central_connector: false,
        };
        assert_eq!(unstable.instability, 1.0);
    }

    #[test]
    fn test_self_loop_cycle_detection() {
        // A file importing itself should be flagged as cyclic
        use petgraph::graph::DiGraph;

        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let node = graph.add_node("self_loop.rs".to_string());
        graph.add_edge(node, node, ()); // Self-loop

        let sccs = tarjan_scc(&graph);

        // Verify self-loop detection logic
        let mut found_cycle = false;
        for scc in sccs {
            let is_cycle = scc.len() > 1 || (scc.len() == 1 && graph.contains_edge(scc[0], scc[0]));
            if is_cycle {
                found_cycle = true;
                assert_eq!(scc.len(), 1);
                assert_eq!(graph[scc[0]], "self_loop.rs");
            }
        }
        assert!(found_cycle, "Self-loop should be detected as a cycle");
    }

    #[test]
    fn test_multi_node_cycle_detection() {
        // Traditional cycle: A -> B -> A
        use petgraph::graph::DiGraph;

        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let a = graph.add_node("a.rs".to_string());
        let b = graph.add_node("b.rs".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(b, a, ());

        let sccs = tarjan_scc(&graph);

        let mut found_cycle = false;
        for scc in sccs {
            let is_cycle = scc.len() > 1 || (scc.len() == 1 && graph.contains_edge(scc[0], scc[0]));
            if is_cycle {
                found_cycle = true;
                assert_eq!(scc.len(), 2);
            }
        }
        assert!(found_cycle, "Multi-node cycle should be detected");
    }

    /// Write files (relative path -> content) into a fresh temp dir and run
    /// the smells analyzer over it from the filesystem.
    fn analyze_fixture(files: &[(&str, &str)]) -> Analysis {
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet};

        let temp_dir = tempfile::tempdir().unwrap();
        for (rel_path, content) in files {
            let full_path = temp_dir.path().join(rel_path);
            std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
            std::fs::write(full_path, content).unwrap();
        }

        let config = Config::default();
        let file_set = FileSet::from_path(temp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()));
        Analyzer::new().analyze_repo(&ctx).unwrap()
    }

    #[test]
    fn test_no_phantom_self_cycle_on_same_named_ts_files() {
        // Issue #479: `packages/brain/src/types.ts` imports a DIFFERENT file
        // (`packages/mcp/src/types.ts`) that happens to share the same file
        // stem. The old global-stem `.first()` matching in smells.rs could
        // fabricate a self-edge instead of resolving the relative ESM
        // specifier to the actual file it points at.
        let analysis = analyze_fixture(&[
            (
                "packages/brain/src/types.ts",
                "import type { M } from '../../mcp/src/types.js';\nexport type B = M;\n",
            ),
            ("packages/mcp/src/types.ts", "export type M = number;\n"),
        ]);

        assert_eq!(
            analysis.summary.cyclic_count,
            0,
            "no cyclic dependency should be reported, found: {:?}",
            analysis
                .smells
                .iter()
                .filter(|s| s.smell_type == SmellType::CyclicDependency)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_phantom_cycle_in_ts_monorepo_with_barrels() {
        // A small TS "monorepo" with several `index.ts` barrels and `.js`
        // ESM-style relative imports, but NO real import cycle. Global-stem
        // matching can wire barrels to the wrong same-named file and
        // fabricate cycles that do not exist in the real import graph.
        let analysis = analyze_fixture(&[
            ("pkg-a/src/index.ts", "export * from './widget.js';\n"),
            ("pkg-a/src/widget.ts", "export const widget = 1;\n"),
            (
                "pkg-b/src/index.ts",
                "import { widget } from '../../pkg-a/src/index.js';\nexport const used = widget;\n",
            ),
            ("pkg-c/src/index.ts", "export const c = 1;\n"),
        ]);

        assert_eq!(
            analysis.summary.cyclic_count,
            0,
            "no cyclic dependency should be reported in an acyclic monorepo, found: {:?}",
            analysis
                .smells
                .iter()
                .filter(|s| s.smell_type == SmellType::CyclicDependency)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_real_ts_cycle_is_still_detected() {
        // Do not over-correct: a genuine two-file cycle must still be caught.
        let analysis = analyze_fixture(&[
            ("a.ts", "import { b } from './b.js';\nexport const a = b;\n"),
            ("b.ts", "import { a } from './a.js';\nexport const b = a;\n"),
        ]);

        assert_eq!(
            analysis.summary.cyclic_count, 1,
            "the real a.ts <-> b.ts cycle must still be reported"
        );
        let cyclic = analysis
            .smells
            .iter()
            .find(|s| s.smell_type == SmellType::CyclicDependency)
            .expect("expected a cyclic dependency smell");
        assert_eq!(cyclic.metrics.cycle_length, Some(2));
    }

    #[test]
    fn test_self_import_is_not_a_cyclic_smell() {
        // A file importing itself (`import "./a.js"` inside a.ts) is not a
        // real cyclic dependency between two components. `graph` has always
        // rejected self-edges when building its graph; `smells` must apply
        // the same rule so the two analyzers cannot disagree (issue #479).
        let analysis = analyze_fixture(&[(
            "a.ts",
            "import { helper } from './a.js';\nexport function helper() {}\n",
        )]);

        assert_eq!(
            analysis.summary.cyclic_count,
            0,
            "a file importing itself is not a cyclic-dependency smell, found: {:?}",
            analysis
                .smells
                .iter()
                .filter(|s| s.smell_type == SmellType::CyclicDependency)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_repeated_import_produces_single_edge_not_parallel_edges() {
        // Importing the same target twice (e.g. two named imports resolving
        // to the same file) must not inflate fan_in/fan_out with duplicate
        // parallel edges -- `graph` already dedups, `smells` must match.
        let analysis = analyze_fixture(&[
            (
                "a.ts",
                "import { x } from './b.js';\nimport { y } from './b.js';\n",
            ),
            ("b.ts", "export const x = 1;\nexport const y = 2;\n"),
        ]);

        let a = analysis
            .components
            .iter()
            .find(|c| c.id == "a.ts")
            .expect("a.ts component");
        let b = analysis
            .components
            .iter()
            .find(|c| c.id == "b.ts")
            .expect("b.ts component");
        assert_eq!(
            a.fan_out, 1,
            "repeated import must not create a parallel edge"
        );
        assert_eq!(
            b.fan_in, 1,
            "repeated import must not create a parallel edge"
        );
    }

    #[test]
    fn test_repeated_import_produces_single_unstable_dependency_smell() {
        // The unstable-dependency smell loop iterated the raw (possibly
        // duplicated) resolved-imports list instead of the deduped graph
        // edges, so a file importing the same unstable target twice could
        // push two identical `UnstableDependency` smells even though
        // `build_dependency_graph` collapses them to a single edge.
        let analysis = analyze_fixture(&[
            // stable.ts: fan_in=4 (d1..d4), fan_out=1 (deduped edge to
            // unstable.ts, despite two duplicate import statements) ->
            // instability = 1/5 = 0.2 (stable).
            (
                "stable.ts",
                "import { x } from './unstable.js';\nimport { y } from './unstable.js';\n",
            ),
            // unstable.ts: fan_in=1, fan_out=4 -> instability = 4/5 = 0.8
            // (unstable). diff vs stable.ts = 0.6, over the 0.4 threshold.
            (
                "unstable.ts",
                "import './u1.js';\nimport './u2.js';\nimport './u3.js';\nimport './u4.js';\nexport const x = 1;\nexport const y = 2;\n",
            ),
            ("u1.ts", "export const u1 = 1;\n"),
            ("u2.ts", "export const u2 = 1;\n"),
            ("u3.ts", "export const u3 = 1;\n"),
            ("u4.ts", "export const u4 = 1;\n"),
            ("d1.ts", "import './stable.js';\n"),
            ("d2.ts", "import './stable.js';\n"),
            ("d3.ts", "import './stable.js';\n"),
            ("d4.ts", "import './stable.js';\n"),
        ]);

        let unstable_smells: Vec<_> = analysis
            .smells
            .iter()
            .filter(|s| {
                s.smell_type == SmellType::UnstableDependency
                    && s.components == vec!["stable.ts".to_string(), "unstable.ts".to_string()]
            })
            .collect();
        assert_eq!(
            unstable_smells.len(),
            1,
            "a repeated import must produce a single UnstableDependency smell, found: {:?}",
            unstable_smells
        );
    }

    #[test]
    fn test_analyzer_uses_content_source_for_historical_commits() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet, TreeSource};
        use std::process::Command;
        use std::sync::Arc;

        let temp_dir = tempfile::tempdir().unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to init");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to config name");

        // First commit: 2 TypeScript files with NO cyclic dependency
        std::fs::write(
            temp_dir.path().join("a.ts"),
            b"// No imports\nexport const a = 1;",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("b.ts"),
            b"// No imports\nexport const b = 2;",
        )
        .unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "First commit - no cycles"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to get HEAD");
        let sha1 = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // Second commit: modify files to CREATE a cyclic dependency
        std::fs::write(
            temp_dir.path().join("a.ts"),
            b"import { b } from './b';\nexport const a = b + 1;",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("b.ts"),
            b"import { a } from './a';\nexport const b = a + 1;",
        )
        .unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "Second commit - introduces cycle"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");

        // Analyze at the FIRST commit (should have NO cycles)
        let tree_source = TreeSource::new(temp_dir.path(), &sha1).unwrap();
        let config = Config::default();
        let file_set = FileSet::from_tree_source(&tree_source, &config).unwrap();
        let content_source: Arc<dyn crate::core::ContentSource> = Arc::new(tree_source);

        let ctx = AnalysisContext::new(&file_set, &config, Some(temp_dir.path()))
            .with_content_source(content_source);

        let analyzer = Analyzer::new();
        let result = analyzer.analyze(&ctx).unwrap();

        // The analysis should find NO cycles because we're analyzing the first commit
        // which had no cyclic imports. If the analyzer reads from filesystem instead
        // of content_source, it will incorrectly find the cycle from the second commit.
        assert_eq!(
            result.summary.cyclic_count, 0,
            "Historical commit analysis should find no cycles, but found {}. \
             This indicates the analyzer is reading from filesystem instead of content_source.",
            result.summary.cyclic_count
        );
    }
}
