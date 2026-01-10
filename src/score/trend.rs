//! Score trend analysis over git history.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};

use crate::cli::TrendPeriod;
use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, FileSet, Result};
use crate::git::GitRepo;
use crate::report::{ComponentTrendStats, TrendData, TrendPoint};

use super::Analyzer as ScoreAnalyzer;

/// Analyze score trends over time by iterating through git history.
pub fn analyze_trend(
    path: &Path,
    config: &Config,
    since: &str,
    period: TrendPeriod,
) -> Result<TrendData> {
    let repo = GitRepo::open(path)?;
    let now = Utc::now();

    // Parse the "since" parameter to determine how far back to go
    let start_time = parse_since_to_datetime(since, now)?;

    // Determine interval based on period
    let interval = match period {
        TrendPeriod::Daily => Duration::days(1),
        TrendPeriod::Weekly => Duration::days(7),
        TrendPeriod::Monthly => Duration::days(30),
    };

    // Get commits in the time range
    let commits = repo.log(Some(since), None)?;
    if commits.is_empty() {
        return Ok(TrendData::default());
    }

    // Build sample points at regular intervals
    let mut points = Vec::new();
    let mut current_time = start_time;

    while current_time <= now {
        // Find the commit closest to this time
        if let Some(commit) = find_commit_at_time(&commits, current_time) {
            // Checkout and analyze at this commit
            if let Ok(score_data) = analyze_at_commit(path, config, &commit.sha) {
                points.push(TrendPoint {
                    date: current_time.format("%Y-%m-%d").to_string(),
                    score: score_data.overall_score as i32,
                    components: score_data
                        .components
                        .iter()
                        .map(|(k, v)| (k.clone(), v.score as i32))
                        .collect(),
                });
            }
        }
        current_time += interval;
    }

    // Always include the current HEAD
    if let Ok(score_data) = analyze_current(path, config) {
        points.push(TrendPoint {
            date: now.format("%Y-%m-%d").to_string(),
            score: score_data.overall_score as i32,
            components: score_data
                .components
                .iter()
                .map(|(k, v)| (k.clone(), v.score as i32))
                .collect(),
        });
    }

    // Calculate linear regression for overall score
    let (slope, intercept, r_squared) = if points.len() >= 2 {
        calculate_linear_regression(&points)
    } else {
        (0.0, 0.0, 0.0)
    };

    // Calculate component trends
    let component_trends = calculate_component_trends(&points);

    let start_score = points.first().map(|p| p.score).unwrap_or(0);
    let end_score = points.last().map(|p| p.score).unwrap_or(0);

    Ok(TrendData {
        points,
        slope,
        intercept,
        r_squared,
        start_score,
        end_score,
        component_trends,
    })
}

/// Parse "since" string (like "3m", "6m", "1y") to a DateTime.
fn parse_since_to_datetime(since: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
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

/// Analyze score at a specific commit (checkout, analyze, restore).
fn analyze_at_commit(path: &Path, config: &Config, sha: &str) -> Result<super::Analysis> {
    // Use git stash and checkout to temporarily switch to the commit
    let output = std::process::Command::new("git")
        .args(["stash", "push", "-m", "omen-trend-analysis"])
        .current_dir(path)
        .output()
        .map_err(|e| Error::git(format!("Failed to stash: {}", e)))?;

    let had_stash = output.status.success()
        && !String::from_utf8_lossy(&output.stdout).contains("No local changes");

    // Checkout the target commit
    let checkout_result = std::process::Command::new("git")
        .args(["checkout", sha])
        .current_dir(path)
        .output();

    let result = if checkout_result.is_ok() {
        analyze_current(path, config)
    } else {
        Err(Error::git(format!("Failed to checkout {}", sha)))
    };

    // Restore original state
    let _ = std::process::Command::new("git")
        .args(["checkout", "-"])
        .current_dir(path)
        .output();

    if had_stash {
        let _ = std::process::Command::new("git")
            .args(["stash", "pop"])
            .current_dir(path)
            .output();
    }

    result
}

/// Analyze the current working directory.
fn analyze_current(path: &Path, config: &Config) -> Result<super::Analysis> {
    let file_set = FileSet::from_path(path, config)?;
    let ctx = AnalysisContext::new(&file_set, config, Some(path));
    let analyzer = ScoreAnalyzer::new();
    analyzer.analyze(&ctx)
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
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 60,
                components: HashMap::new(),
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 70,
                components: HashMap::new(),
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
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 70,
                components: HashMap::new(),
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 60,
                components: HashMap::new(),
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
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 75,
                components: HashMap::new(),
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 75,
                components: HashMap::new(),
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
            },
            TrendPoint {
                date: "2024-01-08".to_string(),
                score: 70,
                components: components2,
            },
            TrendPoint {
                date: "2024-01-15".to_string(),
                score: 75,
                components: components3,
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
}
