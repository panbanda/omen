//! Types for HTML report generation matching Go version.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata contains report generation metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub repository: String,
    pub generated_at: DateTime<Utc>,
    pub since: String,
    pub omen_version: String,
    #[serde(default)]
    pub paths: Vec<String>,
}

/// ScoreData represents the score.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScoreData {
    pub score: i32,
    pub passed: bool,
    pub files_analyzed: i32,
    #[serde(default)]
    pub components: HashMap<String, i32>,
}

/// ScoreRaw represents the raw score.json from the score command.
#[derive(Debug, Clone, Deserialize)]
pub struct ScoreRaw {
    pub overall_score: f64,
    #[serde(default)]
    pub components: HashMap<String, ScoreComponent>,
    #[serde(default)]
    pub summary: ScoreSummary,
}

/// ScoreComponent for nested score data.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScoreComponent {
    pub score: f64,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub details: String,
}

/// ScoreSummary from score.json.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScoreSummary {
    #[serde(default)]
    pub files_analyzed: i32,
    #[serde(default)]
    pub analyzers_run: i32,
    #[serde(default)]
    pub critical_issues: i32,
}

/// ComplexityData represents complexity statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComplexityData {
    pub avg_cyclomatic: f64,
    pub avg_cognitive: f64,
}

/// ComplexityRaw for loading complexity.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityRaw {
    pub summary: ComplexitySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexitySummary {
    pub avg_cyclomatic: f64,
    pub avg_cognitive: f64,
}

/// HotspotsData represents the hotspots.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotspotsData {
    #[serde(default, alias = "hotspots")]
    pub files: Vec<HotspotItem>,
    pub summary: Option<HotspotSummary>,
}

/// HotspotSummary contains aggregate hotspot stats.
/// The JSON may contain different fields depending on the analyzer version,
/// so all fields use defaults and the renderer computes what it needs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotspotSummary {
    #[serde(default)]
    pub max_score: f64,
    #[serde(default)]
    pub avg_score: f64,
    #[serde(default)]
    pub critical_count: i32,
    #[serde(default)]
    pub high_count: i32,
    #[serde(default)]
    pub total_hotspots: i32,
}

/// HotspotItem represents a single hotspot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotItem {
    #[serde(alias = "file")]
    pub path: String,
    #[serde(alias = "score")]
    pub hotspot_score: f64,
    #[serde(default)]
    pub commits: i32,
    #[serde(default, alias = "avg_complexity")]
    pub avg_cognitive: f64,
}

/// SATDData represents the satd.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SATDData {
    #[serde(default)]
    pub items: Vec<SATDItem>,
}

/// SATDItem represents a single SATD item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SATDItem {
    pub file: String,
    pub line: i32,
    pub severity: String,
    pub category: String,
    #[serde(alias = "text")]
    pub content: String,
}

/// SATDStats contains SATD statistics for charts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SATDStats {
    pub critical: i32,
    pub high: i32,
    pub medium: i32,
    pub low: i32,
    pub categories: HashMap<String, i32>,
}

/// ChurnData represents the churn.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChurnData {
    #[serde(default)]
    pub files: Vec<ChurnFile>,
    #[serde(default)]
    pub summary: ChurnSummary,
}

/// ChurnSummary contains aggregate churn metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChurnSummary {
    #[serde(default, alias = "total_file_changes")]
    pub total_commits: i32,
    #[serde(default, alias = "total_files_changed")]
    pub unique_files: i32,
    #[serde(default, alias = "total_additions")]
    pub total_added: i32,
    #[serde(default, alias = "total_deletions")]
    pub total_deleted: i32,
}

/// ChurnFile represents a single file's churn data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChurnFile {
    #[serde(alias = "relative_path")]
    pub file: String,
    #[serde(alias = "commit_count")]
    pub commits: i32,
    #[serde(default, alias = "unique_authors")]
    pub authors: Vec<String>,
    pub churn_score: f64,
    #[serde(default)]
    pub additions: i32,
    #[serde(default)]
    pub deletions: i32,
}

