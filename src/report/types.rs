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
    #[serde(default)]
    pub files: Vec<HotspotItem>,
    pub summary: Option<HotspotSummary>,
}

/// HotspotSummary contains aggregate hotspot stats.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotspotSummary {
    pub max_score: f64,
    pub avg_score: f64,
}

/// HotspotItem represents a single hotspot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotItem {
    pub path: String,
    pub hotspot_score: f64,
    pub commits: i32,
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
    pub files: i32,
}

/// OwnershipFile represents a single file's ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipFile {
    pub file: String,
}

/// OwnerInfo represents a code owner's stats for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerInfo {
    pub name: String,
    pub files_owned: i32,
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
    pub flag_key: String,
    pub provider: String,
    pub priority: FlagPriority,
    pub complexity: FlagComplexity,
    pub staleness: FlagStaleness,
    #[serde(default)]
    pub references: Vec<FlagReference>,
}

/// FlagReference represents a single occurrence of a feature flag in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagReference {
    pub file: String,
    pub line: u32,
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
    pub component_trends: HashMap<String, ComponentTrendStats>,
    pub satd_stats: Option<SATDStats>,
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
}
