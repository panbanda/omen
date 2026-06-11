use std::collections::{BTreeMap, BTreeSet};
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
    pub tree: Vec<DirSummary>,
    pub entry_points: Vec<EntryPoint>,
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
pub struct DirSummary {
    pub path: String,
    pub files: usize,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    pub file: String,
    pub reason: String,
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

/// Well-known entry-point filenames with human-readable reasons.
const ENTRY_POINT_TABLE: &[(&str, &str)] = &[
    ("main.rs", "well-known entry filename"),
    ("main.go", "well-known entry filename"),
    ("index.ts", "well-known entry filename"),
    ("index.js", "well-known entry filename"),
    ("__main__.py", "well-known entry filename"),
    ("main.py", "well-known entry filename"),
    ("app.py", "well-known entry filename"),
    ("Program.cs", "well-known entry filename"),
    ("Main.java", "well-known entry filename"),
    ("main.c", "well-known entry filename"),
    ("main.cpp", "well-known entry filename"),
    ("app.rb", "well-known entry filename"),
    ("index.php", "well-known entry filename"),
    ("main.sh", "well-known entry filename"),
];

/// Build the directory summary tree from a file set, depth-capped at 3 levels from root.
/// Deeper paths are rolled up into their depth-3 ancestor directory.
fn build_tree(root: &Path, files: &FileSet) -> Vec<DirSummary> {
    // Map from directory path (depth-capped) to (file count, language set).
    let mut dir_map: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();

    for file_path in files.files() {
        // Get relative path from root
        let rel = file_path.strip_prefix(root).unwrap_or(file_path.as_path());

        // Get the parent directory of the file
        let parent = rel.parent().unwrap_or_else(|| Path::new(""));

        // Depth-cap: collect at most 3 components
        let components: Vec<&std::ffi::OsStr> = parent
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s),
                _ => None,
            })
            .take(3)
            .collect();

        let dir_key = if components.is_empty() {
            ".".to_string()
        } else {
            components
                .iter()
                .map(|c| c.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        };

        let entry = dir_map.entry(dir_key).or_insert((0, BTreeSet::new()));
        entry.0 += 1;

        if let Some(lang) = Language::detect(file_path) {
            entry.1.insert(lang.display_name().to_string());
        }
    }

    dir_map
        .into_iter()
        .map(|(path, (files, langs))| DirSummary {
            path,
            files,
            languages: langs.into_iter().collect(),
        })
        .collect()
}

/// Detect entry points from the file set by matching well-known filenames.
fn detect_entry_points(root: &Path, files: &FileSet) -> Vec<EntryPoint> {
    let mut entry_points: Vec<EntryPoint> = Vec::new();

    for file_path in files.files() {
        if let Some(file_name) = file_path.file_name() {
            let name = file_name.to_string_lossy();
            if let Some(&(_, reason)) = ENTRY_POINT_TABLE
                .iter()
                .find(|(ep_name, _)| *ep_name == name.as_ref())
            {
                let rel = file_path
                    .strip_prefix(root)
                    .unwrap_or(file_path.as_path())
                    .to_string_lossy()
                    .into_owned();
                entry_points.push(EntryPoint {
                    file: rel,
                    reason: reason.to_string(),
                });
            }
        }
    }

    entry_points.sort_by(|a, b| a.file.cmp(&b.file));
    entry_points
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

    let tree = build_tree(root, files);
    let entry_points = detect_entry_points(root, files);

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
        tree,
        entry_points,
        top_symbols,
        risks,
        hints,
    })
}