/// OwnershipData represents the ownership.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OwnershipData {
    pub summary: OwnershipSummary,
    #[serde(default)]
    pub files: Vec<OwnershipFile>,
    // Computed fields for display
    #[serde(default)]
    pub bus_factor: i32,
    #[serde(default)]
    pub knowledge_silos: i32,
    #[serde(default)]
    pub total_files: i32,
    #[serde(default)]
    pub top_owners: Vec<OwnerInfo>,
}

/// OwnershipSummary contains aggregate ownership metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OwnershipSummary {
    pub total_files: i32,
    pub bus_factor: i32,
    #[serde(default)]
    pub silo_count: i32,
    #[serde(default)]
    pub top_contributors: Vec<TopContributor>,
}

/// TopContributor represents a top contributor's stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopContributor {
    pub name: String,
    pub files_owned: usize,
}

/// OwnershipFile represents a single file's ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipFile {
    pub path: String,
}

/// OwnerInfo represents a code owner's stats for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerInfo {
    pub name: String,
    pub files_owned: usize,
}

/// DuplicatesData represents the duplicates.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DuplicatesData {
    #[serde(default)]
    pub summary: DuplicatesSummary,
    // Computed fields for display
    #[serde(default)]
    pub clone_groups: i32,
    #[serde(default)]
    pub duplicate_lines: i32,
    #[serde(default)]
    pub total_lines: i32,
    #[serde(default)]
    pub duplication_ratio: f64,
}

/// DuplicatesSummary contains aggregate duplication metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DuplicatesSummary {
    #[serde(default)]
    pub total_groups: i32,
    #[serde(default)]
    pub duplicated_lines: i32,
    #[serde(default)]
    pub total_lines: i32,
    #[serde(default)]
    pub duplication_ratio: f64,
}

/// TrendData represents the trend.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrendData {
    #[serde(default)]
    pub points: Vec<TrendPoint>,
    #[serde(default)]
    pub slope: f64,
    #[serde(default)]
    pub intercept: f64,
    #[serde(default)]
    pub r_squared: f64,
    #[serde(default)]
    pub start_score: i32,
    #[serde(default)]
    pub end_score: i32,
    #[serde(default)]
    pub component_trends: HashMap<String, ComponentTrendStats>,
}

/// TrendPoint represents a single point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    pub date: String,
    pub score: i32,
    #[serde(default)]
    pub components: HashMap<String, i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notable_commits: Vec<String>,
}

/// ComponentTrendStats contains trend statistics for a component.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentTrendStats {
    #[serde(rename = "Slope")]
    pub slope: f64,
    #[serde(rename = "Correlation")]
    pub correlation: f64,
}

/// CohesionData represents the cohesion.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CohesionData {
    #[serde(default)]
    pub classes: Vec<CohesionClass>,
}

/// CohesionClass represents a single class's CK metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CohesionClass {
    pub path: String,
    pub class_name: String,
    pub language: String,
    pub wmc: i32,
    pub cbo: i32,
    pub rfc: i32,
    pub lcom: i32,
    pub dit: i32,
    pub noc: i32,
    pub nom: i32,
    pub nof: i32,
    pub loc: i32,
}

/// FlagsData represents the flags.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagsData {
    #[serde(default)]
    pub flags: Vec<FlagItem>,
    #[serde(default)]
    pub summary: FlagsSummary,
}

/// FlagItem represents a single feature flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagItem {
    #[serde(alias = "key")]
    pub flag_key: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub priority: FlagPriority,
    #[serde(default)]
    pub complexity: FlagComplexity,
    #[serde(default)]
    pub staleness: FlagStaleness,
    #[serde(default)]
    pub references: Vec<FlagReference>,
    #[serde(default)]
    pub file_spread: Option<i32>,
    #[serde(default)]
    pub first_seen: Option<String>,
    #[serde(default)]
    pub stale: Option<bool>,
}

/// FlagReference represents a single occurrence of a feature flag in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagReference {
    pub file: String,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
}

/// FlagPriority contains flag priority scoring.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagPriority {
    pub score: f64,
    pub level: String,
}

/// FlagComplexity contains flag complexity metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagComplexity {
    pub file_spread: i32,
}

/// FlagStaleness contains flag staleness metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagStaleness {
    pub introduced_at: String,
}

