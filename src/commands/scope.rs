//! The files that a command reads, and the context that an analyzer gets.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::cli::AnalyzerArgs;
use crate::config::Config;
use crate::core::{AnalysisContext, ContentSource, Error, FileSet, Result, TreeSource};

/// The options that select files, and the page of the result.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub glob: Option<String>,
    pub exclude: Vec<String>,
    pub changed_since: Option<String>,
    pub top: Option<usize>,
    pub offset: Option<usize>,
}

impl From<&AnalyzerArgs> for Scope {
    fn from(args: &AnalyzerArgs) -> Self {
        Self {
            glob: args.glob.clone(),
            exclude: args.exclude.clone(),
            changed_since: args.changed_since.clone(),
            top: args.pagination.top,
            offset: args.pagination.offset,
        }
    }
}

/// Build an `AnalysisContext` for the given path, including repository root
/// discovery.
pub fn build_context<'a>(
    path: &'a PathBuf,
    file_set: &'a FileSet,
    config: &'a Config,
) -> AnalysisContext<'a> {
    let mut ctx = AnalysisContext::new(file_set, config, Some(path));
    if let Ok(repo) = crate::git::GitRepo::open(path) {
        let root = repo.root().to_path_buf();
        ctx = ctx.with_git_path(Box::leak(Box::new(root)));
    }
    ctx
}

pub fn filtered_file_set(
    path: &PathBuf,
    config: &Config,
    args: Option<&AnalyzerArgs>,
) -> Result<FileSet> {
    match args {
        Some(args) => scoped_file_set(path, config, &Scope::from(args)),
        None => FileSet::from_path(path, config),
    }
}

pub fn scoped_file_set(path: &PathBuf, config: &Config, scope: &Scope) -> Result<FileSet> {
    let mut file_set = FileSet::from_path(path, config)?;
    if let Some(changed_since) = &scope.changed_since {
        let changed_files = changed_files_since(path, changed_since)?;
        file_set = file_set.filter_by_paths(&changed_files);
    }
    if let Some(glob) = &scope.glob {
        file_set = file_set.filter_by_glob(glob);
    }
    for exclude in &scope.exclude {
        file_set = file_set.exclude_by_glob(exclude);
    }
    Ok(file_set)
}

pub fn changed_files_since(path: &Path, base: &str) -> Result<Vec<PathBuf>> {
    changed_files_between(path, base, None)
}

/// The files that changed between `base` and `target`. Without a target, the
/// files that changed between `base` and the working tree.
pub fn changed_files_between(
    path: &Path,
    base: &str,
    target: Option<&str>,
) -> Result<Vec<PathBuf>> {
    if base.starts_with('-') || target.is_some_and(|target| target.starts_with('-')) {
        return Err(Error::git(
            "changed-since reference must not start with '-'",
        ));
    }

    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            "--diff-filter=ACMR",
            "--end-of-options",
        ])
        .arg(base)
        .args(target)
        .arg("--")
        .current_dir(path)
        .output()
        .map_err(Error::Io)?;

    if !output.status.success() {
        return Err(Error::git(format!(
            "failed to list files changed since {base}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

/// The files of `symbol` and `impact`, and the source of their content. With
/// a reference, the files and their content come from that commit.
pub fn index_scope(
    path: &PathBuf,
    config: &Config,
    scope: &Scope,
    git_ref: Option<&str>,
) -> Result<(FileSet, Option<Arc<dyn ContentSource>>)> {
    let Some(reference) = git_ref else {
        return Ok((scoped_file_set(path, config, scope)?, None));
    };
    let tree = TreeSource::new(path, reference)?;
    let mut file_set = FileSet::from_tree_source(&tree, config)?;
    if let Some(base) = &scope.changed_since {
        let changed_files = changed_files_between(path, base, Some(reference))?;
        file_set = file_set.filter_by_paths(&changed_files);
    }
    if let Some(glob) = &scope.glob {
        file_set = file_set.filter_by_glob(glob);
    }
    for exclude in &scope.exclude {
        file_set = file_set.exclude_by_glob(exclude);
    }
    Ok((file_set, Some(Arc::new(tree))))
}

/// Write the report as pretty JSON to a file, if a path is present.
pub fn write_report_file(output: Option<&Path>, report: &serde_json::Value) -> Result<()> {
    if let Some(path) = output {
        std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(report)?))?;
    }
    Ok(())
}
