//! The `report` command and its subcommands. The command writes files and
//! messages on stderr. It writes nothing to standard output.

use std::path::PathBuf;

use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::ReportSubcommand;
use crate::config::Config;
use crate::core::progress::is_tty;
use crate::core::{AnalysisContext, Analyzer, FileSet};

/// One change that `report generate` makes in the data directory.
enum Output {
    Write(PathBuf, String),
    Remove(PathBuf),
}

impl Output {
    fn path(&self) -> &std::path::Path {
        match self {
            Output::Write(path, _) | Output::Remove(path) => path,
        }
    }
}

/// Make all changes, or none.
///
/// First, each new file goes to a temporary file in the directory of its target. If a target is
/// not a file, or if a temporary file fails, nothing changes. Then each temporary file replaces
/// its target. If a replacement fails, the function puts the earlier files back.
fn commit_outputs(outputs: &[Output]) -> Result<(), String> {
    let describe = |path: &std::path::Path, error: &dyn std::fmt::Display| {
        format!("{}: {error}", path.display())
    };
    let mut old_content: Vec<Option<Vec<u8>>> = Vec::new();
    for output in outputs {
        let path = output.path();
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => {
                old_content.push(Some(std::fs::read(path).map_err(|e| describe(path, &e))?));
            }
            Ok(_) => return Err(describe(path, &"the target is not a file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => old_content.push(None),
            Err(error) => return Err(describe(path, &error)),
        }
    }

    let mut staged: Vec<Option<tempfile::NamedTempFile>> = Vec::new();
    for output in outputs {
        let Output::Write(path, text) = output else {
            staged.push(None);
            continue;
        };
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let stage = || -> std::io::Result<tempfile::NamedTempFile> {
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(directory)?;
            file.write_all(text.as_bytes())?;
            file.as_file().sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o644))?;
            }
            Ok(file)
        };
        staged.push(Some(stage().map_err(|e| describe(path, &e))?));
    }

    for (index, (output, stage)) in outputs.iter().zip(staged).enumerate() {
        let result = match (output, stage) {
            (Output::Write(path, _), Some(file)) => {
                file.persist(path).map(|_| ()).map_err(|e| e.error)
            }
            (Output::Remove(path), _) => match std::fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            (Output::Write(..), None) => unreachable!("each write has a staged file"),
        };
        if let Err(error) = result {
            for (done, content) in outputs[..index].iter().zip(&old_content) {
                let path = done.path();
                let _ = match content {
                    Some(bytes) => crate::core::write_atomically(path, bytes, 0o644),
                    None => std::fs::remove_file(path),
                };
            }
            return Err(describe(output.path(), &error));
        }
    }
    Ok(())
}

/// The one history window of `report generate`. Each part reads it.
struct GenerateWindow {
    /// The text of the window in `metadata.json`.
    label: String,
    /// The window as a `--since` value, for the trend.
    since: String,
    days: u32,
}