/// FlagsSummary contains aggregate flag metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagsSummary {
    pub total_flags: i32,
    #[serde(default)]
    pub by_priority: HashMap<String, i32>,
    #[serde(default)]
    pub by_provider: HashMap<String, i32>,
    #[serde(default)]
    pub avg_file_spread: f64,
}

// ============================================================================
// Temporal Coupling Types
// ============================================================================

/// TemporalData represents the temporal.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalData {
    #[serde(default)]
    pub couplings: Vec<TemporalCoupling>,
    #[serde(default)]
    pub summary: TemporalSummary,
}

/// A pair of files that change together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalCoupling {
    pub file_a: String,
    pub file_b: String,
    #[serde(default)]
    pub cochange_count: u32,
    #[serde(default)]
    pub coupling_strength: f64,
    #[serde(default)]
    pub commits_a: u32,
    #[serde(default)]
    pub commits_b: u32,
}

/// Summary statistics for temporal coupling.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalSummary {
    #[serde(default)]
    pub total_couplings: usize,
    #[serde(default)]
    pub strong_couplings: usize,
    #[serde(default)]
    pub avg_coupling_strength: f64,
    #[serde(default)]
    pub max_coupling_strength: f64,
    #[serde(default)]
    pub total_files_analyzed: usize,
}

// ============================================================================
// Architectural Smells Types
// ============================================================================

/// SmellsData represents the smells.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmellsData {
    #[serde(default)]
    pub smells: Vec<SmellItem>,
    #[serde(default)]
    pub components: Vec<SmellComponent>,
    #[serde(default)]
    pub summary: SmellsSummary,
}

/// A detected architectural smell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmellItem {
    pub smell_type: String,
    pub severity: String,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub metrics: SmellItemMetrics,
}

/// Quantitative metrics for a smell.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmellItemMetrics {
    #[serde(default)]
    pub fan_in: Option<usize>,
    #[serde(default)]
    pub fan_out: Option<usize>,
    #[serde(default)]
    pub instability: Option<f64>,
    #[serde(default)]
    pub cycle_length: Option<usize>,
}

/// Component-level metrics from smell analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmellComponent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub fan_in: usize,
    #[serde(default)]
    pub fan_out: usize,
    #[serde(default)]
    pub instability: f64,
    #[serde(default)]
    pub is_hub: bool,
    #[serde(default)]
    pub is_central_connector: bool,
}

/// Summary statistics for architectural smells.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmellsSummary {
    #[serde(default)]
    pub total_smells: usize,
    #[serde(default)]
    pub cyclic_count: usize,
    #[serde(default)]
    pub hub_count: usize,
    #[serde(default)]
    pub unstable_count: usize,
    #[serde(default)]
    pub central_connector_count: usize,
    #[serde(default)]
    pub critical_count: usize,
    #[serde(default)]
    pub high_count: usize,
    #[serde(default)]
    pub medium_count: usize,
    #[serde(default)]
    pub total_components: usize,
    #[serde(default)]
    pub average_instability: f64,
}

// ============================================================================
// Dependency Graph Types
// ============================================================================

/// GraphData represents the graph.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphData {
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub cycles: Vec<Vec<String>>,
    #[serde(default)]
    pub summary: GraphSummary,
}

/// A node in the dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub path: String,
    #[serde(default)]
    pub pagerank: f64,
    #[serde(default)]
    pub betweenness: f64,
    #[serde(default)]
    pub in_degree: usize,
    #[serde(default)]
    pub out_degree: usize,
    #[serde(default)]
    pub instability: f64,
}

/// An edge in the dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

/// Summary statistics for the dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphSummary {
    #[serde(default)]
    pub total_nodes: usize,
    #[serde(default)]
    pub total_edges: usize,
    #[serde(default)]
    pub avg_degree: f64,
    #[serde(default)]
    pub cycle_count: usize,
}

// ============================================================================
// Technical Debt Gradient Types
// ============================================================================

/// TdgData represents the tdg.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TdgData {
    #[serde(default)]
    pub files: Vec<TdgFile>,
    #[serde(default)]
    pub average_score: f32,
    #[serde(default)]
    pub average_grade: String,
    #[serde(default)]
    pub total_files: usize,
    #[serde(default)]
    pub grade_distribution: HashMap<String, usize>,
}

