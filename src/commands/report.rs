//! The `report` command and its subcommands. The command writes files and
//! messages on stderr. It writes nothing to standard output.

use std::path::PathBuf;

use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::ReportSubcommand;
use crate::config::Config;
use crate::core::progress::is_tty;
use crate::core::{AnalysisContext, Analyzer, FileSet};

/// Writes the file in one step: the old file stays until the new text is complete.
fn write_atomically(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut pending = path.as_os_str().to_owned();
    pending.push(".tmp");
    let pending = std::path::PathBuf::from(pending);
    std::fs::write(&pending, text)?;
    std::fs::rename(&pending, path)
}

/// The data files that `report generate` writes and that `report validate` expects.
pub(crate) const EXPECTED_DATA_FILES: [&str; 21] = [
    "metadata",
    "complexity",
    "satd",
    "stubs",
    "deadcode",
    "churn",
    "clones",
    "defect",
    "changes",
    "tdg",
    "graph",
    "hotspot",
    "temporal",
    "ownership",
    "cohesion",
    "repomap",
    "smells",
    "flags",
    "security",
    "score",
    "trend",
];

pub fn run(
    path: &PathBuf,
    config: &Config,
    subcommand: &ReportSubcommand,
    git_ref: Option<&str>,
) -> crate::core::Result<()> {
    use serde_json::{json, Value};

    match subcommand {
        ReportSubcommand::Generate(args) => {
            let since_days = super::analyzers::since_flag_days(&args.since)?;
            // The reference is checked before the first write, so a failed run keeps
            // the files of an earlier run.
            let changes = git_ref
                .map(|reference| -> crate::core::Result<_> {
                    let mut changes =
                        crate::changeset::build::compute(path, Some(reference), config)?;
                    changes.security = Some(crate::changeset::security::compute(
                        path,
                        Some(reference),
                        config,
                        &changes,
                    )?);
                    Ok(changes)
                })
                .transpose()?;
            std::fs::create_dir_all(&args.data)?;

            let file_set = FileSet::from_path(path, config)?;
            let git_root = crate::git::GitRepo::open(path)
                .ok()
                .map(|r| r.root().to_path_buf());
            let mut ctx = AnalysisContext::new(&file_set, config, Some(path));
            if let Some(ref git_path) = git_root {
                ctx = ctx.with_git_path(git_path);
            }

            // Generate metadata.json (matches Go structure)
            // Canonicalize path to handle "." and get actual directory name
            let repo_name = std::fs::canonicalize(path)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "unknown".to_string());
            // Use --days if provided, otherwise use --since
            let since_str = if let Some(days) = args.days {
                format!("{} days", days)
            } else {
                args.since.clone()
            };
            let metadata = json!({
                "repository": repo_name,
                "generated_at": chrono::Utc::now().to_rfc3339(),
                "since": since_str,
                "omen_version": env!("CARGO_PKG_VERSION"),
                "paths": [path.display().to_string()]
            });
            let metadata_path = args.data.join("metadata.json");
            std::fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;

            let changes_path = args.data.join("diff.json");
            match changes {
                Some(changes) => {
                    let pending_path = args.data.join("diff.json.tmp");
                    std::fs::write(&pending_path, serde_json::to_string_pretty(&changes)?)?;
                    std::fs::rename(&pending_path, &changes_path)?;
                }
                None => match std::fs::remove_file(&changes_path) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.into())
                    }
                    _ => {}
                },
            }

            let skip_list: Vec<&str> = args
                .skip
                .as_deref()
                .map(|s| s.split(',').collect())
                .unwrap_or_default();

            // Each name is a command name. It is also the artifact file stem and the
            // `--skip` value.
            let analyzer_names = [
                "complexity",
                "satd",
                "stubs",
                "deadcode",
                "churn",
                "clones",
                "defect",
                "changes",
                "tdg",
                "graph",
                "hotspot",
                "temporal",
                "ownership",
                "cohesion",
                "repomap",
                "smells",
                "flags",
                "security",
                "score",
                "trend",
            ];
            let total_analyzers = analyzer_names
                .iter()
                .filter(|n| !skip_list.contains(*n))
                .count();

            // Set up progress bar
            let progress = if is_tty() {
                let bar = ProgressBar::new(total_analyzers as u64);
                bar.set_style(
                    ProgressStyle::default_bar()
                        .template("{prefix:.bold} [{bar:30.green/white}] {pos}/{len} {msg}")
                        .expect("valid template")
                        .progress_chars("=>-"),
                );
                bar.set_prefix("Generating");
                Some(bar)
            } else {
                None
            };

            let completed = std::sync::atomic::AtomicU64::new(0);
            let write_errors: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
            let output_dir = &args.data;

            // Helper: run an analyzer and save its JSON output
            macro_rules! run_analyzer {
                ($analyzer:expr, $name:expr, $filename:expr) => {{
                    if !skip_list.contains(&$name) {
                        let result: Value = match $analyzer.analyze(&ctx) {
                            Ok(r) => serde_json::to_value(&r)
                                .unwrap_or(json!({"error": "serialization failed"})),
                            Err(e) => json!({"error": e.to_string()}),
                        };
                        let output_path = output_dir.join(format!("{}.json", $filename));
                        match write_atomically(
                            &output_path,
                            &serde_json::to_string_pretty(&result).unwrap_or_default(),
                        ) {
                            Ok(()) => {
                                let done =
                                    completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                        + 1;
                                if let Some(ref bar) = progress {
                                    bar.set_position(done);
                                } else {
                                    eprintln!("Generated: {}", output_path.display());
                                }
                            }
                            Err(error) => write_errors
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .push(format!("{}: {error}", output_path.display())),
                        }
                    }
                }};
            }

            // Phase 1: Run all analyzers in parallel groups.
            // Group A: File-parsing analyzers (CPU-bound, share tree-sitter work)
            // Group B: Git-heavy analyzers (I/O-bound, overlap with CPU work)
            // Group C: Mixed analyzers (benefit from warm OS caches)
            let churn_days = args.days.unwrap_or(since_days);
            let security_analyzer = crate::analyzers::security::Analyzer::from_config(config)?;

            std::thread::scope(|s| {
                // Group A: file-based analyzers
                s.spawn(|| {
                    run_analyzer!(
                        crate::analyzers::complexity::Analyzer::default(),
                        "complexity",
                        "complexity"
                    );
                    run_analyzer!(crate::analyzers::satd::Analyzer::default(), "satd", "satd");
                    run_analyzer!(crate::analyzers::stubs::Analyzer::new(), "stubs", "stubs");
                    run_analyzer!(security_analyzer, "security", "security");
                    run_analyzer!(
                        crate::analyzers::deadcode::Analyzer::default(),
                        "deadcode",
                        "deadcode"
                    );
                    run_analyzer!(
                        crate::analyzers::duplicates::Analyzer::default(),
                        "clones",
                        "clones"
                    );
                    run_analyzer!(
                        crate::analyzers::cohesion::Analyzer::default(),
                        "cohesion",
                        "cohesion"
                    );
                    run_analyzer!(
                        crate::analyzers::repomap::Analyzer::default(),
                        "repomap",
                        "repomap"
                    );
                });

                // Group B: git-heavy analyzers (ownership is the longest at ~57s)
                s.spawn(|| {
                    run_analyzer!(
                        crate::analyzers::ownership::Analyzer::default(),
                        "ownership",
                        "ownership"
                    );
                    run_analyzer!(
                        crate::analyzers::churn::Analyzer::new().with_days(churn_days),
                        "churn",
                        "churn"
                    );
                    run_analyzer!(
                        crate::analyzers::temporal::Analyzer::default(),
                        "temporal",
                        "temporal"
                    );
                    run_analyzer!(
                        crate::analyzers::changes::Analyzer::default(),
                        "changes",
                        "changes"
                    );
                });

                // Group C: mixed file+git analyzers
                s.spawn(|| {
                    run_analyzer!(
                        crate::analyzers::graph::Analyzer::default(),
                        "graph",
                        "graph"
                    );
                    run_analyzer!(
                        crate::analyzers::smells::Analyzer::default(),
                        "smells",
                        "smells"
                    );
                    run_analyzer!(
                        crate::analyzers::flags::Analyzer::default(),
                        "flags",
                        "flags"
                    );
                    run_analyzer!(
                        crate::analyzers::defect::Analyzer::from_config(config),
                        "defect",
                        "defect"
                    );
                    run_analyzer!(
                        crate::analyzers::hotspot::Analyzer::from_config(config),
                        "hotspot",
                        "hotspot"
                    );
                    run_analyzer!(crate::analyzers::tdg::Analyzer::default(), "tdg", "tdg");
                });
            });

            let failed = write_errors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            if !failed.is_empty() {
                return Err(crate::core::Error::analysis(format!(
                    "The report data was not written: {}",
                    failed.join("; ")
                )));
            }

            // Phase 2: Score (reads pre-generated JSON files, nearly instant)
            if !skip_list.contains(&"score") {
                if let Some(ref bar) = progress {
                    bar.set_message("score...");
                }
                let result: Value = match crate::score::compute_from_data_dir(
                    output_dir,
                    ctx.files.files().len(),
                ) {
                    Ok(r) => {
                        serde_json::to_value(&r).unwrap_or(json!({"error": "serialization failed"}))
                    }
                    Err(e) => json!({"error": e.to_string()}),
                };
                let output_path = output_dir.join("score.json");
                std::fs::write(&output_path, serde_json::to_string_pretty(&result)?)?;
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if let Some(ref bar) = progress {
                    bar.set_position(done);
                } else {
                    eprintln!("Generated: {}", output_path.display());
                }
            }

            // Phase 3: Trend data (analyzes historical commits)
            if !skip_list.contains(&"trend") {
                if let Some(ref bar) = progress {
                    bar.set_message("trend...");
                }
                let samples = args.samples.or_else(|| {
                    // All the history has no number of days. Use 50 years.
                    let days = if since_days == u32::MAX {
                        (365 * 50) as f64
                    } else {
                        since_days as f64
                    };
                    Some(crate::score::default_sample_count(days))
                });
                match crate::score::analyze_trend(
                    path,
                    config,
                    &args.since,
                    crate::cli::TrendPeriod::Monthly,
                    samples,
                ) {
                    Ok(trend_data) => {
                        let output_path = output_dir.join("trend.json");
                        if let Err(e) =
                            std::fs::write(&output_path, serde_json::to_string_pretty(&trend_data)?)
                        {
                            eprintln!("Warning: failed to write trend.json: {}", e);
                        } else {
                            let done =
                                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            if let Some(ref bar) = progress {
                                bar.set_position(done);
                            } else {
                                eprintln!("Generated: {}", output_path.display());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: trend analysis failed: {}", e);
                    }
                }
            }

            if let Some(bar) = progress {
                bar.finish_with_message("done");
            }
            eprintln!("Report data generated in: {}", output_dir.display());
        }
        ReportSubcommand::Validate(args) => {
            // Basic validation: check that expected JSON files exist and are valid JSON
            // File list matches Go version
            let expected_files = EXPECTED_DATA_FILES;

            let mut errors = Vec::new();
            let mut valid_count = 0;

            for name in expected_files {
                let file_path = args.data.join(format!("{}.json", name));
                if file_path.exists() {
                    match std::fs::read_to_string(&file_path) {
                        Ok(contents) => match serde_json::from_str::<Value>(&contents) {
                            Ok(_) => {
                                valid_count += 1;
                                eprintln!("Valid: {}.json", name);
                            }
                            Err(e) => errors.push(format!("{}.json: invalid JSON - {}", name, e)),
                        },
                        Err(e) => errors.push(format!("{}.json: read error - {}", name, e)),
                    }
                } else {
                    errors.push(format!("{}.json: missing", name));
                }
            }

            if errors.is_empty() {
                eprintln!("All {} data files are valid.", valid_count);
            } else {
                eprintln!("\nValidation errors:");
                for error in &errors {
                    eprintln!("  - {}", error);
                }
                return Err(crate::core::Error::config(format!(
                    "{} validation errors found",
                    errors.len()
                )));
            }
        }
        ReportSubcommand::Render(args) => {
            use crate::report::Renderer;

            let renderer = Renderer::new()?;
            let output = args
                .output
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from(".omen/report.html"));
            renderer.render_to_file(&args.data, &output)?;
            eprintln!("Report rendered to: {}", output.display());
            eprintln!(
                "Compressed:         {}",
                Renderer::gz_path(&output).display()
            );
        }
        ReportSubcommand::Serve(args) => {
            eprintln!("Starting server at http://{}:{}/", args.host, args.port);
            eprintln!("Serving data from: {}", args.data.display());
            eprintln!("Press Ctrl+C to stop.");

            // Simple HTTP server using std::net
            use std::io::{Read, Write};
            use std::net::TcpListener;

            let addr = format!("{}:{}", args.host, args.port);
            let listener = TcpListener::bind(&addr)?;

            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                if stream.read(&mut buffer).is_ok() {
                    let request = String::from_utf8_lossy(&buffer);

                    let response = if request.starts_with("GET / ")
                        || request.starts_with("GET /index.html ")
                    {
                        // Serve rendered report
                        let report_path = args.data.parent().unwrap_or(path).join("report.html");
                        if report_path.exists() {
                            match std::fs::read_to_string(&report_path) {
                                Ok(html) => format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                                    html.len(),
                                    html
                                ),
                                Err(_) => "HTTP/1.1 500 Internal Server Error\r\n\r\nFailed to read report".to_string(),
                            }
                        } else {
                            "HTTP/1.1 404 Not Found\r\n\r\nReport not found. Run 'omen report render' first.".to_string()
                        }
                    } else {
                        "HTTP/1.1 404 Not Found\r\n\r\nNot Found".to_string()
                    };

                    let _ = stream.write_all(response.as_bytes());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