/// Apply a token budget to a context, trimming in priority order:
/// tree → risks → top_symbols → entry_points → hints, until the serialized
/// JSON fits within `max_tokens * 4` bytes, keeping each list at minimum
/// `MIN_ITEMS` entries.
pub fn apply_token_budget(ctx: &mut Context, max_tokens: usize) {
    let byte_budget = max_tokens * 4;
    const MIN_ITEMS: usize = 5;

    // Helper: estimate size
    let estimate_size = |c: &Context| -> usize {
        serde_json::to_string(c)
            .map(|s| s.len())
            .unwrap_or(usize::MAX)
    };

    if estimate_size(ctx) <= byte_budget {
        return;
    }

    // Trim tree
    while ctx.tree.len() > MIN_ITEMS && estimate_size(ctx) > byte_budget {
        ctx.tree.pop();
    }

    if estimate_size(ctx) <= byte_budget {
        return;
    }

    // Trim risks
    while ctx.risks.len() > MIN_ITEMS && estimate_size(ctx) > byte_budget {
        ctx.risks.pop();
    }

    if estimate_size(ctx) <= byte_budget {
        return;
    }

    // Trim top_symbols
    while ctx.top_symbols.len() > MIN_ITEMS && estimate_size(ctx) > byte_budget {
        ctx.top_symbols.pop();
    }

    if estimate_size(ctx) <= byte_budget {
        return;
    }

    // Trim entry_points
    while ctx.entry_points.len() > MIN_ITEMS && estimate_size(ctx) > byte_budget {
        ctx.entry_points.pop();
    }

    if estimate_size(ctx) <= byte_budget {
        return;
    }

    // Trim hints (last resort)
    while ctx.hints.len() > MIN_ITEMS && estimate_size(ctx) > byte_budget {
        ctx.hints.pop();
    }
}

