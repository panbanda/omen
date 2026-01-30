//! Score trend analysis over git history.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rayon::prelude::*;

use crate::cli::TrendPeriod;
use crate::config::Config;
use crate::core::{
    AnalysisContext, Analyzer as AnalyzerTrait, ContentSource, Error, FileSet, Result, TreeSource,
};
use crate::git::{Commit, GitRepo};
use crate::report::{ComponentTrendStats, TrendData, TrendPoint};

use super::Analyzer as ScoreAnalyzer;

/// Analyze score trends over time by iterating through git history.
/// Uses parallel worktree analysis for performance.
pub fn analyze_trend(
    path: &Path,
    config: &Config,
    since: &str,
    period: TrendPeriod,
    samples: Option<usize>,
) -> Result<TrendData> {
    let repo = GitRepo::open(path)?;
    let now = Utc::now();

    // Parse the "since" parameter to determine how far back to go
    let start_time = parse_since_to_datetime(since, now)?;

    // If an explicit sample count is given, divide the range into evenly-spaced
    // intervals. Otherwise fall back to the period-based fixed interval.
    let interval = if let Some(n) = samples {
        let n = n.max(2);
        let total_seconds = (now - start_time).num_seconds();
        Duration::seconds(total_seconds / (n as i64 - 1))
    } else {
        match period {
            TrendPeriod::Daily => Duration::days(1),
            TrendPeriod::Weekly => Duration::days(7),
            TrendPeriod::Monthly => Duration::days(30),
        }
    };

    // Get commits in the time range
    let since_arg = if crate::git::is_since_all(since) {
        None
    } else {
        Some(since)
    };
    let commits = repo.log(since_arg, None, None)?;
    if commits.is_empty() {
        return Ok(TrendData::default());
    }

    // Build list of commits to analyze at each sample point
    let mut sample_commits: Vec<(DateTime<Utc>, String)> = Vec::new();
    let mut current_time = start_time;

    while current_time <= now {
        if let Some(commit) = find_commit_at_time(&commits, current_time) {
            // Avoid duplicate commits (same commit for multiple time points)
            if sample_commits
                .last()
                .map(|(_, sha)| sha != &commit.sha)
                .unwrap_or(true)
            {
                sample_commits.push((current_time, commit.sha.clone()));
            }
        }
        current_time += interval;
    }

    // Analyze commits in parallel using worktrees
    let points = analyze_commits_parallel(path, config, &sample_commits, &commits)?;

    // Always include the current HEAD if not already included
    let mut final_points = points;
    let head_date = now.format("%Y-%m-%d").to_string();
    if final_points.last().map(|p| &p.date) != Some(&head_date) {
        if let Ok(score_data) = analyze_current(path, config) {
            let last_ts = final_points
                .last()
                .and_then(|p| chrono::NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").ok())
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc().timestamp())
                .unwrap_or(0);
            let head_commits = collect_commits_in_range(&commits, last_ts, now.timestamp());
            final_points.push(TrendPoint {
                date: head_date,
                score: score_data.overall_score as i32,
                components: score_data
                    .components
                    .iter()
                    .map(|(k, v)| (k.clone(), v.score as i32))
                    .collect(),
                notable_commits: head_commits,
            });
        }
    }

    // Calculate linear regression for overall score
    let (slope, intercept, r_squared) = if final_points.len() >= 2 {
        calculate_linear_regression(&final_points)
    } else {
        (0.0, 0.0, 0.0)
    };

    // Calculate component trends
    let component_trends = calculate_component_trends(&final_points);

    let start_score = final_points.first().map(|p| p.score).unwrap_or(0);
    let end_score = final_points.last().map(|p| p.score).unwrap_or(0);

    Ok(TrendData {
        points: final_points,
        slope,
        intercept,
        r_squared,
        start_score,
        end_score,
        component_trends,
    })
}

