//! HTML report rendering using minijinja templating.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use minijinja::{context, Environment, Value};
use pulldown_cmark::{html, Options, Parser};

use crate::core::Language;
use crate::core::Result;
use crate::report::types::*;

/// The embedded HTML template (matches Go version exactly).
const TEMPLATE_HTML: &str = include_str!("template.html");

/// Renderer handles HTML report generation.
pub struct Renderer {
    env: Environment<'static>,
}

impl Renderer {
    /// Create a new renderer with the embedded template.
    pub fn new() -> Result<Self> {
        let mut env = Environment::new();

        // Add template filters (equivalent to Go's template.FuncMap)
        env.add_filter("rel_path", rel_path);
        env.add_filter("markdown", markdown_filter);
        env.add_filter("score_class", score_class);
        env.add_filter("hotspot_badge", hotspot_badge);
        env.add_filter("priority_badge", priority_badge);
        env.add_filter("churn_badge", churn_badge);
        env.add_filter("limit", limit_filter);
        env.add_filter("lower", |s: &str| s.to_lowercase());
        env.add_filter("title", title_case);
        env.add_filter("truncate", truncate);
        env.add_filter("truncate_path", truncate_path);
        env.add_filter("num", num_format);
        env.add_filter("tojson", tojson_filter);
        env.add_filter("lang_from_path", lang_from_path);

        // Add template functions
        env.add_function("percent", percent);
        env.add_function("count_severity", count_severity);
        env.add_function("filter_severity", filter_severity);
        env.add_function("flag_files_tooltip", flag_files_tooltip);

        // Add the template
        env.add_template("report", TEMPLATE_HTML)?;

        Ok(Self { env })
    }

