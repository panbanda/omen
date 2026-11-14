//! The `mutation` command and `mutation train`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use indicatif::{ProgressBar, ProgressStyle};

use super::gate::{gate_dispatch, resolve_gate_mode};
use super::scope::{build_context, filtered_file_set, write_report_file};
use crate::cli::{GateMode, MutationArgs, MutationTrainArgs};
use crate::config::Config;
use crate::core::progress::is_tty;
use crate::core::Analyzer;
use crate::output::{format_with_limits, Format};

pub fn run(
    path: &PathBuf,
    config: &Config,
    args: &MutationArgs,
    format: Format,
    mut out: &mut dyn Write,
) -> crate::core::Result<()> {
    use crate::analyzers::mutation;
    use crate::analyzers::mutation::ml_predictor::{SurvivabilityPredictor, TrainingData};
    use crate::analyzers::mutation::MutantStatus;

    // Route glob/exclude/changed-since through the same shared helper every
    // other analyzer uses, instead of hand-rolling glob/exclude here. This
    // also means mutation now honors --changed-since, which it previously
    // ignored entirely.
    let file_set = filtered_file_set(path, config, Some(&args.common))?;

    // Load predictor model if --skip-predicted is specified omen:ignore
    let predictor = if args.skip_predicted.is_some() {
        let model_path = args
            .model
            .clone()
            .unwrap_or_else(|| path.join(SurvivabilityPredictor::default_model_path()));
        let p = SurvivabilityPredictor::load_or_default(&model_path);
        if !p.is_trained() {
            eprintln!(
                "Warning: No trained model found at {}. Run 'omen mutation train' first.",
                model_path.display()
            );
        }
        Some(p)
    } else {
        None
    };

    // Show analysis progress
    let spinner = if is_tty() {
        let s = ProgressBar::new_spinner();
        s.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("valid template"),
        );
        s.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(s)
    } else {
        None
    };

    if let Some(ref s) = spinner {
        s.set_message(format!("Analyzing {} files...", file_set.len()));
    }

    // Parse operators
    let operators: Vec<String> = args
        .operators
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .collect();

    // Build analyzer. Note: the gate threshold is intentionally
    // *not* wired through `mutation::Analyzer::min_score` here (that would
    // make `analyze()` itself return `Err` before the report is written to
    // stdout, unlike every other gated command). Instead the CLI evaluates
    // the threshold itself below, after the report has been emitted.
    let mut analyzer = mutation::Analyzer::new()
        .operators(operators)
        .test_command(args.test_command.clone())
        .timeout(args.timeout)
        .dry_run(args.dry_run)
        .allow_dirty(args.allow_dirty);

    // Configure ML-based filtering if --skip-predicted is set omen:ignore
    if let Some(threshold) = args.skip_predicted {
        if let Some(p) = predictor {
            analyzer = analyzer.skip_predicted(Some(threshold)).predictor(p);
        }
    }

    let mut ctx = build_context(path, &file_set, config);

    // Add progress callback
    let progress_counter = Arc::new(AtomicUsize::new(0));
    let total_files = file_set.len();
    let spinner_clone = spinner.clone();
    let counter_clone = progress_counter.clone();
    ctx = ctx.with_progress(move |current, _total| {
        counter_clone.store(current, Ordering::Relaxed);
        if let Some(ref s) = spinner_clone {
            s.set_message(format!("Analyzing... {}/{} files", current, total_files));
        }
    });

    let result = analyzer.analyze(&ctx)?;

    if let Some(s) = spinner {
        s.finish_and_clear();
    }

    write_report_file(
        args.output.output.as_deref(),
        &serde_json::to_value(&result)?,
    )?;

    // Output results
    match format {
        Format::Json | Format::JsonCompact => {
            let value = serde_json::to_value(&result)?;
            format_with_limits(
                value,
                format,
                args.common.pagination.top,
                args.common.pagination.offset,
                &mut out,
            )?;
        }
        Format::Markdown => {
            say!(out, "# Mutation Testing Report\n");
            say!(out, "## Summary\n");
            say!(out, "- **Total Files**: {}", result.summary.total_files);
            say!(out, "- **Total Mutants**: {}", result.summary.total_mutants);
            say!(out, "- **Killed**: {}", result.summary.killed);
            say!(out, "- **Survived**: {}", result.summary.survived);
            say!(out, "- **Timeout**: {}", result.summary.timeout);
            say!(out, "- **Error**: {}", result.summary.error);
            if result.summary.skipped > 0 {
                say!(
                    out,
                    "- **Skipped**: {} (ML predicted)",
                    result.summary.skipped
                );
            }
            say!(
                out,
                "- **Mutation Score**: {:.1}%",
                result.summary.mutation_score * 100.0
            );
            say!(out, "- **Duration**: {}ms\n", result.summary.duration_ms);

            if !result.summary.by_operator.is_empty() {
                say!(out, "## By Operator\n");
                say!(out, "| Operator | Total | Killed | Survived |");
                say!(out, "|----------|-------|--------|----------|");
                // `by_operator` is a hash map, so it has no fixed order.
                let mut by_operator: Vec<_> = result.summary.by_operator.iter().collect();
                by_operator.sort_by_key(|(operator, _)| *operator);
                for (op, stats) in by_operator {
                    say!(
                        out,
                        "| {} | {} | {} | {} |",
                        op,
                        stats.total,
                        stats.killed,
                        stats.survived
                    );
                }
                say!(out);
            }

            if !result.files.is_empty() {
                say!(out, "## Files\n");
                // Apply the same --top/--offset pagination the JSON/SARIF
                // branches get from `format_with_limits`, so `mutation -f
                // markdown --top N` doesn't dump every file while JSON does
                // not. Summary totals above are computed from the full
                // `result`, not this paginated slice.
                let offset = args.common.pagination.offset.unwrap_or(0);
                let top = args.common.pagination.top.unwrap_or(0); // 0 = unlimited
                let limit = if top == 0 { usize::MAX } else { top };
                for file in result.files.iter().skip(offset).take(limit) {
                    say!(
                        out,
                        "### {} (score: {:.1}%)\n",
                        file.file,
                        file.score * 100.0
                    );
                    if file.skipped > 0 {
                        say!(
                            out,
                            "- Killed: {}, Survived: {}, Skipped: {}, Timeout: {}, Error: {}\n",
                            file.killed,
                            file.survived,
                            file.skipped,
                            file.timeout,
                            file.error
                        );
                    } else {
                        say!(
                            out,
                            "- Killed: {}, Survived: {}, Timeout: {}, Error: {}\n",
                            file.killed,
                            file.survived,
                            file.timeout,
                            file.error
                        );
                    }
                }
            }
        }
        Format::Text => {
            say!(out, "Mutation Testing Report");
            say!(out, "=======================");
            say!(out, "Files: {}", result.summary.total_files);
            say!(out, "Mutants: {}", result.summary.total_mutants);
            if result.summary.skipped > 0 {
                say!(
                    out,
                    "Killed: {} | Survived: {} | Skipped: {} | Timeout: {} | Error: {}",
                    result.summary.killed,
                    result.summary.survived,
                    result.summary.skipped,
                    result.summary.timeout,
                    result.summary.error
                );
            } else {
                say!(
                    out,
                    "Killed: {} | Survived: {} | Timeout: {} | Error: {}",
                    result.summary.killed,
                    result.summary.survived,
                    result.summary.timeout,
                    result.summary.error
                );
            }
            say!(
                out,
                "Mutation Score: {:.1}%",
                result.summary.mutation_score * 100.0
            );
            say!(out, "Duration: {}ms", result.summary.duration_ms);
        }
        Format::Sarif => {
            let value = serde_json::to_value(&result)?;
            format_with_limits(
                value,
                format,
                args.common.pagination.top,
                args.common.pagination.offset,
                &mut out,
            )?;
        }
    }

    // The gate fails if the mutation score is below `--min-score`.
    let gate = resolve_gate_mode(args.gate.mode, config.mutation.gate);
    if gate != GateMode::Off && result.summary.mutation_score < args.min_score {
        let message = format!(
            "Mutation score {:.1}% is below minimum threshold {:.1}%",
            result.summary.mutation_score * 100.0,
            args.min_score * 100.0
        );
        eprintln!("{message}");
        gate_dispatch(
            gate,
            crate::core::Error::threshold_violation(message, result.summary.mutation_score),
        )?;
    }

    // Save results to history if --record flag is set
    if args.record && !args.dry_run {
        use std::io::Write;

        let history_path = path.join(".omen/mutation-history.jsonl");

        // Ensure .omen directory exists
        if let Some(parent) = history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Append results to history file (JSONL format - one JSON object per line)
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path);

        match file {
            Ok(mut f) => {
                let mut count = 0;
                for file_result in &result.files {
                    let source =
                        std::fs::read_to_string(path.join(&file_result.file)).unwrap_or_default();

                    for mutation_result in &file_result.mutants {
                        let was_killed = match mutation_result.status {
                            MutantStatus::Killed => true,
                            MutantStatus::Survived => false,
                            _ => continue,
                        };

                        let lines: Vec<&str> = source.lines().collect();
                        let line_idx = mutation_result.mutant.line.saturating_sub(1) as usize;
                        let start = line_idx.saturating_sub(5);
                        let end = (line_idx + 6).min(lines.len());
                        let source_context = lines[start..end].join("\n");

                        let record = TrainingData {
                            mutant: mutation_result.mutant.clone(),
                            source_context,
                            was_killed,
                            execution_time_ms: mutation_result.duration_ms,
                        };

                        if let Ok(json) = serde_json::to_string(&record) {
                            let _ = writeln!(f, "{}", json);
                            count += 1;
                        }
                    }
                }
                eprintln!("Saved {} results to {}", count, history_path.display());
            }
            Err(e) => {
                eprintln!("Warning: Failed to open history file: {}", e);
            }
        }
    }

    Ok(())
}