/// Analyze multiple commits in parallel using TreeSource (no worktrees needed).
/// Reads file contents directly from git's object store without filesystem checkout.
fn analyze_commits_parallel(
    path: &Path,
    config: &Config,
    sample_commits: &[(DateTime<Utc>, String)],
    all_commits: &[Commit],
) -> Result<Vec<TrendPoint>> {
    if sample_commits.is_empty() {
        return Ok(Vec::new());
    }

    let total = sample_commits.len();
    eprintln!(
        "Trend analysis: analyzing {} commits using tree-based analysis",
        total
    );

    // Build time windows for commit message collection.
    // Each sample point gets messages from (previous_sample_time, current_sample_time].
    let windows: Vec<(i64, i64)> = sample_commits
        .iter()
        .enumerate()
        .map(|(i, (time, _))| {
            let start = if i == 0 {
                0
            } else {
                sample_commits[i - 1].0.timestamp()
            };
            (start, time.timestamp())
        })
        .collect();

    let completed = Arc::new(AtomicUsize::new(0));
    let path_buf = path.to_path_buf();

    // Analyze all commits in parallel using TreeSource
    let all_points: Vec<TrendPoint> = sample_commits
        .par_iter()
        .zip(windows.par_iter())
        .filter_map(|((time, sha), &(window_start, window_end))| {
            // Create TreeSource for this commit
            let tree_source = TreeSource::new(&path_buf, sha).ok()?;
            let result = analyze_at_tree(&tree_source, config).ok()?;

            // Update progress
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if done.is_multiple_of(10) || done == total {
                eprintln!("Trend analysis: {}/{} commits analyzed", done, total);
            }

            let notable = collect_commits_in_range(all_commits, window_start, window_end);

            Some(TrendPoint {
                date: time.format("%Y-%m-%d").to_string(),
                score: result.overall_score as i32,
                components: result
                    .components
                    .iter()
                    .map(|(k, v)| (k.clone(), v.score as i32))
                    .collect(),
                notable_commits: notable,
            })
        })
        .collect();

    // Sort by date
    let mut sorted_points = all_points;
    sorted_points.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(sorted_points)
}

/// Analyze commits sequentially using TreeSource (for debugging or when parallelism fails).
#[allow(dead_code)]
fn analyze_commits_sequential(
    path: &Path,
    config: &Config,
    sample_commits: &[(DateTime<Utc>, String)],
    all_commits: &[Commit],
) -> Result<Vec<TrendPoint>> {
    let mut points = Vec::new();
    let mut prev_ts: i64 = 0;

    for (time, sha) in sample_commits {
        if let Ok(tree_source) = TreeSource::new(path, sha) {
            if let Ok(score_data) = analyze_at_tree(&tree_source, config) {
                let notable = collect_commits_in_range(all_commits, prev_ts, time.timestamp());
                points.push(TrendPoint {
                    date: time.format("%Y-%m-%d").to_string(),
                    score: score_data.overall_score as i32,
                    components: score_data
                        .components
                        .iter()
                        .map(|(k, v)| (k.clone(), v.score as i32))
                        .collect(),
                    notable_commits: notable,
                });
                prev_ts = time.timestamp();
            }
        }
    }

    Ok(points)
}

/// Compute a default sample count from the number of days in the time range.
///
/// Uses `100 * tanh(sqrt(days) / 50)`. For small ranges this approximates
/// `2 * sqrt(days)` (e.g. 1 month -> 11 samples), because tanh(x) ~ x for
/// small x. For large ranges tanh asymptotes to 1, naturally compressing
/// toward a cap of 100 (e.g. 10 years -> 84 samples). This avoids both
/// under-sampling short histories and over-sampling long ones.
pub fn default_sample_count(days: f64) -> usize {
    let count = 100.0 * (days.sqrt() / 50.0).tanh();
    (count.round() as usize).max(2)
}

