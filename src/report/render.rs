//! HTML report rendering using minijinja templating.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
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
        env.add_filter("coupling_badge", coupling_badge);
        env.add_filter("grade_class", grade_class);
        env.add_filter("smell_type_label", smell_type_label);
        env.add_filter("instability_class", instability_class);

        // Add template functions
        env.add_function("percent", percent);
        env.add_function("count_severity", count_severity);
        env.add_function("filter_severity", filter_severity);
        env.add_function("flag_files_tooltip", flag_files_tooltip);

        // Add the template
        env.add_template("report", TEMPLATE_HTML)?;

        Ok(Self { env })
    }

    /// Render generates HTML from the data directory into a byte buffer.
    fn render_to_bytes(&self, data_dir: &Path) -> Result<Vec<u8>> {
        let data = self.load_data(data_dir)?;
        let roots = &data.metadata.paths;

        let hotspots_json = data
            .hotspots
            .as_ref()
            .map(|h| build_hotspots_json(&h.files, roots));
        let satd_json = data.satd.as_ref().map(|s| build_satd_json(&s.items, roots));
        let churn_json = data.churn.as_ref().map(|c| build_churn_json(&c.files));
        let cohesion_json = data
            .cohesion
            .as_ref()
            .map(|c| build_cohesion_json(&c.classes));
        let graph_json = data
            .graph
            .as_ref()
            .map(|g| build_graph_json(&g.nodes, roots));
        let tdg_json = data.tdg.as_ref().map(|t| build_tdg_json(&t.files, roots));
        let temporal_json = data
            .temporal
            .as_ref()
            .map(|t| build_temporal_json(&t.couplings, roots));

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
            Temporal => data.temporal,
            TemporalInsight => data.temporal_insight,
            Smells => data.smells,
            SmellsInsight => data.smells_insight,
            Graph => data.graph,
            GraphInsight => data.graph_insight,
            Tdg => data.tdg,
            TdgInsight => data.tdg_insight,
            ComponentTrends => data.component_trends,
            SATDStats => data.satd_stats,
            HotspotsTableJson => hotspots_json,
            SATDTableJson => satd_json,
            ChurnTableJson => churn_json,
            CohesionTableJson => cohesion_json,
            GraphTableJson => graph_json,
            TdgTableJson => tdg_json,
            TemporalTableJson => temporal_json,
        })?;

        let minified = minify_html_output(rendered.as_bytes());
        Ok(minified)
    }

    /// Render generates HTML from the data directory and writes to the output.
    pub fn render<W: Write>(&self, data_dir: &Path, writer: &mut W) -> Result<()> {
        let output = self.render_to_bytes(data_dir)?;
        writer.write_all(&output)?;
        Ok(())
    }

    /// Render to a file, also producing a `.html.gz` companion.
    pub fn render_to_file(&self, data_dir: &Path, output_path: &Path) -> Result<()> {
        let output = self.render_to_bytes(data_dir)?;

        fs::write(output_path, &output)?;

        let gz_path = output_path.with_extension("html.gz");
        let gz_file = fs::File::create(&gz_path)?;
        let mut encoder = GzEncoder::new(gz_file, Compression::best());
        encoder.write_all(&output)?;
        encoder.finish()?;

        Ok(())
    }

    /// Return the gzip companion path for a given output path.
    pub fn gz_path(output_path: &Path) -> std::path::PathBuf {
        output_path.with_extension("html.gz")
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
                    files_owned: c.files_owned,
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

        // Load flags (priorities computed by the flags analyzer)
        if let Ok(flags) = load_json::<FlagsData>(&data_dir.join("flags.json")) {
            data.flags = Some(flags);
        }

        // Load temporal coupling
        if let Ok(temporal) = load_json::<TemporalData>(&data_dir.join("temporal.json")) {
            data.temporal = Some(temporal);
        }

        // Load architectural smells
        if let Ok(smells) = load_json::<SmellsData>(&data_dir.join("smells.json")) {
            data.smells = Some(smells);
        }

        // Load dependency graph
        if let Ok(graph) = load_json::<GraphData>(&data_dir.join("graph.json")) {
            data.graph = Some(graph);
        }

        // Load TDG (technical debt gradient) and sort by score ascending (worst first)
        if let Ok(mut tdg) = load_json::<TdgData>(&data_dir.join("tdg.json")) {
            tdg.files.sort_by(|a, b| {
                a.total
                    .partial_cmp(&b.total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            // Normalize grade_distribution keys from Debug format ("APlus") to display ("A+").
            // The grade_distribution HashMap uses format!("{:?}", grade) which gives variant
            // names, while average_grade and file grades use serde rename ("A+").
            tdg.grade_distribution = tdg
                .grade_distribution
                .into_iter()
                .map(|(k, v)| (normalize_grade_key(&k), v))
                .collect();
            data.tdg = Some(tdg);
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
            if let Ok(insight) = load_json::<TemporalInsight>(&insights_dir.join("temporal.json")) {
                data.temporal_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<SmellsInsight>(&insights_dir.join("smells.json")) {
                data.smells_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<GraphInsight>(&insights_dir.join("graph.json")) {
                data.graph_insight = Some(insight);
            }
            if let Ok(insight) = load_json::<TdgInsight>(&insights_dir.join("tdg.json")) {
                data.tdg_insight = Some(insight);
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

/// Normalize TDG grade keys from Debug format to display format.
/// "APlus" -> "A+", "AMinus" -> "A-", "B" -> "B", etc.
fn normalize_grade_key(key: &str) -> String {
    match key {
        "APlus" => "A+".to_string(),
        "AMinus" => "A-".to_string(),
        "BPlus" => "B+".to_string(),
        "BMinus" => "B-".to_string(),
        "CPlus" => "C+".to_string(),
        "CMinus" => "C-".to_string(),
        other => other.to_string(),
    }
}

/// Get badge class based on temporal coupling strength.
fn coupling_badge(strength: f64) -> &'static str {
    if strength >= 0.5 {
        "critical"
    } else if strength >= 0.3 {
        "high"
    } else if strength >= 0.1 {
        "medium"
    } else {
        "low"
    }
}

/// Get CSS class based on TDG grade letter.
fn grade_class(grade: &str) -> &'static str {
    match grade.chars().next() {
        Some('A') => "good",
        Some('B') => "good",
        Some('C') => "warning",
        Some('D') => "warning",
        _ => "danger",
    }
}

/// Convert SmellType enum name to human-readable label.
fn smell_type_label(s: &str) -> String {
    match s {
        "CyclicDependency" => "Cyclic Dependency".to_string(),
        "UnstableDependency" => "Unstable Dependency".to_string(),
        "Hub" | "HubLikeDependency" => "Hub Dependency".to_string(),
        "CentralConnector" | "GodComponent" | "GodClass" => "Central Connector".to_string(),
        "FeatureEnvy" => "Feature Envy".to_string(),
        _ => s.to_string(),
    }
}

/// Get CSS class based on instability value.
fn instability_class(instability: f64) -> &'static str {
    if instability >= 0.7 {
        "danger"
    } else if instability >= 0.3 {
        "warning"
    } else {
        "good"
    }
}

// ============================================================================
// HTML Minification
// ============================================================================

fn minify_html_output(input: &[u8]) -> Vec<u8> {
    let cfg = minify_html::Cfg {
        minify_js: true,
        minify_css: true,
        ..Default::default()
    };
    minify_html::minify(input, &cfg)
}

// ============================================================================
// JSON Table Data Builders
// ============================================================================

/// Serialize rows as a JSON array-of-arrays string for simple-datatables `data` option.
fn rows_to_json(rows: &[Vec<String>]) -> String {
    serde_json::to_string(rows).unwrap_or_else(|_| "[]".to_string())
}

fn build_hotspots_json(files: &[HotspotItem], roots: &[String]) -> String {
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|item| {
            let path = rel_path(&item.path, Some(roots.to_vec()));
            let lang = lang_from_path(&item.path);
            let score = item.hotspot_score;
            let badge = hotspot_badge(score);
            vec![
                format!("<code>{}</code>", html_escape(&truncate_path(&path, 60))),
                lang,
                format!("<span class=\"badge {badge}\">{:.3}</span>", score),
                item.commits.to_string(),
                format!("{:.1}", item.avg_cognitive),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_satd_json(items: &[SATDItem], roots: &[String]) -> String {
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            let sev_lower = item.severity.to_lowercase();
            let path = rel_path(&item.file, Some(roots.to_vec()));
            let lang = lang_from_path(&item.file);
            vec![
                format!(
                    "<span class=\"badge {sev_lower}\">{}</span>",
                    html_escape(&item.severity)
                ),
                html_escape(&item.category),
                format!("<code>{}</code>", html_escape(&truncate_path(&path, 50))),
                lang,
                item.line.to_string(),
                html_escape(&item.content),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_churn_json(files: &[ChurnFile]) -> String {
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|item| {
            let lang = lang_from_path(&item.file);
            let badge = churn_badge(item.churn_score);
            vec![
                format!("<code>{}</code>", html_escape(&item.file)),
                lang,
                item.commits.to_string(),
                item.authors.len().to_string(),
                format!(
                    "<span class=\"badge {badge}\">{:.3}</span>",
                    item.churn_score
                ),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_cohesion_json(classes: &[CohesionClass]) -> String {
    let rows: Vec<Vec<String>> = classes
        .iter()
        .map(|item| {
            let lcom_class = if item.lcom > 50 {
                "critical"
            } else if item.lcom > 20 {
                "high"
            } else {
                "medium"
            };
            vec![
                format!("<code>{}</code>", html_escape(&item.class_name)),
                html_escape(&truncate_path(&item.path, 40)),
                html_escape(&item.language),
                format!("<span class=\"badge {lcom_class}\">{}</span>", item.lcom),
                item.wmc.to_string(),
                item.cbo.to_string(),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_graph_json(nodes: &[GraphNode], roots: &[String]) -> String {
    let rows: Vec<Vec<String>> = nodes
        .iter()
        .map(|node| {
            let path = rel_path(&node.path, Some(roots.to_vec()));
            let lang = lang_from_path(&node.path);
            let inst_class = instability_class(node.instability);
            vec![
                format!("<code>{}</code>", html_escape(&truncate_path(&path, 50))),
                lang,
                format!("{:.4}", node.pagerank),
                format!("{:.4}", node.betweenness),
                node.in_degree.to_string(),
                node.out_degree.to_string(),
                format!(
                    "<span class=\"{inst_class}\">{:.2}</span>",
                    node.instability
                ),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_tdg_json(files: &[TdgFile], roots: &[String]) -> String {
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|item| {
            let path = rel_path(&item.file_path, Some(roots.to_vec()));
            let lang = lang_from_path(&item.file_path);
            let score_class_val = score_class(item.total.round() as i32);
            let grade_class_val = grade_class(&item.grade);
            vec![
                format!("<code>{}</code>", html_escape(&truncate_path(&path, 50))),
                lang,
                format!("<span class=\"{score_class_val}\">{:.1}</span>", item.total),
                format!(
                    "<span class=\"badge {grade_class_val}\">{}</span>",
                    html_escape(&item.grade)
                ),
                format!("{:.1}", item.structural_complexity),
                format!("{:.1}", item.semantic_complexity),
                format!("{:.1}", item.duplication_ratio),
                format!("{:.1}", item.coupling_score),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn build_temporal_json(couplings: &[TemporalCoupling], roots: &[String]) -> String {
    let rows: Vec<Vec<String>> = couplings
        .iter()
        .map(|item| {
            let path_a = rel_path(&item.file_a, Some(roots.to_vec()));
            let path_b = rel_path(&item.file_b, Some(roots.to_vec()));
            let badge = coupling_badge(item.coupling_strength);
            vec![
                format!("<code>{}</code>", html_escape(&truncate_path(&path_a, 40))),
                format!("<code>{}</code>", html_escape(&truncate_path(&path_b, 40))),
                item.cochange_count.to_string(),
                format!(
                    "<span class=\"badge {badge}\">{:.2}</span>",
                    item.coupling_strength
                ),
            ]
        })
        .collect();
    rows_to_json(&rows)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
    fn test_coupling_badge() {
        assert_eq!(coupling_badge(0.6), "critical");
        assert_eq!(coupling_badge(0.5), "critical");
        assert_eq!(coupling_badge(0.4), "high");
        assert_eq!(coupling_badge(0.3), "high");
        assert_eq!(coupling_badge(0.2), "medium");
        assert_eq!(coupling_badge(0.1), "medium");
        assert_eq!(coupling_badge(0.05), "low");
    }

    #[test]
    fn test_grade_class() {
        assert_eq!(grade_class("A+"), "good");
        assert_eq!(grade_class("A"), "good");
        assert_eq!(grade_class("B"), "good");
        assert_eq!(grade_class("C"), "warning");
        assert_eq!(grade_class("D"), "warning");
        assert_eq!(grade_class("F"), "danger");
    }

    #[test]
    fn test_smell_type_label() {
        assert_eq!(smell_type_label("CyclicDependency"), "Cyclic Dependency");
        assert_eq!(smell_type_label("Hub"), "Hub Dependency");
        assert_eq!(smell_type_label("CentralConnector"), "Central Connector");
        assert_eq!(smell_type_label("Unknown"), "Unknown");
    }

    #[test]
    fn test_instability_class() {
        assert_eq!(instability_class(0.8), "danger");
        assert_eq!(instability_class(0.5), "warning");
        assert_eq!(instability_class(0.2), "good");
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