pub fn run_train(
    path: &std::path::Path,
    args: &MutationTrainArgs,
    out: &mut dyn Write,
) -> crate::core::Result<()> {
    use crate::analyzers::mutation::ml_predictor::{SurvivabilityPredictor, TrainingData};
    use std::io::BufRead;

    let history_path = args
        .history
        .clone()
        .unwrap_or_else(|| path.join(".omen/mutation-history.jsonl"));

    let model_path = args
        .model
        .clone()
        .unwrap_or_else(|| path.join(SurvivabilityPredictor::default_model_path()));

    // Check if history file exists
    if !history_path.exists() {
        return Err(crate::core::Error::analysis(format!(
            "History file not found: {}\nRun 'omen mutation --record' first to collect training data.",
            history_path.display()
        )));
    }

    // Read training data from history file (JSONL format)
    let file = std::fs::File::open(&history_path)
        .map_err(|e| crate::core::Error::analysis(format!("Failed to open history file: {}", e)))?;

    let reader = std::io::BufReader::new(file);
    let mut training_data: Vec<TrainingData> = Vec::new();

    for line in reader.lines() {
        let line =
            line.map_err(|e| crate::core::Error::analysis(format!("Failed to read line: {}", e)))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TrainingData>(&line) {
            Ok(data) => training_data.push(data),
            Err(e) => {
                eprintln!("Warning: Skipping malformed line: {}", e);
            }
        }
    }

    if training_data.is_empty() {
        return Err(crate::core::Error::analysis(
            "No valid training data found in history file".to_string(),
        ));
    }

    say!(
        out,
        "Training model from {} historical results...",
        training_data.len()
    );

    // Train the predictor
    let mut predictor = SurvivabilityPredictor::new();
    predictor
        .train(&training_data)
        .map_err(|e| crate::core::Error::analysis(format!("Training failed: {}", e)))?;

    // Ensure output directory exists
    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::core::Error::analysis(format!("Failed to create directory: {}", e))
        })?;
    }

    // Save the model
    predictor
        .save(&model_path)
        .map_err(|e| crate::core::Error::analysis(format!("Failed to save model: {}", e)))?;

    say!(out, "Model saved to {}", model_path.display());
    say!(out, "\nOperator kill rates learned:");
    for (op, rate) in predictor.operator_kill_rates() {
        say!(out, "  {}: {:.1}%", op, rate * 100.0);
    }

    Ok(())
}