/// `--since` wins over `--days`, which is the alias. Without the two flags, the window is one
/// year.
fn generate_window(since: Option<&str>, days: Option<u32>) -> crate::core::Result<GenerateWindow> {
    let since = match (since, days) {
        (Some(since), _) if crate::git::is_since_all(since) => "all".to_string(),
        (Some(since), _) => since.trim().to_lowercase(),
        (None, Some(days)) => format!("{days}d"),
        (None, None) => "1y".to_string(),
    };
    Ok(GenerateWindow {
        days: super::analyzers::since_flag_days(&since)?,
        label: since.clone(),
        since,
    })
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

/// Each name is a command name. It is also the artifact file stem and the `--skip` value.
const ANALYZER_NAMES: [&str; 20] = [
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

/// The number of files that `generate` counts as done.
fn progress_total(skip_list: &[&str], has_diff: bool) -> u64 {
    let analyzers = ANALYZER_NAMES
        .iter()
        .filter(|name| !skip_list.contains(name))
        .count();
    (analyzers + 1 + usize::from(has_diff)) as u64
}

fn generate(
    path: &PathBuf,
    config: &Config,
    args: &crate::cli::ReportGenerateArgs,
    git_ref: Option<&str>,
    source: Option<std::sync::Arc<dyn crate::core::ContentSource>>,
) -> crate::core::Result<()> {
    use serde_json::{json, Value};

    let window = generate_window(args.since.as_deref(), args.days)?;
    // The reference is checked before the first write, so a failed run keeps
    // the files of an earlier run.
    let changes = git_ref
        .map(|reference| -> crate::core::Result<_> {
            let mut changes = crate::changeset::build::compute(path, Some(reference), config)?;
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
    if let Some(source) = source {
        ctx = ctx.with_content_source(source);
    }

    // Generate metadata.json (matches Go structure)
    // Canonicalize path to handle "." and get actual directory name
    let repo_name = std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "unknown".to_string());
    let metadata = json!({
        "repository": repo_name,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "since": window.label,
        "omen_version": env!("CARGO_PKG_VERSION"),
        "paths": [path.display().to_string()]
    });
    let metadata_path = args.data.join("metadata.json");
    let metadata_text = serde_json::to_string_pretty(&metadata)?;

    let changes_path = args.data.join("diff.json");
    let changes_text = changes
        .map(|changes| serde_json::to_string_pretty(&changes))
        .transpose()?;

    let skip_list: Vec<&str> = args
        .skip
        .as_deref()
        .map(|s| s.split(',').collect())
        .unwrap_or_default();

    // Set up progress bar
    let progress = if is_tty() {
        let bar = ProgressBar::new(progress_total(&skip_list, changes_text.is_some()));
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

    let output_dir = &args.data;

    // The results stay in memory until all analyzers have finished. A file that
    // changed during the analysis fails the command before the first write, so the
    // data directory never has a mix of old and new files.
    let staged: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());
    let changed_file: std::sync::Mutex<Option<crate::core::Error>> = std::sync::Mutex::new(None);
    let keep_if_changed = |error: crate::core::Error| {
        if matches!(error, crate::core::Error::ChangedDuringAnalysis { .. }) {
            changed_file
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get_or_insert(error);
        }
    };

    // Helper: run an analyzer and keep its JSON output
    macro_rules! run_analyzer {
                ($analyzer:expr, $name:expr, $filename:expr) => {{
                    if !skip_list.contains(&$name) {
                        let result: Value = match $analyzer.analyze(&ctx) {
                            Ok(r) => serde_json::to_value(&r)
                                .unwrap_or(json!({"error": "serialization failed"})),
                            Err(e) => {
                                let entry = json!({"error": e.to_string()});
                                keep_if_changed(e);
                                entry
                            }
                        };
                        let output_path = output_dir.join(format!("{}.json", $filename));
                        staged
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push((
                                output_path,
                                serde_json::to_string_pretty(&result).unwrap_or_default(),
                            ));
                    }
                }};
            }

    // Phase 1: Run all analyzers in parallel groups.
    // Group A: File-parsing analyzers (CPU-bound, share tree-sitter work)
    // Group B: Git-heavy analyzers (I/O-bound, overlap with CPU work)
    // Group C: Mixed analyzers (benefit from warm OS caches)
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
                crate::analyzers::churn::Analyzer::new().with_days(window.days),
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

    // Phase 2: Trend data (analyzes historical commits). It needs no file of the
    // other analyzers.
    let mut trend_text = None;
    if !skip_list.contains(&"trend") {
        if let Some(ref bar) = progress {
            bar.set_message("trend...");
        }
        let samples = args.samples.or_else(|| {
            // All the history has no number of days. Use 50 years.
            let days = if window.days == u32::MAX {
                (365 * 50) as f64
            } else {
                window.days as f64
            };
            Some(crate::score::default_sample_count(days))
        });
        match crate::score::analyze_trend(
            path,
            config,
            &window.since,
            crate::cli::TrendPeriod::Monthly,
            samples,
        ) {
            Ok(trend_data) => trend_text = Some(serde_json::to_string_pretty(&trend_data)?),
            Err(e) => {
                eprintln!("Warning: trend analysis failed: {}", e);
                keep_if_changed(e);
            }
        }
    }

    if let Some(error) = changed_file
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
    {
        return Err(error);
    }

    // Phase 3: Score, from the results of this run in memory.
    let mut outputs: Vec<Output> = vec![Output::Write(metadata_path, metadata_text)];
    outputs.push(match changes_text {
        Some(text) => Output::Write(changes_path, text),
        None => Output::Remove(changes_path),
    });
    outputs.extend(
        std::mem::take(&mut *staged.lock().unwrap_or_else(|p| p.into_inner()))
            .into_iter()
            .map(|(output_path, text)| Output::Write(output_path, text)),
    );
    if !skip_list.contains(&"score") {
        if let Some(ref bar) = progress {
            bar.set_message("score...");
        }
        let document = |name: &str| {
            outputs.iter().find_map(|output| match output {
                Output::Write(output_path, text)
                    if output_path.file_name().is_some_and(|file| file == name) =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
        };
        let result: Value =
            match crate::score::compute_from_documents(&document, ctx.files.files().len()) {
                Ok(r) => {
                    serde_json::to_value(&r).unwrap_or(json!({"error": "serialization failed"}))
                }
                Err(e) => json!({"error": e.to_string()}),
            };
        outputs.push(Output::Write(
            output_dir.join("score.json"),
            serde_json::to_string_pretty(&result)?,
        ));
    }
    // A trend that failed leaves no trend.json: an old one must not stay beside new data.
    if !skip_list.contains(&"trend") {
        let trend_path = output_dir.join("trend.json");
        outputs.push(match trend_text {
            Some(text) => Output::Write(trend_path, text),
            None => Output::Remove(trend_path),
        });
    }

    // Phase 4: Commit all outputs, or none.
    commit_outputs(&outputs).map_err(|error| {
        crate::core::Error::analysis(format!("The report data was not written: {error}"))
    })?;
    let written = outputs
        .iter()
        .filter(|output| matches!(output, Output::Write(..)))
        .count();
    match progress {
        Some(bar) => {
            bar.set_position(written as u64);
            bar.finish_with_message("done");
        }
        None => {
            for output in &outputs {
                if let Output::Write(output_path, _) = output {
                    eprintln!("Generated: {}", output_path.display());
                }
            }
        }
    }
    eprintln!("Report data generated in: {}", output_dir.display());
    Ok(())
}

pub fn run(
    path: &PathBuf,
    config: &Config,
    subcommand: &ReportSubcommand,
    git_ref: Option<&str>,
) -> crate::core::Result<()> {
    match subcommand {
        ReportSubcommand::Generate(args) => generate(path, config, args, git_ref, None)?,
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
                        Ok(contents) => {
                            match serde_json::from_str::<serde_json::Value>(&contents) {
                                Ok(_) => {
                                    valid_count += 1;
                                    eprintln!("Valid: {}.json", name);
                                }
                                Err(e) => {
                                    errors.push(format!("{}.json: invalid JSON - {}", name, e))
                                }
                            }
                        }
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