/// Per-file TDG score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TdgFile {
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub total: f32,
    #[serde(default)]
    pub grade: String,
    #[serde(default)]
    pub structural_complexity: f32,
    #[serde(default)]
    pub semantic_complexity: f32,
    #[serde(default)]
    pub duplication_ratio: f32,
    #[serde(default)]
    pub coupling_score: f32,
    #[serde(default)]
    pub hotspot_score: f32,
    #[serde(default)]
    pub temporal_coupling_score: f32,
    #[serde(default)]
    pub has_critical_defects: bool,
}

// ============================================================================
// Insight Types (LLM-generated content)
// ============================================================================

/// Recommendation represents a single recommendation item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub title: String,
    pub description: String,
}

/// Recommendations groups recommendations by priority.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Recommendations {
    #[serde(default)]
    pub high_priority: Vec<Recommendation>,
    #[serde(default)]
    pub medium_priority: Vec<Recommendation>,
    #[serde(default)]
    pub ongoing: Vec<Recommendation>,
}

/// SummaryInsight contains executive summary and recommendations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryInsight {
    #[serde(default)]
    pub executive_summary: String,
    #[serde(default)]
    pub key_findings: Vec<String>,
    #[serde(default)]
    pub recommendations: Recommendations,
}

/// ScoreAnnotation represents an annotation on the score trend chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreAnnotation {
    pub date: String,
    pub label: String,
    pub change: i32,
    pub description: String,
}

/// HistoricalEvent represents a significant score change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalEvent {
    pub period: String,
    pub change: i32,
    pub primary_driver: String,
    #[serde(default)]
    pub releases: Vec<String>,
}

/// TrendsInsight contains trend analysis and annotations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrendsInsight {
    #[serde(default)]
    pub section_insight: String,
    #[serde(default)]
    pub score_annotations: Vec<ScoreAnnotation>,
    #[serde(default)]
    pub historical_events: Vec<HistoricalEvent>,
}

/// ComponentAnnotation represents an annotation on a component trend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentAnnotation {
    pub date: String,
    pub label: String,
    pub from: i32,
    pub to: i32,
    pub description: String,
}

/// ComponentEvent represents a significant component change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentEvent {
    pub period: String,
    pub component: String,
    pub from: i32,
    pub to: i32,
    pub context: String,
}

/// ComponentsInsight contains component-level analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentsInsight {
    #[serde(default)]
    pub component_annotations: HashMap<String, Vec<ComponentAnnotation>>,
    #[serde(default)]
    pub component_events: Vec<ComponentEvent>,
    #[serde(default)]
    pub component_insights: HashMap<String, String>,
}

/// FileAnnotation represents an LLM comment on a specific file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnnotation {
    pub file: String,
    pub comment: String,
}

/// HotspotsInsight contains hotspot analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotspotsInsight {
    #[serde(default)]
    pub section_insight: String,
    #[serde(default)]
    pub item_annotations: Vec<FileAnnotation>,
}

/// OwnershipInsight contains ownership analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OwnershipInsight {
    #[serde(default)]
    pub section_insight: String,
    #[serde(default)]
    pub item_annotations: Vec<FileAnnotation>,
}

/// SATDAnnotation represents an LLM comment on a SATD item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SATDAnnotation {
    pub file: String,
    #[serde(default)]
    pub line: i32,
    pub comment: String,
}

/// SATDInsight contains SATD analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SATDInsight {
    #[serde(default)]
    pub section_insight: String,
    #[serde(default)]
    pub item_annotations: Vec<SATDAnnotation>,
}

/// ChurnInsight contains churn analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChurnInsight {
    #[serde(default)]
    pub section_insight: String,
}

/// DuplicationInsight contains duplication analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DuplicationInsight {
    #[serde(default)]
    pub section_insight: String,
}

/// FlagAnnotation represents an LLM comment on a feature flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagAnnotation {
    pub flag: String,
    pub priority: String,
    pub introduced_at: String,
    pub comment: String,
}

/// FlagsInsight contains feature flags analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlagsInsight {
    #[serde(default)]
    pub section_insight: String,
    #[serde(default)]
    pub item_annotations: Vec<FlagAnnotation>,
}

