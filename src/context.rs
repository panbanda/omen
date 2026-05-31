use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analyzers::{complexity, repomap, satd};
use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, FileSet, Language, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub repository: String,
    pub file_count: usize,
    pub languages: Vec<LanguageSummary>,
    pub top_symbols: Vec<SymbolSummary>,
    pub risks: Vec<RiskSummary>,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageSummary {
    pub language: String,
    pub files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSummary {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSummary {
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub message: String,
    pub severity: String,
}

pub fn build_context(
    root: &Path,
    files: &FileSet,
    config: &Config,
    max_symbols: Option<usize>,
    max_risks: Option<usize>,
) -> Result<Context> {
    let ctx = AnalysisContext::new(files, config, Some(root));
    let repository = root
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "unknown".to_string());

    let languages = summarize_languages(files);
    let symbol_limit = max_symbols.unwrap_or(25);
    let risk_limit = max_risks.unwrap_or(25);

    let repomap = repomap::Analyzer::default().analyze(&ctx)?;
    let top_symbols = repomap
        .symbols
        .iter()
        .take(symbol_limit)
        .map(|symbol| SymbolSummary {
            name: symbol.name.clone(),
            kind: format!("{:?}", symbol.kind).to_lowercase(),
            file: symbol.file.clone(),
            line: symbol.line,
            score: symbol.pagerank,
        })
        .collect();

    let risks = summarize_risks(&ctx, risk_limit);
    let hints = build_hints(files, &languages, &risks);

    Ok(Context {
        repository,
        file_count: files.len(),
        languages,
        top_symbols,
        risks,
        hints,
    })
}

fn summarize_languages(files: &FileSet) -> Vec<LanguageSummary> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for file in files.files() {
        if let Some(language) = Language::detect(file) {
            *counts
                .entry(language.display_name().to_string())
                .or_default() += 1;
        }
    }

    counts
        .into_iter()
        .map(|(language, files)| LanguageSummary { language, files })
        .collect()
}

fn summarize_risks(ctx: &AnalysisContext<'_>, limit: usize) -> Vec<RiskSummary> {
    let mut risks = Vec::new();

    if let Ok(satd) = satd::Analyzer::default().analyze(ctx) {
        risks.extend(satd.items.into_iter().map(|item| RiskSummary {
            kind: "satd".to_string(),
            file: item.file,
            line: item.line,
            message: item.text,
            severity: format!("{:?}", item.severity).to_lowercase(),
        }));
    }

    if let Ok(complexity) = complexity::Analyzer::default().analyze(ctx) {
        let mut functions: Vec<_> = complexity
            .files
            .into_iter()
            .flat_map(|file| file.functions)
            .collect();
        functions.sort_by_key(|func| std::cmp::Reverse(func.metrics.cognitive));
        risks.extend(functions.into_iter().take(limit).filter_map(|func| {
            if func.metrics.cognitive < 15 && func.metrics.cyclomatic < 10 {
                return None;
            }
            Some(RiskSummary {
                kind: "complexity".to_string(),
                file: func.file,
                line: func.start_line,
                message: format!(
                    "{} has cyclomatic {} and cognitive {} complexity",
                    func.name, func.metrics.cyclomatic, func.metrics.cognitive
                ),
                severity: if func.metrics.cognitive >= 30 || func.metrics.cyclomatic >= 20 {
                    "high"
                } else {
                    "medium"
                }
                .to_string(),
            })
        }));
    }

    risks.truncate(limit);
    risks
}

fn build_hints(
    files: &FileSet,
    languages: &[LanguageSummary],
    risks: &[RiskSummary],
) -> Vec<String> {
    let mut hints = Vec::new();
    if let Some(primary) = languages.iter().max_by_key(|lang| lang.files) {
        hints.push(format!(
            "Start with the {} files; they are the largest language slice.",
            primary.language
        ));
    }
    if let Some(first) = files.files().first() {
        hints.push(format!(
            "Use `{}` as an initial navigation anchor.",
            first.display()
        ));
    }
    if let Some(risk) = risks.first() {
        hints.push(format!(
            "Review `{}` near line {} first for {} risk.",
            risk.file, risk.line, risk.kind
        ));
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_context_includes_repo_map_and_risks() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src/lib.rs"),
            "pub fn entrypoint() { if true { println!(\"hi\"); } }\n// TODO: remove shortcut\n",
        )
        .unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let context = build_context(temp.path(), &files, &config, None, None).unwrap();

        assert_eq!(context.file_count, 1);
        assert_eq!(context.languages[0].language, "Rust");
        assert!(context.top_symbols.iter().any(|s| s.name == "entrypoint"));
        assert!(context.risks.iter().any(|r| r.kind == "satd"));
        assert!(context.hints.iter().any(|h| h.contains("Start with")));
    }
}