/// Parse "since" string (like "3m", "6m", "1y", "all") to a DateTime.
fn parse_since_to_datetime(since: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    if crate::git::is_since_all(since) {
        // Return a date far enough in the past to cover any repository
        return Ok(DateTime::from_timestamp(0, 0).unwrap_or(now - Duration::days(365 * 50)));
    }

    let since = since.trim().to_lowercase();

    // Find where the number ends and the unit begins
    let first_alpha = since
        .find(|c: char| c.is_alphabetic())
        .unwrap_or(since.len());
    let num_str = &since[..first_alpha];
    let unit = since[first_alpha..].trim();

    let num: i64 = num_str
        .trim()
        .parse()
        .map_err(|_| Error::config(format!("Invalid since value: {}", since)))?;

    let duration = match unit {
        "d" | "day" | "days" => Duration::days(num),
        "w" | "wk" | "week" | "weeks" => Duration::weeks(num),
        "m" | "mo" | "mon" | "month" | "months" => Duration::days(num * 30),
        "y" | "yr" | "year" | "years" => Duration::days(num * 365),
        _ => return Err(Error::config(format!("Unknown time unit: {}", unit))),
    };

    Ok(now - duration)
}

/// Find the commit closest to the given time.
fn find_commit_at_time(
    commits: &[crate::git::Commit],
    target: DateTime<Utc>,
) -> Option<&crate::git::Commit> {
    let target_ts = target.timestamp();

    // Find the commit with timestamp closest to but not after target
    commits
        .iter()
        .filter(|c| c.timestamp <= target_ts)
        .min_by_key(|c| (target_ts - c.timestamp).abs())
}

/// Analyze the current working directory.
fn analyze_current(path: &Path, config: &Config) -> Result<super::Analysis> {
    let file_set = FileSet::from_path(path, config)?;
    let ctx = AnalysisContext::new(&file_set, config, Some(path));
    let analyzer = ScoreAnalyzer::new();
    analyzer.analyze(&ctx)
}

/// Analyze code at a specific git tree (commit) without filesystem checkout.
/// Reads file contents directly from git's object store.
pub fn analyze_at_tree(tree_source: &TreeSource, config: &Config) -> Result<super::Analysis> {
    let file_set = FileSet::from_tree_source(tree_source, config)?;
    let content_source: Arc<dyn ContentSource> = Arc::new(TreeSourceWrapper {
        repo_path: tree_source.repo_path().to_path_buf(),
        tree_id: tree_source.tree_id(),
    });
    let root = Path::new(".");
    let ctx =
        AnalysisContext::new(&file_set, config, Some(root)).with_content_source(content_source);
    let analyzer = ScoreAnalyzer::new();
    analyzer.analyze(&ctx)
}

/// Wrapper to create a new TreeSource for the content source.
/// This is needed because TreeSource stores state that can't be easily cloned.
struct TreeSourceWrapper {
    repo_path: std::path::PathBuf,
    tree_id: [u8; 20],
}

impl ContentSource for TreeSourceWrapper {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        // Re-create TreeSource for each read (thread-safe approach)
        let repo = gix::open(&self.repo_path)
            .map_err(|e| Error::git(format!("Failed to open repository: {e}")))?;

        let tree_oid = gix::ObjectId::from_bytes_or_panic(&self.tree_id);
        let tree = repo
            .find_object(tree_oid)
            .map_err(|e| Error::git(format!("Failed to find tree: {e}")))?
            .try_into_tree()
            .map_err(|e| Error::git(format!("Not a tree: {e}")))?;

        let path_str = path.to_string_lossy();
        let entry = tree
            .lookup_entry_by_path(path_str.as_ref())
            .map_err(|e| Error::git(format!("Failed to lookup {path_str}: {e}")))?
            .ok_or_else(|| Error::git(format!("File not found in tree: {path_str}")))?;

        let object = entry
            .object()
            .map_err(|e| Error::git(format!("Failed to get object: {e}")))?;

        let blob = object
            .try_into_blob()
            .map_err(|_| Error::git(format!("Not a blob: {path_str}")))?;

        Ok(blob.data.to_vec())
    }
}