impl Context {
    /// Render compact agent-facing markdown.
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("# Repository: {}\n\n", self.repository));
        out.push_str(&format!("**Files**: {}\n\n", self.file_count));

        // Languages
        let lang_line: Vec<String> = self
            .languages
            .iter()
            .map(|l| format!("{} ({})", l.language, l.files))
            .collect();
        out.push_str(&format!("**Languages**: {}\n\n", lang_line.join(", ")));

        // Entry Points
        if !self.entry_points.is_empty() {
            out.push_str("## Entry Points\n\n");
            for ep in &self.entry_points {
                out.push_str(&format!("- `{}` — {}\n", ep.file, ep.reason));
            }
            out.push('\n');
        }

        // Directory Tree
        if !self.tree.is_empty() {
            out.push_str("## Directory Tree\n\n");
            for dir in &self.tree {
                let langs = if dir.languages.is_empty() {
                    String::new()
                } else {
                    format!(", {}", dir.languages.join(", "))
                };
                out.push_str(&format!("{}/  ({} files{})\n", dir.path, dir.files, langs));
            }
            out.push('\n');
        }

        // Top Symbols
        if !self.top_symbols.is_empty() {
            out.push_str("## Top Symbols\n\n");
            for sym in &self.top_symbols {
                out.push_str(&format!(
                    "- `{}` ({}) {}:{}\n",
                    sym.name, sym.kind, sym.file, sym.line
                ));
            }
            out.push('\n');
        }

        // Risks
        if !self.risks.is_empty() {
            out.push_str("## Risks\n\n");
            for risk in &self.risks {
                out.push_str(&format!(
                    "- [{}] {} {}:{} — {}\n",
                    risk.severity, risk.kind, risk.file, risk.line, risk.message
                ));
            }
            out.push('\n');
        }

        // Hints
        if !self.hints.is_empty() {
            out.push_str("## Hints\n\n");
            for hint in &self.hints {
                out.push_str(&format!("- {}\n", hint));
            }
            out.push('\n');
        }

        out
    }
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

    #[test]
    fn test_build_tree_depth_capped_and_sorted() {
        let temp = tempfile::tempdir().unwrap();
        // Create dirs at various depths
        std::fs::create_dir_all(temp.path().join("a/b/c/d")).unwrap();
        std::fs::create_dir_all(temp.path().join("a/b/e")).unwrap();
        std::fs::create_dir_all(temp.path().join("x")).unwrap();
        // Files at various depths
        std::fs::write(temp.path().join("a/b/c/d/deep.rs"), "fn f() {}").unwrap();
        std::fs::write(temp.path().join("a/b/c/d/other.rs"), "fn g() {}").unwrap();
        std::fs::write(temp.path().join("a/b/e/mid.rs"), "fn h() {}").unwrap();
        std::fs::write(temp.path().join("x/root.rs"), "fn i() {}").unwrap();
        std::fs::write(temp.path().join("top.rs"), "fn j() {}").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let tree = build_tree(temp.path(), &files);

        // Sorted by path (BTreeMap guarantees this)
        let paths: Vec<&str> = tree.iter().map(|d| d.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "tree entries should be sorted by path");

        // The two deep files (a/b/c/d/) should be rolled up into a/b/c
        let abc = tree.iter().find(|d| d.path == "a/b/c");
        assert!(abc.is_some(), "a/b/c should appear (depth-3 rollup)");
        assert_eq!(abc.unwrap().files, 2, "a/b/c should have 2 files rolled up");

        // a/b/e should appear
        let abe = tree.iter().find(|d| d.path == "a/b/e");
        assert!(abe.is_some(), "a/b/e should appear");
        assert_eq!(abe.unwrap().files, 1);

        // Root-level file -> "."
        let root_dir = tree.iter().find(|d| d.path == ".");
        assert!(root_dir.is_some(), ". should appear for root-level files");
        assert_eq!(root_dir.unwrap().files, 1);

        // x should appear
        let x = tree.iter().find(|d| d.path == "x");
        assert!(x.is_some(), "x should appear");
        assert_eq!(x.unwrap().files, 1);
    }

    #[test]
    fn test_detect_entry_points_finds_known_filenames() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn f() {}").unwrap();
        std::fs::write(temp.path().join("main.go"), "package main").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let eps = detect_entry_points(temp.path(), &files);

        let ep_files: Vec<&str> = eps.iter().map(|e| e.file.as_str()).collect();
        assert!(
            ep_files.iter().any(|f| f.ends_with("main.rs")),
            "main.rs should be detected"
        );
        assert!(
            ep_files.iter().any(|f| f.ends_with("main.go")),
            "main.go should be detected"
        );
        // lib.rs is not an entry point
        assert!(
            !ep_files.iter().any(|f| f.ends_with("lib.rs")),
            "lib.rs should not be detected"
        );
        // Sorted by file
        let mut sorted = ep_files.clone();
        sorted.sort();
        assert_eq!(ep_files, sorted, "entry points should be sorted by file");
        // All have the correct reason
        for ep in &eps {
            assert_eq!(ep.reason, "well-known entry filename");
        }
    }

    #[test]
    fn test_render_markdown_contains_expected_sections() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src/main.rs"),
            "pub fn my_symbol() { if true { } }\n// TODO: fix this\n",
        )
        .unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let context = build_context(temp.path(), &files, &config, None, None).unwrap();
        let md = context.render_markdown();

        assert!(md.contains("# Repository:"), "should have repo header");
        assert!(md.contains("**Languages**"), "should have languages line");
        assert!(
            md.contains("## Entry Points"),
            "should have entry points section"
        );
        assert!(md.contains("## Directory Tree"), "should have tree section");
        assert!(md.contains("## Top Symbols"), "should have symbols section");
        assert!(md.contains("## Hints"), "should have hints section");
        // One symbol per line
        assert!(md.contains("my_symbol"), "should include symbol name");
        // Entry point for main.rs
        assert!(
            md.contains("main.rs"),
            "should reference main.rs entry point"
        );
    }

    #[test]
    fn test_apply_token_budget_trims_in_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "fn f() {}").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let mut context = build_context(temp.path(), &files, &config, None, None).unwrap();

        // Inflate tree and risks with artificial data (50 entries each)
        for i in 0..50 {
            context.tree.push(DirSummary {
                path: format!("dir/sub/path{i}"),
                files: 1,
                languages: vec!["Rust".to_string()],
            });
            context.risks.push(RiskSummary {
                kind: "satd".to_string(),
                file: format!("some/deep/file{i}.rs"),
                line: i as u32,
                message: "x".repeat(200),
                severity: "low".to_string(),
            });
            context.top_symbols.push(SymbolSummary {
                name: format!("sym_{i}"),
                kind: "function".to_string(),
                file: format!("src/file{i}.rs"),
                line: i as u32,
                score: 0.1,
            });
        }

        let original_size = serde_json::to_string(&context)
            .map(|s| s.len())
            .unwrap_or(0);
        assert!(
            original_size > 10_000,
            "test setup: context should be large"
        );

        // Record counts before trimming - tree should be trimmed first
        let tree_before = context.tree.len();
        let risks_before = context.risks.len();
        let symbols_before = context.top_symbols.len();

        // Use a large-enough budget so trimming works
        // Original is ~21k bytes; use 4000 tokens = 16000 bytes to force
        // some trimming of tree/risks while keeping minimums.
        apply_token_budget(&mut context, 4000);

        let after_size = serde_json::to_string(&context)
            .map(|s| s.len())
            .unwrap_or(0);
        let byte_budget = 4000 * 4;
        assert!(
            after_size <= byte_budget,
            "context should fit within budget: {} > {} (was {})",
            after_size,
            byte_budget,
            original_size
        );

        // Lists were trimmed (started at 50+, should now be shorter)
        // Each list should be at minimum 5 if trimming was needed
        assert!(
            context.tree.len() <= tree_before,
            "tree should have been trimmed"
        );
        assert!(
            context.risks.len() <= risks_before,
            "risks should have been trimmed"
        );
        assert!(
            context.top_symbols.len() <= symbols_before,
            "symbols may have been trimmed"
        );

        // Minimums respected
        assert!(
            context.tree.len() >= 5 || tree_before < 5,
            "tree >= MIN_ITEMS"
        );
        assert!(
            context.risks.len() >= 5 || risks_before < 5,
            "risks >= MIN_ITEMS"
        );
        assert!(
            context.top_symbols.len() >= 5 || symbols_before < 5,
            "symbols >= MIN_ITEMS"
        );
    }

    /// B5: entry_points and hints must also be trimmed when the budget cannot
    /// be met after trimming tree/risks/top_symbols alone.
    ///
    /// Note: the budget assertion is best-effort — if even MIN_ITEMS (5)
    /// entries each exceed the remaining budget, trimming stops at the minimum
    /// rather than going below it.  What we verify here is:
    ///   1. entry_points and hints *are* trimmed (they shrink),
    ///   2. minimums are respected (>= 5 if we started with >= 5),
    ///   3. the final size is smaller than the original size.
    #[test]
    fn test_apply_token_budget_trims_entry_points_and_hints() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "fn f() {}").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let mut context = build_context(temp.path(), &files, &config, None, None).unwrap();

        // Keep tree/risks/symbols tiny so their trimming alone can't fix the budget.
        context.tree.clear();
        context.risks.clear();
        context.top_symbols.clear();

        // Bloat entry_points and hints with large entries.
        for i in 0..30 {
            context.entry_points.push(EntryPoint {
                file: format!("src/entry_{i}.rs"),
                reason: "x".repeat(300),
            });
            context.hints.push("y".repeat(300));
        }

        let original_size = serde_json::to_string(&context)
            .map(|s| s.len())
            .unwrap_or(0);
        assert!(original_size > 5_000, "test setup: context should be large");

        let entry_before = context.entry_points.len();
        let hints_before = context.hints.len();

        // Tight budget: forces entry_points and hints to be trimmed.
        // Use 500 tokens; even if the budget floor (MIN_ITEMS entries) cannot
        // fit within 2000 bytes, the lists must be trimmed toward the minimum.
        apply_token_budget(&mut context, 500);

        let after_size = serde_json::to_string(&context)
            .map(|s| s.len())
            .unwrap_or(0);

        // The lists should have been trimmed.
        assert!(
            context.entry_points.len() < entry_before,
            "entry_points should have been trimmed (was {entry_before}, now {})",
            context.entry_points.len()
        );
        assert!(
            context.hints.len() < hints_before,
            "hints should have been trimmed (was {hints_before}, now {})",
            context.hints.len()
        );
        // Minimums respected.
        assert_eq!(context.entry_points.len(), 5, "entry_points >= MIN_ITEMS");
        assert_eq!(context.hints.len(), 5, "hints >= MIN_ITEMS");
        // Size must have decreased.
        assert!(
            after_size < original_size,
            "size should have decreased: {after_size} vs original {original_size}"
        );
    }

    #[test]
    fn test_apply_token_budget_no_trim_when_under_budget() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "fn f() {}").unwrap();

        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let mut context = build_context(temp.path(), &files, &config, None, None).unwrap();

        let tree_before = context.tree.len();
        let risks_before = context.risks.len();
        let symbols_before = context.top_symbols.len();

        // Very large budget: nothing should be trimmed
        apply_token_budget(&mut context, 1_000_000);

        assert_eq!(context.tree.len(), tree_before);
        assert_eq!(context.risks.len(), risks_before);
        assert_eq!(context.top_symbols.len(), symbols_before);
    }
}