/// TemporalInsight contains temporal coupling analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalInsight {
    #[serde(default)]
    pub section_insight: String,
}

/// SmellsInsight contains architectural smells analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmellsInsight {
    #[serde(default)]
    pub section_insight: String,
}

/// GraphInsight contains dependency graph analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphInsight {
    #[serde(default)]
    pub section_insight: String,
}

/// TdgInsight contains technical debt gradient analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TdgInsight {
    #[serde(default)]
    pub section_insight: String,
}

// ============================================================================
// Render Data (combined structure for template)
// ============================================================================

/// RenderData contains all data needed to render the report.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RenderData {
    pub metadata: Metadata,
    pub score: ScoreData,
    pub score_class: String,
    pub complexity: Option<ComplexityData>,
    pub hotspots: Option<HotspotsData>,
    pub satd: Option<SATDData>,
    pub churn: Option<ChurnData>,
    pub ownership: Option<OwnershipData>,
    pub duplicates: Option<DuplicatesData>,
    pub cohesion: Option<CohesionData>,
    pub flags: Option<FlagsData>,
    pub trend: Option<TrendData>,
    pub summary: Option<SummaryInsight>,
    pub hotspots_insight: Option<HotspotsInsight>,
    pub satd_insight: Option<SATDInsight>,
    pub trends_insight: Option<TrendsInsight>,
    pub churn_insight: Option<ChurnInsight>,
    pub duplication_insight: Option<DuplicationInsight>,
    pub components_insight: Option<ComponentsInsight>,
    pub flags_insight: Option<FlagsInsight>,
    pub ownership_insight: Option<OwnershipInsight>,
    pub temporal_insight: Option<TemporalInsight>,
    pub smells_insight: Option<SmellsInsight>,
    pub graph_insight: Option<GraphInsight>,
    pub tdg_insight: Option<TdgInsight>,
    pub component_trends: HashMap<String, ComponentTrendStats>,
    pub satd_stats: Option<SATDStats>,
    pub temporal: Option<TemporalData>,
    pub smells: Option<SmellsData>,
    pub graph: Option<GraphData>,
    pub tdg: Option<TdgData>,
}

impl RenderData {
    /// Compute the score class based on score value.
    pub fn compute_score_class(&mut self) {
        self.score_class = if self.score.score >= 80 {
            "good".to_string()
        } else if self.score.score >= 60 {
            "warning".to_string()
        } else {
            "danger".to_string()
        };
    }

    /// Compute SATD statistics from items.
    pub fn compute_satd_stats(&mut self) {
        if let Some(ref satd) = self.satd {
            let mut stats = SATDStats::default();
            for item in &satd.items {
                match item.severity.to_lowercase().as_str() {
                    "critical" => stats.critical += 1,
                    "high" => stats.high += 1,
                    "medium" => stats.medium += 1,
                    _ => stats.low += 1,
                }
                if !item.category.is_empty() {
                    *stats.categories.entry(item.category.clone()).or_insert(0) += 1;
                }
            }
            self.satd_stats = Some(stats);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_class_good() {
        let mut data = RenderData::default();
        data.score.score = 85;
        data.compute_score_class();
        assert_eq!(data.score_class, "good");
    }

    #[test]
    fn test_score_class_warning() {
        let mut data = RenderData::default();
        data.score.score = 70;
        data.compute_score_class();
        assert_eq!(data.score_class, "warning");
    }

    #[test]
    fn test_score_class_danger() {
        let mut data = RenderData::default();
        data.score.score = 50;
        data.compute_score_class();
        assert_eq!(data.score_class, "danger");
    }

    #[test]
    fn test_satd_stats_computation() {
        let mut data = RenderData {
            satd: Some(SATDData {
                items: vec![
                    SATDItem {
                        file: "test.rs".to_string(),
                        line: 1,
                        severity: "critical".to_string(),
                        category: "defect".to_string(),
                        content: "FIXME".to_string(),
                    },
                    SATDItem {
                        file: "test.rs".to_string(),
                        line: 2,
                        severity: "high".to_string(),
                        category: "defect".to_string(),
                        content: "TODO".to_string(),
                    },
                    SATDItem {
                        file: "test.rs".to_string(),
                        line: 3,
                        severity: "medium".to_string(),
                        category: "design".to_string(),
                        content: "HACK".to_string(),
                    },
                ],
            }),
            ..Default::default()
        };
        data.compute_satd_stats();

        let stats = data.satd_stats.unwrap();
        assert_eq!(stats.critical, 1);
        assert_eq!(stats.high, 1);
        assert_eq!(stats.medium, 1);
        assert_eq!(stats.low, 0);
        assert_eq!(stats.categories.get("defect"), Some(&2));
        assert_eq!(stats.categories.get("design"), Some(&1));
    }

    #[test]
    fn test_metadata_deserialize() {
        let json = r#"{
            "repository": "test/repo",
            "generated_at": "2024-01-15T10:30:00Z",
            "since": "6m",
            "omen_version": "4.0.0",
            "paths": ["/path/to/repo"]
        }"#;
        let metadata: Metadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.repository, "test/repo");
        assert_eq!(metadata.since, "6m");
    }