/// Collect commit messages that fall within a time range (exclusive start, inclusive end).
/// Returns up to 5 most recent commit messages for the window.
fn collect_commits_in_range(commits: &[Commit], after_ts: i64, up_to_ts: i64) -> Vec<String> {
    let mut messages: Vec<&Commit> = commits
        .iter()
        .filter(|c| c.timestamp > after_ts && c.timestamp <= up_to_ts)
        .collect();
    // Most recent first
    messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    messages
        .into_iter()
        .take(5)
        .map(|c| c.message.clone())
        .collect()
}

/// Calculate linear regression for score trend.
/// Returns (slope, intercept, r_squared).
fn calculate_linear_regression(points: &[TrendPoint]) -> (f64, f64, f64) {
    let n = points.len() as f64;
    if n < 2.0 {
        return (0.0, 0.0, 0.0);
    }

    // Use index as x value (0, 1, 2, ...)
    let x_values: Vec<f64> = (0..points.len()).map(|i| i as f64).collect();
    let y_values: Vec<f64> = points.iter().map(|p| p.score as f64).collect();

    let x_mean = x_values.iter().sum::<f64>() / n;
    let y_mean = y_values.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;
    let mut ss_tot = 0.0;
    let mut ss_res = 0.0;

    for i in 0..points.len() {
        let x_diff = x_values[i] - x_mean;
        let y_diff = y_values[i] - y_mean;
        numerator += x_diff * y_diff;
        denominator += x_diff * x_diff;
        ss_tot += y_diff * y_diff;
    }

    let slope = if denominator != 0.0 {
        numerator / denominator
    } else {
        0.0
    };

    let intercept = y_mean - slope * x_mean;

    // Calculate R-squared
    for i in 0..points.len() {
        let predicted = slope * x_values[i] + intercept;
        let residual = y_values[i] - predicted;
        ss_res += residual * residual;
    }

    let r_squared = if ss_tot != 0.0 {
        1.0 - (ss_res / ss_tot)
    } else {
        0.0
    };

    (slope, intercept, r_squared)
}