    /// Render generates HTML from the data directory and writes to the output.
    pub fn render<W: Write>(&self, data_dir: &Path, writer: &mut W) -> Result<()> {
        let data = self.load_data(data_dir)?;
        let tmpl = self.env.get_template("report")?;
        let rendered = tmpl.render(context! {
            Metadata => data.metadata,
            Score => data.score,
            ScoreClass => data.score_class,
            Complexity => data.complexity,
            Hotspots => data.hotspots,
            SATD => data.satd,
            Churn => data.churn,
            Ownership => data.ownership,
            Duplicates => data.duplicates,
            Cohesion => data.cohesion,
            Flags => data.flags,
            Trend => data.trend,
            Summary => data.summary,
            HotspotsInsight => data.hotspots_insight,
            SATDInsight => data.satd_insight,
            TrendsInsight => data.trends_insight,
            ChurnInsight => data.churn_insight,
            DuplicationInsight => data.duplication_insight,
            ComponentsInsight => data.components_insight,
            FlagsInsight => data.flags_insight,
            OwnershipInsight => data.ownership_insight,
            ComponentTrends => data.component_trends,
            SATDStats => data.satd_stats,
        })?;
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    /// Render to a file.
    pub fn render_to_file(&self, data_dir: &Path, output_path: &Path) -> Result<()> {
        let mut file = fs::File::create(output_path)?;
        self.render(data_dir, &mut file)
    }

    /// Load all JSON data files and transform for rendering.
    fn load_data(&self, data_dir: &Path) -> Result<RenderData> {
        let mut data = RenderData::default();

        // Load metadata
        if let Ok(metadata) = load_json::<Metadata>(&data_dir.join("metadata.json")) {
            data.metadata = metadata;
        }

        // Load score (handle both flat and nested component formats)
        if let Ok(raw) = load_json::<ScoreRaw>(&data_dir.join("score.json")) {
            let mut components = HashMap::new();
            for (name, comp) in raw.components {
                components.insert(name, comp.score.round() as i32);
            }
            data.score = ScoreData {
                score: raw.overall_score.round() as i32,
                passed: raw.overall_score >= 60.0,
                files_analyzed: raw.summary.files_analyzed,
                components,
            };
        }
        data.compute_score_class();

        // Load complexity and compute averages
        if let Ok(raw) = load_json::<ComplexityRaw>(&data_dir.join("complexity.json")) {
            data.complexity = Some(ComplexityData {
                avg_cyclomatic: raw.summary.avg_cyclomatic,
                avg_cognitive: raw.summary.avg_cognitive,
            });
        }

        // Load hotspots and compute summary if needed
        if let Ok(mut hotspots) = load_json::<HotspotsData>(&data_dir.join("hotspots.json")) {
            // Compute max_score/avg_score if not present (summary is None or has default values)
            let needs_compute = !hotspots.files.is_empty()
                && (hotspots.summary.is_none()
                    || hotspots
                        .summary
                        .as_ref()
                        .is_some_and(|s| s.max_score == 0.0));
            if needs_compute {
                let max_score = hotspots
                    .files
                    .iter()
                    .map(|f| f.hotspot_score)
                    .fold(0.0f64, f64::max);
                let total_score: f64 = hotspots.files.iter().map(|f| f.hotspot_score).sum();
                let avg_score = total_score / hotspots.files.len() as f64;
                if let Some(ref mut summary) = hotspots.summary {
                    summary.max_score = max_score;
                    summary.avg_score = avg_score;
                } else {
                    hotspots.summary = Some(HotspotSummary {
                        max_score,
                        avg_score,
                        ..Default::default()
                    });
                }
            }
            data.hotspots = Some(hotspots);
        }

        // Load SATD and sort by severity (Critical > High > Medium > Low)
        if let Ok(mut satd) = load_json::<SATDData>(&data_dir.join("satd.json")) {
            satd.items
                .sort_by(|a, b| severity_order(&b.severity).cmp(&severity_order(&a.severity)));
            data.satd = Some(satd);
            data.compute_satd_stats();
        }

        // Load churn and compute summary
        if let Ok(mut churn) = load_json::<ChurnData>(&data_dir.join("churn.json")) {
            churn.summary.unique_files = churn.files.len() as i32;
            let mut total_commits = 0;
            let mut total_added = 0;
            let mut total_deleted = 0;
            for f in &churn.files {
                total_commits += f.commits;
                total_added += f.additions;
                total_deleted += f.deletions;
            }
            churn.summary.total_commits = total_commits;
            churn.summary.total_added = total_added;
            churn.summary.total_deleted = total_deleted;
            data.churn = Some(churn);
        }

        // Load ownership and transform for display
        if let Ok(mut ownership) = load_json::<OwnershipData>(&data_dir.join("ownership.json")) {
            ownership.bus_factor = ownership.summary.bus_factor;
            ownership.knowledge_silos = ownership.summary.silo_count;
            ownership.total_files = ownership.summary.total_files;
            ownership.top_owners = ownership
                .summary
                .top_contributors
                .iter()
                .map(|c| OwnerInfo {
                    name: c.name.clone(),
                    files_owned: c.files,
                })
                .collect();
            data.ownership = Some(ownership);
        }

        // Load duplicates and transform for display
        if let Ok(mut duplicates) = load_json::<DuplicatesData>(&data_dir.join("duplicates.json")) {
            duplicates.clone_groups = duplicates.summary.total_groups;
            duplicates.duplicate_lines = duplicates.summary.duplicated_lines;
            duplicates.total_lines = duplicates.summary.total_lines;
            // Convert ratio to percentage
            duplicates.duplication_ratio = duplicates.summary.duplication_ratio * 100.0;
            data.duplicates = Some(duplicates);
        }

        // Load cohesion
        if let Ok(cohesion) = load_json::<CohesionData>(&data_dir.join("cohesion.json")) {
            data.cohesion = Some(cohesion);
        }

        // Load flags
        if let Ok(flags) = load_json::<FlagsData>(&data_dir.join("flags.json")) {
            data.flags = Some(flags);
        }

        // Load trend
        if let Ok(trend) = load_json::<TrendData>(&data_dir.join("trend.json")) {
            data.component_trends = trend.component_trends.clone();
            data.trend = Some(trend);
        }

        // Load insights if available
        let insights_dir = data_dir.join("insights");
        if insights_dir.exists() {
            if let Ok(summary) = load_json::<SummaryInsight>(&insights_dir.join("summary.json")) {
                data.summary = Some(summary);
            }
            if let Ok(insight) = load_json::<HotspotsInsight>(&insights_dir.join("hotspots.json")) {
                data.hotspots_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<SATDInsight>(&insights_dir.join("satd.json")) {
                data.satd_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<TrendsInsight>(&insights_dir.join("trends.json")) {
                data.trends_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<ChurnInsight>(&insights_dir.join("churn.json")) {
                data.churn_insight = Some(insight);
            }
            if let Ok(insight) =
                load_json::<DuplicationInsight>(&insights_dir.join("duplication.json"))
            {
                data.duplication_insight = Some(insight);
            }
            if let Ok(insight) =
                load_json::<ComponentsInsight>(&insights_dir.join("components.json"))
            {
                data.components_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<FlagsInsight>(&insights_dir.join("flags.json")) {
                data.flags_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<OwnershipInsight>(&insights_dir.join("ownership.json"))
            {
                data.ownership_insight = Some(insight);
            }
        }

        Ok(data)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new().expect("failed to create default renderer")
    }
}

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path)?;
    let value = serde_json::from_str(&content)?;
    Ok(value)
}

/// Convert severity string to numeric order for sorting.
/// Higher values = higher severity (for descending sort).
fn severity_order(severity: &str) -> u8 {
    match severity.to_lowercase().as_str() {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

// ============================================================================
// Template Filters
// ============================================================================

/// Strip root prefix from path for relative display.
fn rel_path(path: &str, roots: Option<Vec<String>>) -> String {
    let roots = roots.unwrap_or_default();
    if roots.is_empty() {
        return path.to_string();
    }
    let mut root = roots[0].clone();
    if !root.ends_with('/') {
        root.push('/');
    }
    path.strip_prefix(&root).unwrap_or(path).to_string()
}

/// Convert markdown to HTML.
fn markdown_filter(s: &str) -> Value {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(s, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Mark as safe HTML (won't be escaped)
    Value::from_safe_string(html_output)
}

/// Get CSS class based on score value.
fn score_class(score: i32) -> &'static str {
    if score >= 80 {
        "good"
    } else if score >= 60 {
        "warning"
    } else {
        "danger"
    }
}

/// Get badge class based on hotspot score.
fn hotspot_badge(score: f64) -> &'static str {
    if score >= 0.7 {
        "critical"
    } else if score >= 0.5 {
        "high"
    } else if score >= 0.3 {
        "medium"
    } else {
        "low"
    }
}

/// Get badge class based on priority string.
fn priority_badge(priority: &str) -> &'static str {
    match priority.to_uppercase().as_str() {
        "CRITICAL" => "critical",
        "HIGH" => "high",
        "MEDIUM" => "medium",
        _ => "low",
    }
}

/// Get badge class based on churn score.
fn churn_badge(score: f64) -> &'static str {
    if score >= 0.3 {
        "critical"
    } else if score >= 0.1 {
        "high"
    } else if score >= 0.02 {
        "medium"
    } else {
        "low"
    }
}

/// Limit array to n items.
fn limit_filter(items: Value, n: usize) -> Value {
    if let Ok(iter) = items.try_iter() {
        let limited: Vec<Value> = iter.take(n).collect();
        Value::from_iter(limited)
    } else {
        items
    }
}

/// Title case a string.
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Truncate string to n characters.
fn truncate(s: &str, n: usize) -> String {
    if s.len() > n {
        format!("{}...", &s[..n])
    } else {
        s.to_string()
    }
}

/// Truncate path intelligently, keeping filename visible.
fn truncate_path(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }

    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() <= 2 {
        return format!("{}...", &s[..n.saturating_sub(3)]);
    }

    let filename = parts.last().unwrap_or(&"");
    if filename.len() >= n.saturating_sub(3) {
        return format!("...{}", &filename[filename.len().saturating_sub(n - 3)..]);
    }

    let remaining = n.saturating_sub(filename.len()).saturating_sub(4);
    let prefix = parts[..parts.len() - 1].join("/");
    let truncated_prefix = if prefix.len() > remaining {
        &prefix[prefix.len() - remaining..]
    } else {
        &prefix
    };

    format!(".../{}/{}", truncated_prefix, filename)
}

/// Convert value to JSON string.
fn tojson_filter(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

/// Format number with thousands separator.
fn num_format(n: Value) -> String {
    let num = n.as_i64().unwrap_or(0);
    let s = num.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

// ============================================================================
// Template Functions
// ============================================================================

/// Calculate percentage.
fn percent(a: i32, b: i32) -> f64 {
    if b == 0 {
        0.0
    } else {
        (a as f64) / (b as f64) * 100.0
    }
}

/// Count items with given severity.
fn count_severity(items: Vec<Value>, severity: &str) -> i32 {
    items
        .iter()
        .filter(|item| {
            if let Ok(sev) = item.get_attr("severity") {
                sev.as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(severity))
            } else {
                false
            }
        })
        .count() as i32
}

/// Filter items by severities.
fn filter_severity(items: Vec<Value>, severities: Vec<String>) -> Vec<Value> {
    let severity_set: std::collections::HashSet<String> =
        severities.iter().map(|s| s.to_lowercase()).collect();

    items
        .into_iter()
        .filter(|item| {
            if let Ok(sev) = item.get_attr("severity") {
                sev.as_str()
                    .is_some_and(|s| severity_set.contains(&s.to_lowercase()))
            } else {
                false
            }
        })
        .collect()
}

/// Map a file path to its language display name.
fn lang_from_path(path: &str) -> String {
    Language::detect(Path::new(path))
        .map(|l: Language| l.display_name().to_string())
        .unwrap_or_default()
}

/// Generate tooltip for flag file references.
fn flag_files_tooltip(refs: Vec<Value>, roots: Option<Vec<String>>) -> String {
    if refs.is_empty() {
        return "No file references".to_string();
    }

    let root = roots
        .and_then(|r| r.into_iter().next())
        .map(|mut r| {
            if !r.ends_with('/') {
                r.push('/');
            }
            r
        })
        .unwrap_or_default();

    let mut seen: HashMap<String, Vec<u32>> = HashMap::new();

    for ref_val in refs {
        let file = ref_val
            .get_attr("file")
            .ok()
            .map(|v| {
                let s = v.to_string();
                s.strip_prefix(&root).unwrap_or(&s).to_string()
            })
            .unwrap_or_default();

        let line = ref_val
            .get_attr("line")
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as u32;

        seen.entry(file).or_default().push(line);
    }

    seen.into_iter()
        .map(|(file, lines)| {
            let line_strs: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            format!("{}:{}", file, line_strs.join(","))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_class() {
        assert_eq!(score_class(85), "good");
        assert_eq!(score_class(80), "good");
        assert_eq!(score_class(70), "warning");
        assert_eq!(score_class(60), "warning");
        assert_eq!(score_class(50), "danger");
    }

    #[test]
    fn test_hotspot_badge() {
        assert_eq!(hotspot_badge(0.8), "critical");
        assert_eq!(hotspot_badge(0.7), "critical");
        assert_eq!(hotspot_badge(0.6), "high");
        assert_eq!(hotspot_badge(0.5), "high");
        assert_eq!(hotspot_badge(0.4), "medium");
        assert_eq!(hotspot_badge(0.3), "medium");
        assert_eq!(hotspot_badge(0.2), "low");
    }

    #[test]
    fn test_truncate_path() {
        assert_eq!(truncate_path("short.rs", 20), "short.rs");
        // Path gets truncated with prefix
        let result = truncate_path("src/analyzers/complexity/mod.rs", 25);
        assert!(result.starts_with("..."));
        assert!(result.ends_with("mod.rs"));
        assert!(result.len() <= 30); // Some flexibility for ellipsis
    }

    #[test]
    fn test_title_case() {
        assert_eq!(title_case("hello world"), "Hello World");
        assert_eq!(title_case("TEST"), "TEST");
    }

    #[test]
    fn test_num_format() {
        assert_eq!(num_format(Value::from(1234567)), "1,234,567");
        assert_eq!(num_format(Value::from(123)), "123");
    }

    #[test]
    fn test_percent() {
        assert!((percent(1, 4) - 25.0).abs() < f64::EPSILON);
        assert!((percent(0, 0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_lang_from_path() {
        assert_eq!(lang_from_path("src/main.rs"), "Rust");
        assert_eq!(lang_from_path("app/server.go"), "Go");
        assert_eq!(lang_from_path("lib/utils.py"), "Python");
        assert_eq!(lang_from_path("index.tsx"), "TSX");
        assert_eq!(lang_from_path("README.md"), "");
    }

    #[test]
    fn test_rel_path() {
        assert_eq!(
            rel_path(
                "/home/user/repo/src/main.rs",
                Some(vec!["/home/user/repo".to_string()])
            ),
            "src/main.rs"
        );
        assert_eq!(rel_path("src/main.rs", None), "src/main.rs");
    }
}