    #[test]
    fn test_churn_file_aliases() {
        let json = r#"{
            "relative_path": "src/main.rs",
            "commit_count": 42,
            "churn_score": 0.5
        }"#;
        let file: ChurnFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.file, "src/main.rs");
        assert_eq!(file.commits, 42);
    }

    #[test]
    fn test_hotspots_deserialize() {
        let json = r#"{
            "hotspots": [
                {
                    "file": "test.js",
                    "score": 0.99,
                    "commits": 10,
                    "avg_complexity": 93.0
                }
            ]
        }"#;
        let hotspots: HotspotsData = serde_json::from_str(json).unwrap();
        assert_eq!(hotspots.files.len(), 1);
        assert_eq!(hotspots.files[0].path, "test.js");
        assert!((hotspots.files[0].hotspot_score - 0.99).abs() < f64::EPSILON);
    }

    #[test]
    fn test_hotspots_deserialize_with_extra_fields() {
        // Test with actual JSON structure including extra fields like severity, percentiles
        let json = r#"{
            "hotspots": [
                {
                    "avg_complexity": 93.0,
                    "churn_percentile": 99.84,
                    "commits": 10,
                    "complexity_percentile": 99.78,
                    "file": "test.js",
                    "score": 0.99,
                    "severity": "Critical"
                }
            ]
        }"#;
        let hotspots: HotspotsData = serde_json::from_str(json).unwrap();
        assert_eq!(hotspots.files.len(), 1);
        assert_eq!(hotspots.files[0].path, "test.js");
        assert!((hotspots.files[0].hotspot_score - 0.99).abs() < f64::EPSILON);
        assert_eq!(hotspots.files[0].commits, 10);
    }

    #[test]
    fn test_temporal_deserialize() {
        let json = r#"{
            "generated_at": "2024-01-15T10:30:00Z",
            "period_days": 30,
            "min_cochanges": 3,
            "couplings": [
                {
                    "file_a": "src/main.rs",
                    "file_b": "src/lib.rs",
                    "cochange_count": 10,
                    "coupling_strength": 0.75,
                    "commits_a": 12,
                    "commits_b": 10
                }
            ],
            "summary": {
                "total_couplings": 1,
                "strong_couplings": 1,
                "avg_coupling_strength": 0.75,
                "max_coupling_strength": 0.75,
                "total_files_analyzed": 2
            }
        }"#;
        let data: TemporalData = serde_json::from_str(json).unwrap();
        assert_eq!(data.couplings.len(), 1);
        assert!((data.couplings[0].coupling_strength - 0.75).abs() < f64::EPSILON);
        assert_eq!(data.summary.strong_couplings, 1);
    }

    #[test]
    fn test_smells_deserialize() {
        let json = r#"{
            "generated_at": "2024-01-15T10:30:00Z",
            "smells": [
                {
                    "smell_type": "CyclicDependency",
                    "severity": "Critical",
                    "components": ["src/a.rs", "src/b.rs"],
                    "description": "Cyclic dependency detected",
                    "suggestion": "Break the cycle",
                    "metrics": { "cycle_length": 2 }
                }
            ],
            "components": [],
            "summary": {
                "total_smells": 1,
                "cyclic_count": 1,
                "hub_count": 0,
                "unstable_count": 0,
                "central_connector_count": 0,
                "critical_count": 1,
                "high_count": 0,
                "medium_count": 0,
                "total_components": 0,
                "average_instability": 0.0
            },
            "thresholds": {}
        }"#;
        let data: SmellsData = serde_json::from_str(json).unwrap();
        assert_eq!(data.smells.len(), 1);
        assert_eq!(data.smells[0].smell_type, "CyclicDependency");
        assert_eq!(data.summary.critical_count, 1);
    }

    #[test]
    fn test_graph_deserialize() {
        let json = r#"{
            "nodes": [
                {
                    "path": "src/main.rs",
                    "pagerank": 0.15,
                    "betweenness": 0.5,
                    "in_degree": 3,
                    "out_degree": 2,
                    "instability": 0.4
                }
            ],
            "edges": [{ "from": "src/main.rs", "to": "src/lib.rs" }],
            "cycles": [["src/a.rs", "src/b.rs"]],
            "summary": {
                "total_nodes": 1,
                "total_edges": 1,
                "avg_degree": 5.0,
                "cycle_count": 1
            }
        }"#;
        let data: GraphData = serde_json::from_str(json).unwrap();
        assert_eq!(data.nodes.len(), 1);
        assert_eq!(data.cycles.len(), 1);
        assert_eq!(data.summary.cycle_count, 1);
    }

    #[test]
    fn test_tdg_deserialize() {
        let json = r#"{
            "files": [
                {
                    "file_path": "src/main.rs",
                    "total": 72.5,
                    "grade": "B",
                    "structural_complexity": 15.0,
                    "semantic_complexity": 10.0,
                    "duplication_ratio": 5.0,
                    "coupling_score": 8.0,
                    "hotspot_score": 6.0,
                    "temporal_coupling_score": 3.0,
                    "has_critical_defects": false
                }
            ],
            "average_score": 72.5,
            "average_grade": "B",
            "total_files": 1,
            "grade_distribution": { "B": 1 }
        }"#;
        let data: TdgData = serde_json::from_str(json).unwrap();
        assert_eq!(data.files.len(), 1);
        assert!((data.average_score - 72.5).abs() < f32::EPSILON);
        assert_eq!(data.grade_distribution.get("B"), Some(&1));
    }

    #[test]
    fn test_flags_deserialize() {
        let json = r#"{
            "flags": [
                {
                    "key": "test_flag",
                    "provider": "feature",
                    "file_spread": 3,
                    "first_seen": "2020-01-01T00:00:00Z",
                    "stale": true,
                    "references": [
                        {"file": "test.rb", "line": 10}
                    ]
                }
            ]
        }"#;
        let flags: FlagsData = serde_json::from_str(json).unwrap();
        assert_eq!(flags.flags.len(), 1);
        assert_eq!(flags.flags[0].flag_key, "test_flag");
        assert_eq!(flags.flags[0].file_spread, Some(3));
        assert_eq!(flags.flags[0].stale, Some(true));
    }

    #[test]
    fn test_flags_deserialize_with_priority() {
        let json = r#"{
            "flags": [
                {
                    "key": "test_flag",
                    "provider": "feature",
                    "file_spread": 3,
                    "first_seen": "2020-01-01T00:00:00Z",
                    "stale": true,
                    "references": [{"file": "test.rb", "line": 10}],
                    "priority": {"level": "High", "score": 35.0}
                }
            ]
        }"#;
        let flags: FlagsData = serde_json::from_str(json).unwrap();
        assert_eq!(flags.flags[0].priority.level, "High");
        assert_eq!(flags.flags[0].priority.score, 35.0);
    }

    #[test]
    fn test_flags_deserialize_priority_defaults_when_missing() {
        let json = r#"{
            "flags": [
                {
                    "key": "test_flag",
                    "provider": "feature",
                    "references": []
                }
            ]
        }"#;
        let flags: FlagsData = serde_json::from_str(json).unwrap();
        assert_eq!(flags.flags[0].priority.level, "");
        assert_eq!(flags.flags[0].priority.score, 0.0);
    }
}