/// Calculate trend statistics for each component.
fn calculate_component_trends(points: &[TrendPoint]) -> HashMap<String, ComponentTrendStats> {
    let mut trends = HashMap::new();

    if points.len() < 2 {
        return trends;
    }

    // Collect all component names
    let mut component_names: Vec<String> = Vec::new();
    for point in points {
        for name in point.components.keys() {
            if !component_names.contains(name) {
                component_names.push(name.clone());
            }
        }
    }

    // Calculate trend for each component
    for name in component_names {
        let component_points: Vec<TrendPoint> = points
            .iter()
            .filter_map(|p| {
                p.components.get(&name).map(|&score| TrendPoint {
                    date: p.date.clone(),
                    score,
                    components: HashMap::new(),
                    notable_commits: Vec::new(),
                })
            })
            .collect();

        if component_points.len() >= 2 {
            let (slope, _, r_squared) = calculate_linear_regression(&component_points);
            trends.insert(
                name,
                ComponentTrendStats {
                    slope,
                    correlation: r_squared.sqrt(), // Correlation is sqrt of R-squared
                },
            );
        }
    }

    trends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_sample_count() {
        // Short ranges should match ~2*sqrt(days) (tanh(x) ~ x for small x)
        assert_eq!(default_sample_count(30.0), 11);
        assert_eq!(default_sample_count(90.0), 19);

        // Long ranges compress toward 100
        assert!(default_sample_count(3650.0) <= 100);
        assert!(default_sample_count(3650.0) >= 80);

        // Minimum is 2
        assert_eq!(default_sample_count(0.0), 2);
        assert_eq!(default_sample_count(1.0), 2);
    }

    #[test]
    fn test_parse_since_days() {
        let now = Utc::now();
        let result = parse_since_to_datetime("30d", now).unwrap();
        let expected = now - Duration::days(30);
        assert!((result.timestamp() - expected.timestamp()).abs() < 1);
    }

    #[test]
    fn test_parse_since_weeks() {
        let now = Utc::now();
        let result = parse_since_to_datetime("2w", now).unwrap();
        let expected = now - Duration::weeks(2);
        assert!((result.timestamp() - expected.timestamp()).abs() < 1);
    }

    #[test]
    fn test_parse_since_months() {
        let now = Utc::now();
        let result = parse_since_to_datetime("3m", now).unwrap();
        let expected = now - Duration::days(90);
        assert!((result.timestamp() - expected.timestamp()).abs() < 1);
    }

    #[test]
    fn test_parse_since_years() {
        let now = Utc::now();
        let result = parse_since_to_datetime("1y", now).unwrap();
        let expected = now - Duration::days(365);
        assert!((result.timestamp() - expected.timestamp()).abs() < 1);
    }

    #[test]
    fn test_parse_since_all() {
        let now = Utc::now();
        let result = parse_since_to_datetime("all", now).unwrap();
        // Should return epoch (Unix timestamp 0)
        assert_eq!(result.timestamp(), 0);
    }

    #[test]
    fn test_parse_since_invalid() {
        let now = Utc::now();
        let result = parse_since_to_datetime("invalid", now);
        assert!(result.is_err());
    }

    #[test]
    fn test_linear_regression_increasing() {
        let points = vec![
            TrendPoint {
                date: "2024-01-01".to_string(),
                score: 50,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 60,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 70,
                components: HashMap::new(),
                notable_commits: vec![],
            },
        ];

        let (slope, _intercept, r_squared) = calculate_linear_regression(&points);
        assert!(slope > 0.0, "Slope should be positive for increasing trend");
        assert!(r_squared > 0.9, "R-squared should be high for linear data");
    }

    #[test]
    fn test_linear_regression_decreasing() {
        let points = vec![
            TrendPoint {
                date: "2024-01-01".to_string(),
                score: 80,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 70,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 60,
                components: HashMap::new(),
                notable_commits: vec![],
            },
        ];

        let (slope, _intercept, r_squared) = calculate_linear_regression(&points);
        assert!(slope < 0.0, "Slope should be negative for decreasing trend");
        assert!(r_squared > 0.9, "R-squared should be high for linear data");
    }

    #[test]
    fn test_linear_regression_flat() {
        let points = vec![
            TrendPoint {
                date: "2024-01-01".to_string(),
                score: 75,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 75,
                components: HashMap::new(),
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 75,
                components: HashMap::new(),
                notable_commits: vec![],
            },
        ];

        let (slope, _intercept, _r_squared) = calculate_linear_regression(&points);
        assert!(
            slope.abs() < 0.001,
            "Slope should be near zero for flat trend"
        );
    }

    #[test]
    fn test_linear_regression_single_point() {
        let points = vec![TrendPoint {
            date: "2024-01-01".to_string(),
            score: 75,
            components: HashMap::new(),
            notable_commits: vec![],
        }];

        let (slope, intercept, r_squared) = calculate_linear_regression(&points);
        assert_eq!(slope, 0.0);
        assert_eq!(intercept, 0.0);
        assert_eq!(r_squared, 0.0);
    }

    #[test]
    fn test_linear_regression_empty() {
        let points: Vec<TrendPoint> = vec![];
        let (slope, intercept, r_squared) = calculate_linear_regression(&points);
        assert_eq!(slope, 0.0);
        assert_eq!(intercept, 0.0);
        assert_eq!(r_squared, 0.0);
    }

    #[test]
    fn test_component_trends() {
        let mut components1 = HashMap::new();
        components1.insert("complexity".to_string(), 60);
        components1.insert("satd".to_string(), 70);

        let mut components2 = HashMap::new();
        components2.insert("complexity".to_string(), 65);
        components2.insert("satd".to_string(), 75);

        let mut components3 = HashMap::new();
        components3.insert("complexity".to_string(), 70);
        components3.insert("satd".to_string(), 80);

        let points = vec![
            TrendPoint {
                date: "2024-01-01".to_string(),
                score: 65,
                components: components1,
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 70,
                components: components2,
                notable_commits: vec![],
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 75,
                components: components3,
                notable_commits: vec![],
            },
        ];

        let trends = calculate_component_trends(&points);
        assert!(trends.contains_key("complexity"));
        assert!(trends.contains_key("satd"));
        assert!(trends.get("complexity").unwrap().slope > 0.0);
        assert!(trends.get("satd").unwrap().slope > 0.0);
    }

    #[test]
    fn test_component_trends_empty() {
        let points: Vec<TrendPoint> = vec![];
        let trends = calculate_component_trends(&points);
        assert!(trends.is_empty());
    }

    #[test]
    fn test_trend_data_default() {
        let data = TrendData::default();
        assert!(data.points.is_empty());
        assert_eq!(data.slope, 0.0);
        assert_eq!(data.intercept, 0.0);
        assert_eq!(data.r_squared, 0.0);
        assert_eq!(data.start_score, 0);
        assert_eq!(data.end_score, 0);
    }

    #[test]
    fn test_analyze_at_tree() {
        use crate::core::TreeSource;
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(temp.path())
            .output()
            .expect("failed to init");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp.path())
            .output()
            .expect("failed to config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(temp.path())
            .output()
            .expect("failed to config name");

        // Create a simple Rust file
        std::fs::write(
            temp.path().join("main.rs"),
            r#"
fn simple() {
    println!("hello");
}

fn complex(x: i32) -> i32 {
    if x > 0 {
        if x > 10 {
            x * 2
        } else {
            x + 1
        }
    } else {
        0
    }
}
"#,
        )
        .unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(temp.path())
            .output()
            .expect("failed to commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(temp.path())
            .output()
            .expect("failed to get HEAD");
        let sha = String::from_utf8(output.stdout).unwrap().trim().to_string();

        let tree_source = TreeSource::new(temp.path(), &sha).unwrap();
        let config = Config::default();

        // analyze_at_tree should return a score
        let result = analyze_at_tree(&tree_source, &config);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        // Score should be between 0 and 100
        assert!(analysis.overall_score >= 0.0);
        assert!(analysis.overall_score <= 100.0);
    }

    #[test]
    fn test_collect_commits_in_range() {
        let commits = vec![
            Commit {
                sha: "aaa".to_string(),
                author: "A".to_string(),
                email: "a@test.com".to_string(),
                timestamp: 100,
                message: "first commit".to_string(),
                files: vec![],
            },
            Commit {
                sha: "bbb".to_string(),
                author: "B".to_string(),
                email: "b@test.com".to_string(),
                timestamp: 200,
                message: "second commit".to_string(),
                files: vec![],
            },
            Commit {
                sha: "ccc".to_string(),
                author: "C".to_string(),
                email: "c@test.com".to_string(),
                timestamp: 300,
                message: "third commit".to_string(),
                files: vec![],
            },
            Commit {
                sha: "ddd".to_string(),
                author: "D".to_string(),
                email: "d@test.com".to_string(),
                timestamp: 400,
                message: "fourth commit".to_string(),
                files: vec![],
            },
        ];

        // Range (100, 300] should include commits at 200 and 300
        let result = collect_commits_in_range(&commits, 100, 300);
        assert_eq!(result.len(), 2);
        // Most recent first
        assert_eq!(result[0], "third commit");
        assert_eq!(result[1], "second commit");

        // Range (0, 100] should include only the first commit
        let result = collect_commits_in_range(&commits, 0, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "first commit");

        // Empty range
        let result = collect_commits_in_range(&commits, 500, 600);
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_commits_in_range_limits_to_five() {
        let commits: Vec<Commit> = (0..10)
            .map(|i| Commit {
                sha: format!("sha{}", i),
                author: "A".to_string(),
                email: "a@test.com".to_string(),
                timestamp: (i + 1) * 100,
                message: format!("commit {}", i),
                files: vec![],
            })
            .collect();

        let result = collect_commits_in_range(&commits, 0, 1500);
        assert_eq!(result.len(), 5);
        // Most recent first
        assert_eq!(result[0], "commit 9");
    }
}
